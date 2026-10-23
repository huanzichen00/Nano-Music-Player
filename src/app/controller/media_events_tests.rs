use super::*;
use crate::{
    app::{controller::PendingMedia, state::Effect},
    audio::{PlaybackStatus, engine::AudioCommand},
    library::{Track, lyrics::Lyrics, statistics::StatisticsStore},
    video::subtitles::SubtitlePreference,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn fixture(directory: &Path) -> Controller {
    let mut controller = Controller::new(directory.to_owned());
    controller.state.tracks = (0..2)
        .map(|id| Track {
            path: directory.join(format!("track-{id}.wav")),
            title: format!("Track {id}"),
            artist: "Artist".into(),
            album: "Album".into(),
            genre: String::new(),
            number: id as u32,
            duration: 120.0,
            artwork: Vec::new(),
            lyrics: if id == 0 {
                Lyrics::plain_text("Original lyrics")
            } else {
                Lyrics::default()
            },
        })
        .collect();
    controller.state.queue.set(vec![0, 1], 0, false);
    controller.state.current = Some(0);
    controller.state.status = PlaybackStatus::Playing;
    controller.state.position = 8.0;
    // These tests inject authoritative events without loading an audio source.
    controller.state.gapless_playback = false;
    controller.state.notice = "Existing playback notice".into();
    controller
}

fn progress(id: usize, position: f64, observed_at: Instant) -> AudioEvent {
    AudioEvent::Progress {
        id,
        status: PlaybackStatus::Playing,
        position,
        observed_at,
    }
}

#[test]
fn audio_event_guards_preserve_the_active_owner_and_pause_intent() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();
    for (video_active, pending, event_id) in [(false, false, 1), (true, false, 0), (false, true, 0)]
    {
        let directory = tempfile::tempdir().unwrap();
        let mut controller = fixture(directory.path());
        controller.begin_playback_session(0);
        controller.state.video_active = video_active;
        controller.rows_dirty = false;
        controller.lyrics_dirty = false;
        controller.last_track = Some(0);
        if pending {
            controller.pending_media = Some(PendingMedia::Music {
                handoff: 11,
                command: AudioCommand::Pause(false),
            });
            controller.pending_seek = Some(12.5);
        }
        for event in [
            progress(event_id, 99.0, Instant::now()),
            AudioEvent::Error {
                id: Some(event_id),
                message: "Late playback error".into(),
            },
            AudioEvent::Finished(event_id),
            AudioEvent::Transitioned {
                from: event_id,
                to: 1,
                token: 7,
            },
        ] {
            controller.handle_audio_event(&ui, event);
        }
        assert_eq!(controller.state.current, Some(0));
        assert_eq!(controller.state.position, 8.0);
        assert_eq!(controller.state.status, PlaybackStatus::Playing);
        assert_eq!(controller.state.queue.position, 0);
        assert_eq!(controller.state.notice, "Existing playback notice");
        assert_eq!(controller.last_track, Some(0));
        assert!(!controller.rows_dirty);
        assert!(!controller.lyrics_dirty);
        assert_eq!(
            controller.playback_session.as_ref().unwrap().listened(),
            Duration::ZERO
        );
        assert_eq!(controller.pending_media.is_some(), pending);
        assert_eq!(controller.pending_seek, pending.then_some(12.5));
    }

    let directory = tempfile::tempdir().unwrap();
    let mut controller = fixture(directory.path());
    controller.state.paused = true;
    controller.state.status = PlaybackStatus::Paused;
    controller.begin_playback_session(0);
    controller.handle_audio_event(&ui, progress(0, 9.0, Instant::now()));
    assert_eq!(controller.state.position, 9.0);
    assert_eq!(controller.state.status, PlaybackStatus::Paused);
    assert_eq!(
        controller.playback_session.as_ref().unwrap().listened(),
        Duration::ZERO
    );

    controller.state.podcasts.active = Some(7);
    controller.state.podcasts.position = 33.0;
    controller.state.podcasts.paused = false;
    controller.state.podcasts.status = PlaybackStatus::Playing;
    controller.handle_audio_event(&ui, progress(0, 77.0, Instant::now()));
    assert_eq!(controller.state.position, 9.0);
    assert_eq!(controller.state.podcasts.position, 33.0);
    controller.handle_audio_event(
        &ui,
        AudioEvent::Error {
            id: None,
            message: "Output unavailable".into(),
        },
    );
    assert_eq!(controller.state.status, PlaybackStatus::Paused);
    assert_eq!(controller.state.podcasts.status, PlaybackStatus::Error);
    controller.handle_audio_event(
        &ui,
        progress(crate::podcast::AUDIO_ID, 34.0, Instant::now()),
    );
    assert_eq!(controller.state.position, 9.0);
    assert_eq!(controller.state.podcasts.position, 34.0);
    assert_eq!(controller.state.podcasts.status, PlaybackStatus::Playing);
}

#[test]
fn stale_and_wrong_owner_acknowledgements_preserve_pending_handoffs() {
    let directory = tempfile::tempdir().unwrap();
    let mut controller = fixture(directory.path());
    controller.pending_media = Some(PendingMedia::Music {
        handoff: 20,
        command: AudioCommand::Pause(false),
    });
    controller.pending_seek = Some(17.0);
    controller.pending_subtitle = Some(SubtitlePreference::Off);
    controller.finish_music_handoff(19);
    controller.finish_video_handoff(20);
    assert!(matches!(
        controller.pending_media,
        Some(PendingMedia::Music { handoff: 20, .. })
    ));
    assert_eq!(controller.pending_seek, Some(17.0));
    assert_eq!(controller.pending_subtitle, Some(SubtitlePreference::Off));

    controller.pending_media = Some(PendingMedia::Video {
        handoff: 21,
        command: crate::video::engine::Command::Pause {
            generation: 4,
            paused: false,
        },
    });
    controller.pending_seek = Some(23.0);
    controller.finish_video_handoff(20);
    controller.finish_music_handoff(21);
    assert!(matches!(
        controller.pending_media,
        Some(PendingMedia::Video { handoff: 21, .. })
    ));
    assert_eq!(controller.pending_seek, Some(23.0));
    assert_eq!(controller.pending_subtitle, Some(SubtitlePreference::Off));
}

#[test]
fn duplicate_queue_transition_accepts_an_authoritative_late_token() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let mut controller = fixture(directory.path());
    controller.state.queue.set(vec![0, 1, 0], 0, false);
    controller
        .state
        .unavailable_tracks
        .insert(controller.state.tracks[1].path.clone());
    controller.listening_clock = Instant::now() - Duration::from_secs(5);
    controller.begin_playback_session(0);
    controller.update_playback_session(
        0,
        PlaybackStatus::Playing,
        0.0,
        controller.listening_clock + Duration::from_secs(1),
    );
    controller.update_playback_session(
        0,
        PlaybackStatus::Playing,
        1.0,
        controller.listening_clock + Duration::from_secs(2),
    );
    assert_eq!(
        controller.playback_session.as_ref().unwrap().listened(),
        Duration::from_secs(1)
    );
    controller.state.paused = true;
    controller.state.status = PlaybackStatus::Paused;
    controller.state.lyrics_visible = false;
    controller.state.overlay_visible = false;
    controller.last_track = Some(0);
    controller.lyrics_dirty = false;
    controller.rows_dirty = false;
    controller.prefetched = Some((0, 0, 7));
    controller.failed_prefetch = Some((0, 1));
    // A successor may already have started when repeat/shuffle/gain intent changes.
    // Engine generation filtering makes this event authoritative despite its token.
    controller.handle_audio_event(
        &ui,
        AudioEvent::Transitioned {
            from: 0,
            to: 0,
            token: 6,
        },
    );
    assert_eq!(controller.state.queue.order, [0, 1, 0]);
    assert_eq!(controller.state.queue.position, 2);
    assert_eq!(controller.state.current, Some(0));
    assert_eq!(controller.state.position, 0.0);
    assert_eq!(controller.state.status, PlaybackStatus::Paused);
    assert!(controller.state.lyrics_visible);
    assert!(controller.state.overlay_visible);
    assert!(controller.last_track.is_none());
    assert!(controller.lyrics_dirty);
    assert!(controller.rows_dirty);
    assert!(controller.prefetched.is_none());
    assert!(controller.failed_prefetch.is_none());
    let session = controller.playback_session.as_ref().unwrap();
    assert_eq!(session.path(), controller.state.tracks[0].path);
    assert_eq!(session.listened(), Duration::ZERO);
    assert!(!session.counted());
}

#[test]
fn prefetch_failure_requires_both_the_current_track_id_and_token() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let mut controller = fixture(directory.path());
    controller.prefetched = Some((0, 1, 14));
    for (id, token) in [(1, 13), (0, 14)] {
        controller.handle_audio_event(
            &ui,
            AudioEvent::PrefetchFailed {
                id,
                token,
                message: "Stale decode failure".into(),
            },
        );
        assert_eq!(controller.prefetched, Some((0, 1, 14)));
        assert!(controller.failed_prefetch.is_none());
        assert_eq!(controller.state.notice, "Existing playback notice");
    }
    controller.handle_audio_event(
        &ui,
        AudioEvent::PrefetchFailed {
            id: 1,
            token: 14,
            message: "Current decode failure".into(),
        },
    );
    assert!(controller.prefetched.is_none());
    assert_eq!(controller.failed_prefetch, Some((0, 1)));
    assert_eq!(
        controller.state.notice,
        "Could not prepare next track: Current decode failure"
    );
    assert_eq!(controller.state.current, Some(0));
    assert_eq!(controller.state.position, 8.0);
    assert_eq!(controller.state.status, PlaybackStatus::Playing);
    assert_eq!(controller.state.queue.order, [0, 1]);
}

#[test]
fn sampled_session_survives_pause_seek_and_persists_one_play_on_shutdown() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let mut controller = fixture(directory.path());
    let origin = Instant::now() - Duration::from_secs(40);
    controller.listening_clock = origin;
    controller.begin_playback_session(0);
    for (seconds, position) in [(1, 0.0), (31, 30.0), (32, 31.0)] {
        controller.handle_audio_event(
            &ui,
            progress(0, position, origin + Duration::from_secs(seconds)),
        );
    }
    let session = controller.playback_session.as_ref().unwrap();
    assert_eq!(session.listened(), Duration::from_secs(31));
    assert!(session.counted());

    // An unloaded engine receives only Pause/Seek commands, which never open output.
    controller.state.paused = true;
    controller.state.status = PlaybackStatus::Paused;
    controller.send_music(AudioCommand::Pause(true));
    controller.handle_audio_event(&ui, progress(0, 32.0, Instant::now()));
    assert_eq!(controller.state.status, PlaybackStatus::Paused);
    assert_eq!(
        controller.playback_session.as_ref().unwrap().listened(),
        Duration::from_secs(31)
    );
    controller.state.paused = false;
    controller.state.status = PlaybackStatus::Playing;
    controller.send_music(AudioCommand::Pause(false));
    let before_seek = Instant::now();
    controller.handle_audio_event(&ui, progress(0, 32.0, before_seek));
    let Effect::Audio(seek) = controller.state.seek(64.0) else {
        panic!("music seek must emit an audio command");
    };
    controller.send_music(seek);
    // A queued sample predating the seek cannot earn credit or reset its baseline.
    controller.handle_audio_event(&ui, progress(0, 33.0, before_seek));
    assert_eq!(
        controller.playback_session.as_ref().unwrap().listened(),
        Duration::from_secs(31)
    );
    controller.complete_playback_session();
    controller.complete_playback_session();
    controller.statistics.shutdown();

    let store = StatisticsStore::open(&directory.path().join("library.sqlite3")).unwrap();
    let path = &controller.state.tracks[0].path;
    assert_eq!(store.track_statistics(path).unwrap().unwrap().play_count, 1);
    let history = store.listening_history(10).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(&history[0].path, path);
    assert_eq!(history[0].listened_seconds, 30);
}
