use super::*;
use crate::library::{database::Database, lyrics::Lyrics};
use std::{fs, path::Path, time::Duration};

fn fixture(directory: &Path) -> (Controller, Vec<PathBuf>) {
    let root = directory.join("music");
    fs::create_dir(&root).unwrap();
    let paths: Vec<_> = (0..3).map(|i| root.join(format!("{i}.wav"))).collect();
    for path in &paths {
        fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quiet.wav"),
            path,
        )
        .unwrap();
    }
    let database = directory.join("library.sqlite3");
    let tracks = scanner::scan_folders(&database, &[root]).unwrap().tracks;
    let mut controller = Controller::new(directory.to_owned());
    controller.state.merge_library(tracks);
    controller.state.navigation.open(Screen::Songs(Filter::All));
    (controller, paths)
}

fn refreshed(directory: &Path) -> Prepared<crate::library::refresh::Snapshot> {
    thumbnails::refresh(
        directory.join("library.sqlite3"),
        vec![directory.join("music")],
    )
    .recv_timeout(Duration::from_secs(5))
    .unwrap()
    .unwrap()
}

#[test]
fn refresh_preserves_queue_slots_selection_and_manual_lyrics_across_removal_and_restore() {
    let directory = tempfile::tempdir().unwrap();
    let (mut controller, paths) = fixture(directory.path());
    let database = Database::open(&directory.path().join("library.sqlite3")).unwrap();
    let lyrics = Lyrics::plain_text("manual lyrics survive refresh");
    database
        .set_manual_lyrics(&paths[1], &directory.path().join("chosen.txt"), &lyrics)
        .unwrap();
    controller.state.tracks[1].lyrics = lyrics.clone();
    controller.state.queue.set(vec![0, 1, 2], 0, false);
    controller.state.current = Some(0);
    controller.state.status = PlaybackStatus::Playing;
    let selected = controller
        .state
        .rows()
        .iter()
        .position(|row| matches!(row.target, Target::Play(2)))
        .unwrap();
    controller.state.navigation.select(selected);
    fs::remove_file(&paths[1]).unwrap();
    controller.apply_library_refresh(refreshed(directory.path()));
    assert_eq!(controller.state.queue.order, [0, 1, 2]);
    assert_eq!(controller.state.current, Some(0));
    assert!(matches!(
        controller.state.rows()[controller.state.navigation.selected()].target,
        Target::Play(2)
    ));
    assert!(
        !controller
            .state
            .track_available(&controller.state.tracks[1])
    );
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quiet.wav"),
        &paths[1],
    )
    .unwrap();
    controller.apply_library_refresh(refreshed(directory.path()));
    assert!(
        controller
            .state
            .track_available(&controller.state.tracks[1])
    );
    assert_eq!(controller.state.tracks[1].lyrics, lyrics);
    assert_eq!(controller.state.queue.order, [0, 1, 2]);
    fs::remove_file(&paths[0]).unwrap();
    controller.apply_library_refresh(refreshed(directory.path()));
    assert_eq!(controller.state.current, None);
    assert_eq!(controller.state.status, PlaybackStatus::Stopped);
    assert_eq!(controller.state.queue.order, [0, 1, 2]);
}

#[test]
fn refresh_does_not_overwrite_a_lyric_edit_completed_after_its_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let (mut controller, _) = fixture(directory.path());
    let prepared = refreshed(directory.path());
    let edited = Lyrics::plain_text("newer UI lyric edit");
    controller.state.tracks[0].lyrics = edited.clone();
    controller.apply_library_refresh(prepared);
    assert_eq!(controller.state.tracks[0].lyrics, edited);
}

#[test]
fn refresh_completion_preserves_an_error_from_a_newer_user_operation() {
    let directory = tempfile::tempdir().unwrap();
    let (mut controller, _) = fixture(directory.path());
    let prepared = refreshed(directory.path());
    controller.state.notice = "Could not save rating".into();
    controller.apply_library_refresh(prepared);
    assert_eq!(controller.state.notice, "Could not save rating");
}

#[test]
fn missing_previous_music_does_not_stop_or_cancel_the_podcast_or_video_owner() {
    for podcast in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let (mut controller, paths) = fixture(directory.path());
        controller.state.current = Some(0);
        controller.handoff = 9;
        controller.state.video_active = !podcast;
        if podcast {
            controller.state.podcasts.active = Some(1);
            controller.state.podcasts.status = PlaybackStatus::Playing;
            controller.pending_media = Some(PendingMedia::Music {
                handoff: 9,
                command: AudioCommand::Pause(false),
            });
        } else {
            controller.pending_media = Some(PendingMedia::Video {
                handoff: 9,
                command: crate::video::engine::Command::Pause {
                    generation: 1,
                    paused: false,
                },
            });
        }
        fs::remove_file(&paths[0]).unwrap();
        controller.apply_library_refresh(refreshed(directory.path()));
        assert_eq!(controller.state.current, None);
        assert_eq!(
            controller.handoff, 9,
            "no Stop command should be sent to the other owner"
        );
        assert!(controller.pending_media.is_some());
        if podcast {
            assert_eq!(controller.state.podcasts.active, Some(1));
            assert_eq!(controller.state.podcasts.status, PlaybackStatus::Playing);
        } else {
            assert!(controller.state.video_active);
        }
    }
}
