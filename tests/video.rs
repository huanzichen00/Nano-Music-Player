use nano_music_player::{
    app::{
        action::Action,
        i18n::{self, Language},
        navigation::Screen,
        state::{Effect, State},
    },
    audio::{PlaybackStatus, engine::AudioCommand},
    library::{Track, lyrics::Lyrics},
    video::{Status, engine::Command, library::Video},
};
use std::path::PathBuf;

fn video(name: &str, aspect: f64) -> Video {
    Video {
        path: PathBuf::from(format!("/generated/{name}.mp4")),
        title: name.into(),
        duration: 12.0,
        width: 320,
        height: 180,
        display_aspect: aspect,
        video_codec: "h264".into(),
        audio_codec: Some("aac".into()),
    }
}

#[test]
fn rotation_tracks_display_aspect_and_navigation_without_changing_music_or_zoom() {
    let mut state = State::default();
    state.merge_library(vec![Track {
        path: "/generated/music.wav".into(),
        title: "Music".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        genre: "Test".into(),
        number: 1,
        duration: 120.0,
        artwork: vec![],
        lyrics: Lyrics::default(),
    }]);
    state.queue.set(vec![0], 0, false);
    state.start(0);
    state.position = 32.0;
    state.zoom = 1.75;
    state.merge_videos(vec![
        video("Landscape", 16.0 / 9.0),
        video("Portrait", 9.0 / 16.0),
        video("Square", 1.0),
    ]);
    let effects = state.start_video(0);
    assert!(matches!(
        effects.as_slice(),
        [
            Effect::Video(Command::Load { generation: 1, .. }),
            Effect::Video(Command::Subtitle {
                generation: 1,
                preference: nano_music_player::video::subtitles::SubtitlePreference::Automatic,
            }),
        ]
    ));
    assert!(state.landscape_video());
    state.apply(Action::TogglePlay);
    assert!(state.landscape_video());
    state.video_status = Status::Finished;
    assert!(state.landscape_video());
    state.apply(Action::Back);
    assert_eq!(state.navigation.current().screen, Screen::Videos);
    assert!(!state.landscape_video());
    assert_eq!(
        (state.current, state.queue.current(), state.position),
        (Some(0), Some(0), 32.0)
    );
    assert_eq!((state.zoom, state.status), (1.75, PlaybackStatus::Paused));
    state.apply(Action::NowPlaying);
    assert!(state.landscape_video());
    state.apply(Action::Home);
    assert!(!state.landscape_video());
    state.start_video(1);
    assert!(!state.landscape_video());
    state.start_video(2);
    assert!(!state.landscape_video());
    state.start(0);
    assert!(!state.video_active);
    assert!(matches!(
        state.set_volume(0.3),
        Effect::Audio(AudioCommand::Volume(_))
    ));
}

#[test]
fn eof_replay_and_seek_reopen_while_transport_uses_video_generations() {
    let mut state = State::default();
    state.merge_videos(vec![video("Landscape", 16.0 / 9.0)]);
    state.start_video(0);
    assert!(
        matches!(state.set_volume(0.3), Effect::Video(Command::Volume { generation: 1, volume }) if (volume - 0.3).abs() < 0.001)
    );
    assert!(matches!(
        state.apply(Action::ToggleMute).as_slice(),
        [Effect::Video(Command::Volume {
            generation: 1,
            volume: 0.0
        })]
    ));
    assert!(matches!(
        state.seek(4.0),
        Effect::Video(Command::Seek {
            generation: 1,
            seconds: 4.0
        })
    ));
    state.video_status = Status::Finished;
    let replay = state.apply(Action::TogglePlay);
    assert!(matches!(
        replay.as_slice(),
        [
            Effect::Video(Command::Load {
                generation: 2,
                start: 0.0,
                paused: false,
                ..
            }),
            Effect::Video(Command::Subtitle {
                generation: 2,
                preference: nano_music_player::video::subtitles::SubtitlePreference::Automatic,
            }),
        ]
    ));
    state.video_status = Status::Finished;
    assert!(matches!(
        state.seek(3.0),
        Effect::Video(Command::Load {
            generation: 3,
            start: 3.0,
            paused: true,
            ..
        })
    ));
    state.video_status = Status::Error;
    assert!(matches!(
        state.seek(f64::NAN),
        Effect::Video(Command::Load {
            generation: 4,
            start: 0.0,
            paused: true,
            ..
        })
    ));
    // A background import must not retarget the active index.
    state.merge_videos(vec![
        video("Alphabetically earlier", 1.0),
        video("Landscape", 16.0 / 9.0),
    ]);
    assert_eq!(state.video_current, Some(0));
    assert_eq!(state.videos[0].title, "Landscape");
}

#[test]
fn video_import_and_error_labels_are_localized_but_details_are_preserved() {
    assert_eq!(
        i18n::translate(Language::SimplifiedChinese, "Import Videos…"),
        "导入视频…"
    );
    assert_eq!(
        i18n::notice(
            Language::Japanese,
            "Imported 2 videos · 1 errors: broken.mp4"
        ),
        "ビデオを2本読み込みました · 1件のエラー：broken.mp4"
    );
    assert_eq!(
        i18n::notice(
            Language::SimplifiedChinese,
            "Video import failed: missing ffprobe"
        ),
        "视频导入失败： missing ffprobe"
    );
}

#[test]
fn keyboard_seek_uses_active_video_without_a_previous_music_track() {
    let mut state = State::default();
    assert!(state.apply(Action::SeekForward).is_empty());
    state.merge_videos(vec![video("Landscape", 16.0 / 9.0)]);
    state.start_video(0);
    assert_eq!(state.current, None);
    state.video_status = Status::Paused;
    state.video_paused = true;
    state.video_position = 6.0;
    assert!(matches!(
        state.apply(Action::SeekBackward).as_slice(),
        [Effect::Video(Command::Seek {
            generation: 1,
            seconds: 1.0
        })]
    ));
    assert!(matches!(
        state.apply(Action::SeekForward).as_slice(),
        [Effect::Video(Command::Seek {
            generation: 1,
            seconds: 6.0
        })]
    ));
    state.video_status = Status::Finished;
    state.video_position = 12.0;
    assert!(matches!(
        state.apply(Action::SeekBackward).as_slice(),
        [Effect::Video(Command::Load {
            generation: 2,
            start: 7.0,
            paused: true,
            ..
        })]
    ));
    assert_eq!(state.queue.current(), None);
    assert_eq!(state.position, 0.0);
    state.video_active = false;
    assert!(state.apply(Action::SeekForward).is_empty());
}
