//! Historical engine comparison. Native tests are opt-in and never write media.
use nano_music_player::audio::{PlaybackStatus, engine as current};
use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
#[rustfmt::skip]
#[path = "support/legacy_engine.rs"]
mod legacy;

enum Player {
    Legacy(legacy::Engine),
    Current(current::Engine),
}
#[derive(Debug)]
enum Event {
    Progress(usize, PlaybackStatus, f64),
    Finished(usize),
    Stopped(u64),
    Error(String),
    Other,
}
impl Player {
    fn load(&self, id: usize, path: &Path, paused: bool) {
        match self {
            Self::Legacy(e) => e
                .send(legacy::AudioCommand::Load {
                    id,
                    path: path.into(),
                    volume: 0.05,
                    paused,
                })
                .unwrap(),
            Self::Current(e) => e
                .send(current::AudioCommand::Load {
                    id,
                    path: path.into(),
                    volume: 0.05,
                    paused,
                })
                .unwrap(),
        }
    }
    fn volume(&self, volume: f32) {
        match self {
            Self::Legacy(e) => e.send(legacy::AudioCommand::Volume(volume)).unwrap(),
            Self::Current(e) => e.send(current::AudioCommand::Volume(volume)).unwrap(),
        }
    }
    fn pause(&self, paused: bool) {
        match self {
            Self::Legacy(e) => e.send(legacy::AudioCommand::Pause(paused)).unwrap(),
            Self::Current(e) => e.send(current::AudioCommand::Pause(paused)).unwrap(),
        }
    }
    fn seek(&self, seconds: f64) {
        match self {
            Self::Legacy(e) => e.send(legacy::AudioCommand::Seek(seconds)).unwrap(),
            Self::Current(e) => e.send(current::AudioCommand::Seek(seconds)).unwrap(),
        }
    }
    fn stop(&self, handoff: u64) {
        match self {
            Self::Legacy(e) => e.send(legacy::AudioCommand::Stop { handoff }).unwrap(),
            Self::Current(e) => e.send(current::AudioCommand::Stop { handoff }).unwrap(),
        }
    }
    fn event(&self) -> Option<Event> {
        match self {
            Self::Legacy(e) => e.events.try_recv().ok().map(|event| match event {
                legacy::AudioEvent::Progress {
                    id,
                    status,
                    position,
                } => Event::Progress(id, status, position),
                legacy::AudioEvent::Finished(id) => Event::Finished(id),
                legacy::AudioEvent::Stopped { handoff } => Event::Stopped(handoff),
                legacy::AudioEvent::Error { id, message } => {
                    Event::Error(format!("{id:?}: {message}"))
                }
            }),
            Self::Current(e) => e.events.try_recv().ok().map(|event| match event {
                current::AudioEvent::Progress {
                    id,
                    status,
                    position,
                    ..
                } => Event::Progress(id, status, position),
                current::AudioEvent::Finished(id) => Event::Finished(id),
                current::AudioEvent::Stopped { handoff } => Event::Stopped(handoff),
                current::AudioEvent::Error { id, message } => {
                    Event::Error(format!("{id:?}: {message}"))
                }
                _ => Event::Other,
            }),
        }
    }
    fn wait(&self, label: &str, accepts: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut last = None;
        while Instant::now() < deadline {
            while let Some(event) = self.event() {
                if let Event::Error(message) = &event {
                    panic!("{label}: {message}")
                }
                if accepts(&event) {
                    return event;
                }
                last = Some(event);
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("{label}: no native progress/acknowledgement within 4s; last={last:?}");
    }
}
fn fixture(path: &Path) {
    let original = include_bytes!("fixtures/quiet.wav");
    let data = &original[44..];
    let len = data.len() * 16;
    let mut bytes = original[..44].to_vec();
    bytes[4..8].copy_from_slice(&((len + 36) as u32).to_le_bytes());
    bytes[40..44].copy_from_slice(&(len as u32).to_le_bytes());
    for _ in 0..16 {
        bytes.extend_from_slice(data);
    }
    std::fs::write(path, bytes).unwrap();
}
fn transport(player: Player) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("four-seconds.wav");
    fixture(&path);
    player.load(1, &path, true);
    player.wait("initial paused load", |e| {
        matches!(e, Event::Progress(1, PlaybackStatus::Paused, _))
    });
    player.seek(1.0);
    player.wait(
        "paused seek",
        |e| matches!(e,Event::Progress(1,PlaybackStatus::Paused,p) if (*p-1.0).abs()<0.04),
    );
    player.volume(0.05);
    player.pause(false);
    player.wait(
        "real output consumes samples",
        |e| matches!(e,Event::Progress(1,PlaybackStatus::Playing,p) if *p>1.3),
    );
    player.pause(true);
    let event = player.wait("pause acknowledgement", |e| {
        matches!(e, Event::Progress(1, PlaybackStatus::Paused, _))
    });
    let Event::Progress(_, _, paused) = event else {
        unreachable!()
    };
    thread::sleep(Duration::from_millis(250));
    player.wait(
        "paused position stays fixed",
        |e| matches!(e,Event::Progress(1,PlaybackStatus::Paused,p) if (*p-paused).abs()<0.03),
    );
    player.pause(false);
    player.wait("natural completion", |e| matches!(e, Event::Finished(1)));
    for id in 2..12 {
        player.load(id, &path, false);
    }
    player.wait(
        "rapid skipping",
        |e| matches!(e,Event::Progress(11,PlaybackStatus::Playing,p) if *p>0.2),
    );
    player.stop(9);
    player.wait("music/video ownership release", |e| {
        matches!(e, Event::Stopped(9))
    });
    player.load(11, &path, false);
    player.wait(
        "same-ID replay after stop",
        |e| matches!(e,Event::Progress(11,PlaybackStatus::Playing,p) if *p>0.2),
    );
    player.stop(10);
    player.wait("final stop", |e| matches!(e, Event::Stopped(10)));
}
fn local_library(player: Player) {
    let manifest = std::env::var_os("NANO_PLAYBACK_PATHS")
        .expect("set NANO_PLAYBACK_PATHS to a read-only JSON list of audio paths");
    let paths: Vec<PathBuf> = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    assert!(!paths.is_empty());
    for (id, path) in paths.iter().enumerate() {
        player.load(id, path, false);
        player.wait(&format!("track {id} {}",path.display()),|e| matches!(e,Event::Progress(track,PlaybackStatus::Playing,p) if *track==id && *p>0.2));
        eprintln!(
            "native playback passed {id}: {}",
            path.file_name().unwrap().to_string_lossy()
        );
    }
    player.stop(100);
    player.wait("library stop", |e| matches!(e, Event::Stopped(100)));
}
#[test]
#[ignore = "requires real audio output"]
fn legacy_native_transport() {
    transport(Player::Legacy(legacy::Engine::default()));
}
#[test]
#[ignore = "requires real audio output"]
fn current_native_transport() {
    transport(Player::Current(current::Engine::default()));
}
#[test]
#[ignore = "requires real audio output and explicit read-only local path manifest"]
fn legacy_native_local_library() {
    local_library(Player::Legacy(legacy::Engine::default()));
}
#[test]
#[ignore = "requires real audio output and explicit read-only local path manifest"]
fn current_native_local_library() {
    local_library(Player::Current(current::Engine::default()));
}

#[test]
#[ignore = "requires native audio and explicit isolated copy of local library; child uses 256 file handles"]
fn native_controller_legacy_monitor_setting_does_not_block_playback() {
    use nano_music_player::{
        app::{
            action::Action,
            config::Settings,
            controller::{Controller, connect},
            navigation::{Filter, Screen},
        },
        ui::AppWindow,
    };
    use slint::ComponentHandle;
    use std::{cell::RefCell, rc::Rc};
    if std::env::var_os("NANO_CONTROLLER_FD_CHILD").is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_controller_legacy_monitor_setting_does_not_block_playback",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("NANO_CONTROLLER_FD_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "native controller child: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    #[cfg(target_os = "macos")]
    {
        #[repr(C)]
        struct Limit {
            current: u64,
            maximum: u64,
        }
        unsafe extern "C" {
            fn getrlimit(resource: i32, limit: *mut Limit) -> i32;
            fn setrlimit(resource: i32, limit: *const Limit) -> i32;
        }
        let mut limit = Limit {
            current: 0,
            maximum: 0,
        };
        unsafe {
            assert_eq!(getrlimit(8, &mut limit), 0);
            limit.current = 256.min(limit.maximum);
            assert_eq!(setrlimit(8, &limit), 0);
        }
    }
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let source = std::env::var_os("NANO_PLAYBACK_DB_COPY")
        .expect("set NANO_PLAYBACK_DB_COPY to an isolated SQLite snapshot");
    std::fs::copy(source, tmp.path().join("library.sqlite3")).unwrap();
    Settings {
        monitor_deleted_files: true,
        gapless_playback: false,
        replay_gain_mode: Default::default(),
        ..Default::default()
    }
    .save(&tmp.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    ui.show().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(tmp.path().into())));
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    let deadline = Instant::now() + Duration::from_secs(25);
    while controller.borrow().state.tracks.is_empty() && Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !controller.borrow().state.tracks.is_empty(),
        "cached library never published: {}",
        controller.borrow().state.notice
    );
    // Allow startup to process the legacy monitoring setting before opening the decoder.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        thread::sleep(Duration::from_millis(20));
    }
    {
        let mut c = controller.borrow_mut();
        c.state.volume = 0.05;
        c.state.navigation.open(Screen::Songs(Filter::All));
        c.state.navigation.select(1);
        c.execute(&ui, Action::Activate);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut played = false;
    while Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        let c = controller.borrow();
        assert_ne!(c.state.status, PlaybackStatus::Error, "{}", c.state.notice);
        assert!(
            !c.state.notice.contains("monitoring warning"),
            "{}",
            c.state.notice
        );
        if c.state.status == PlaybackStatus::Playing && c.state.position > 0.4 {
            played = true;
            break;
        }
        drop(c);
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        played,
        "native controller never produced advancing playback: {}",
        controller.borrow().state.notice
    );
    ui.hide().unwrap();
}

#[test]
#[ignore = "requires real audio output; compares Stop responsiveness after an EOF seek"]
fn native_eof_seek_comparison() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("short.wav");
    std::fs::write(&path, include_bytes!("fixtures/quiet.wav")).unwrap();
    let old = Player::Legacy(legacy::Engine::default());
    old.load(1, &path, false);
    old.wait("historical EOF", |e| matches!(e, Event::Finished(1)));
    old.seek(0.05);
    old.stop(1);
    old.wait("historical Stop after EOF seek", |e| {
        matches!(e, Event::Stopped(1))
    });
    let current = Player::Current(current::Engine::default());
    current.load(1, &path, false);
    current.wait("current EOF", |e| matches!(e, Event::Finished(1)));
    current.seek(0.05);
    current.stop(1);
    current.wait("current Stop after EOF seek", |e| {
        matches!(e, Event::Stopped(1))
    });
}

#[test]
#[ignore = "requires real audio output; ReplayGain tested after ordinary playback baseline"]
fn native_replaygain_formats() {
    use lofty::{
        config::WriteOptions,
        file::{AudioFile, TaggedFileExt},
        tag::{ItemKey, Tag, TagType},
    };
    use nano_music_player::audio::gain::{GainConfig, GainMode, ReplayGainInfo};
    let tmp = tempfile::tempdir().unwrap();
    for (id, (name, bytes, kind)) in [
        (
            "tone.wav",
            include_bytes!("fixtures/quiet.wav").as_slice(),
            TagType::Id3v2,
        ),
        (
            "tone.flac",
            include_bytes!("fixtures/quiet.flac").as_slice(),
            TagType::VorbisComments,
        ),
        (
            "tone.mp3",
            include_bytes!("fixtures/quiet.mp3").as_slice(),
            TagType::Id3v2,
        ),
        (
            "tone.m4a",
            include_bytes!("fixtures/quiet.m4a").as_slice(),
            TagType::Mp4Ilst,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let path = tmp.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let mut file = lofty::read_from_path(&path).unwrap();
        let mut tag = Tag::new(kind);
        tag.insert_text(ItemKey::ReplayGainTrackGain, "-6 dB".into());
        tag.insert_text(ItemKey::ReplayGainAlbumGain, "-3 dB".into());
        file.insert_tag(tag);
        file.save_to_path(&path, WriteOptions::default()).unwrap();
        let metadata = ReplayGainInfo::read(&path);
        assert_eq!(metadata.track_gain_db, Some(-6.0));
        assert_eq!(metadata.album_gain_db, Some(-3.0));
        for mode in [GainMode::Track, GainMode::Album] {
            let player = Player::Current(current::Engine::default());
            let Player::Current(engine) = &player else {
                unreachable!()
            };
            engine
                .send(current::AudioCommand::LoadWithGain {
                    id,
                    path: path.clone(),
                    volume: 0.05,
                    paused: false,
                    gain: GainConfig {
                        mode,
                        ..Default::default()
                    },
                })
                .unwrap();
            player.wait(name,|e| matches!(e,Event::Progress(track,PlaybackStatus::Playing,p) if *track==id && *p>0.05));
            player.wait(
                "normalized source finishes",
                |e| matches!(e,Event::Finished(track) if *track==id),
            );
        }
    }
}

#[test]
#[ignore = "requires real audio output; same-sink transitions after baseline and gain verification"]
fn native_gapless_mixed_format_transitions() {
    let tmp = tempfile::tempdir().unwrap();
    let paths: Vec<_> = [
        ("one.wav", include_bytes!("fixtures/quiet.wav").as_slice()),
        ("two.flac", include_bytes!("fixtures/quiet.flac").as_slice()),
        ("three.mp3", include_bytes!("fixtures/quiet.mp3").as_slice()),
        ("four.m4a", include_bytes!("fixtures/quiet.m4a").as_slice()),
    ]
    .into_iter()
    .map(|(name, bytes)| {
        let path = tmp.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    })
    .collect();
    let engine = current::Engine::default();
    engine
        .send(current::AudioCommand::Load {
            id: 0,
            path: paths[0].clone(),
            volume: 0.05,
            paused: true,
        })
        .unwrap();
    // Append while paused, then let actual CoreAudio callbacks consume each boundary.
    engine
        .send(current::AudioCommand::AppendNext {
            token: 1,
            id: 1,
            path: paths[1].clone(),
            gain: Default::default(),
        })
        .unwrap();
    engine.send(current::AudioCommand::Pause(false)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut transitions = Vec::new();
    let mut finished = false;
    while Instant::now() < deadline {
        while let Ok(event) = engine.events.try_recv() {
            match event {
                current::AudioEvent::Error { message, .. }
                | current::AudioEvent::PrefetchFailed { message, .. } => panic!("{message}"),
                current::AudioEvent::Transitioned { from, to, .. } => {
                    transitions.push((from, to));
                    if to < 3 {
                        engine
                            .send(current::AudioCommand::AppendNext {
                                token: (to + 1) as u64,
                                id: to + 1,
                                path: paths[to + 1].clone(),
                                gain: Default::default(),
                            })
                            .unwrap();
                    }
                }
                current::AudioEvent::Finished(id) => {
                    assert_eq!(id, 3, "source ended before successor");
                    finished = true;
                }
                _ => {}
            }
        }
        if finished {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(transitions, vec![(0, 1), (1, 2), (2, 3)]);
    assert!(finished, "native chain did not finish");
}
