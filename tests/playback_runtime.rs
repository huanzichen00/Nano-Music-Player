//! Explicit native audio smoke test; run with `cargo test --test playback_runtime -- --ignored`.
use nano_music_player::{
    app::{
        action::Action,
        controller::{Controller, connect},
        navigation::{Filter, Screen},
    },
    audio::PlaybackStatus,
    library::{
        database::Database,
        scanner,
        statistics::{SmartPlaylist, StatisticsStore},
    },
    ui::AppWindow,
};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
    rc::Rc,
    thread,
    time::{Duration, Instant},
};

fn write_tone(path: &std::path::Path) {
    // Four seconds of the repository's original quiet fixture, preserving WAV PCM.
    let fixture = include_bytes!("fixtures/quiet.wav");
    let data = &fixture[44..];
    let len = data.len() * 16;
    let mut wav = fixture[..44].to_vec();
    wav[4..8].copy_from_slice(&((len + 36) as u32).to_le_bytes());
    wav[40..44].copy_from_slice(&(len as u32).to_le_bytes());
    for _ in 0..16 {
        wav.extend_from_slice(data);
    }
    std::fs::write(path, wav).unwrap();
}

#[test]
#[ignore = "requires native audio output; verifies T generation leaves playback running"]
fn native_t_translation_generation_and_failure_do_not_interrupt_audio() {
    use slint::platform::WindowEvent;
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    write_tone(&music.join("song.wav"));
    std::fs::write(music.join("song.txt"), "Original test lyric").unwrap();
    scanner::scan_folders(&directory.path().join("library.sqlite3"), &[music]).unwrap();
    // Exercise the real worker without using a production key or opening HTTP.
    std::fs::write(
        directory.path().join("deepseek.json"),
        "invalid test configuration",
    )
    .unwrap();
    nano_music_player::app::config::Settings {
        translation_visible: true,
        ..Default::default()
    }
    .save(&directory.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    connect(&ui, &controller);
    let deadline = Instant::now() + Duration::from_secs(3);
    while controller.borrow().state.tracks.is_empty() && Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        thread::sleep(Duration::from_millis(10));
    }
    {
        let mut c = controller.borrow_mut();
        assert_eq!(c.state.tracks.len(), 1);
        assert!(!c.state.tracks[0].lyrics.is_empty());
        c.state.navigation.open(Screen::Songs(Filter::All));
        c.state.navigation.select(1);
        c.execute(&ui, Action::Activate);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while controller.borrow().state.position < 0.1 && Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(controller.borrow().state.status, PlaybackStatus::Playing);
    let position = controller.borrow().state.position;
    ui.invoke_focus_screen();
    for event in [
        WindowEvent::KeyPressed { text: "t".into() },
        WindowEvent::KeyReleased { text: "t".into() },
    ] {
        ui.window().dispatch_event(event);
    }
    assert!(ui.get_translation_visible());
    assert!(ui.get_notice().contains("Generating translation"));
    let deadline = Instant::now() + Duration::from_secs(1);
    while ui.get_notice().contains("Generating translation") && Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ui.get_notice().contains("Translation failed"));
    assert!(ui.get_translation_visible());
    for event in [
        WindowEvent::KeyPressed { text: "t".into() },
        WindowEvent::KeyReleased { text: "t".into() },
    ] {
        ui.window().dispatch_event(event);
    }
    assert!(!ui.get_translation_visible());
    let deadline = Instant::now() + Duration::from_secs(2);
    while controller.borrow().state.position < position + 0.2 && Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        assert_ne!(controller.borrow().state.status, PlaybackStatus::Error);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(controller.borrow().state.position >= position + 0.2);
    assert_eq!(controller.borrow().state.status, PlaybackStatus::Playing);
    assert_eq!(controller.borrow().state.current, Some(0));
    assert!(!ui.get_translation_visible());
}

#[test]
#[ignore = "requires a working native audio output; explicitly run for release smoke verification"]
fn native_playback_gapless_statistics_and_artwork_do_not_crash() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    std::fs::create_dir(&music).unwrap();
    for name in ["one.wav", "two.wav"] {
        write_tone(&music.join(name));
    }
    let db_path = tmp.path().join("library.sqlite3");
    let scanned = scanner::scan_folders(&db_path, std::slice::from_ref(&music)).unwrap();
    assert_eq!(scanned.tracks.len(), 2);
    assert!(
        scanned
            .tracks
            .iter()
            .all(|t| t.duration > 3.9 && t.duration < 4.1)
    );
    nano_music_player::app::config::Settings {
        library_roots: vec![music],
        ..Default::default()
    }
    .save(&tmp.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    ui.show().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(tmp.path().to_path_buf())));
    connect(&ui, &controller);
    let deadline = Instant::now() + Duration::from_secs(3);
    while controller.borrow().state.tracks.len() < 2 && Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        thread::sleep(Duration::from_millis(20));
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(16, 16)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    {
        let mut c = controller.borrow_mut();
        for track in &mut c.state.tracks {
            track.artwork = png.get_ref().clone();
        }
        c.state.navigation.open(Screen::Songs(Filter::All));
        c.state.navigation.select(1); // The first row is list Shuffle.
        c.execute(&ui, Action::Activate);
    }
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut saw_first = false;
    let mut saw_second = false;
    let mut statistics_opened = false;
    let mut statistics_closed = false;
    let mut statistics_reopened = false;
    while Instant::now() < deadline {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
        slint::platform::update_timers_and_animations();
        controller.borrow_mut().poll(&ui);
        {
            let mut c = controller.borrow_mut();
            if c.state.status == PlaybackStatus::Playing && c.state.current == Some(0) {
                let queue = c.state.queue.order.clone();
                let position = c.state.position;
                if position >= 0.5 && !statistics_opened {
                    c.state.navigation.open(Screen::Statistics(
                        nano_music_player::app::navigation::StatisticsPage::Overview,
                    ));
                    c.poll(&ui);
                    statistics_opened = true;
                } else if position >= 1.0 && statistics_opened && !statistics_closed {
                    c.execute(&ui, Action::Back);
                    statistics_closed = true;
                } else if position >= 2.5 && statistics_closed && !statistics_reopened {
                    c.state.navigation.open(Screen::Statistics(
                        nano_music_player::app::navigation::StatisticsPage::Overview,
                    ));
                    c.poll(&ui);
                    statistics_reopened = true;
                }
                assert_eq!(c.state.queue.order, queue);
                // poll may consume genuine progress or a gapless handoff;
                // deterministic UI tests separately assert navigation never seeks.
            }
        }
        let c = controller.borrow();
        assert_ne!(c.state.status, PlaybackStatus::Error, "{}", c.state.notice);
        if c.state.status == PlaybackStatus::Playing {
            saw_first |= c.state.current == Some(0);
            saw_second |= c.state.current == Some(1);
            if c.state.current == Some(1) {
                assert_eq!(c.state.queue.position, 1);
            }
        }
        if saw_second && c.state.status == PlaybackStatus::Stopped {
            break;
        }
        drop(c);
        thread::sleep(Duration::from_millis(20));
    }
    assert!(saw_first && saw_second, "both consecutive tracks must play");
    assert!(statistics_opened && statistics_closed && statistics_reopened);
    assert_eq!(
        controller.borrow().state.status,
        PlaybackStatus::Stopped,
        "queue must finish naturally"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        controller.borrow_mut().poll(&ui);
        let ready = {
            let c = controller.borrow();
            !c.state.statistics.loading
                && c.state
                    .statistics
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.total_plays == 2)
        };
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "statistics did not refresh to two qualified plays across the gapless transition"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let store = StatisticsStore::open(&db_path).unwrap();
    let tracks = Database::open(&db_path).unwrap().load().unwrap();
    for track in &tracks {
        assert_eq!(
            store
                .track_statistics(&track.path)
                .unwrap()
                .unwrap()
                .play_count,
            1
        );
    }
    assert_eq!(store.listening_history(10).unwrap().len(), 2);
    assert_eq!(
        store
            .smart_playlist(
                SmartPlaylist::NeverPlayed,
                &tracks.iter().map(|t| t.path.clone()).collect::<Vec<_>>()
            )
            .unwrap()
            .len(),
        0
    );
    controller.borrow_mut().execute(&ui, Action::NowPlaying);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(300));
    slint::platform::update_timers_and_animations();
    controller.borrow_mut().poll(&ui);
    assert_eq!(ui.get_screen(), 2);
    assert_eq!(ui.get_track_title(), "two");
    let shot = ui.window().take_snapshot().unwrap();
    let out = std::env::temp_dir().join("nano-qa/native-playback-finished.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image::save_buffer(
        out,
        shot.as_bytes(),
        shot.width(),
        shot.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
    {
        use nano_music_player::app::navigation::Group;
        let mut c = controller.borrow_mut();
        c.state.navigation.open(Screen::Groups(Group::Playlists));
        c.execute(&ui, Action::FinishSearchEditing);
        assert_eq!(c.state.rows().len(), 8);
        assert_eq!(c.state.smart_playlists[&SmartPlaylist::MostPlayed].len(), 2);
        assert!(c.state.smart_playlists[&SmartPlaylist::NeverPlayed].is_empty());
    }
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(300));
    slint::platform::update_timers_and_animations();
    controller.borrow_mut().poll(&ui);
    ui.invoke_cancel_screen_transition();
    assert_eq!(ui.get_screen_title(), "Playlists");
    let shot = ui.window().take_snapshot().unwrap();
    image::save_buffer(
        std::env::temp_dir().join("nano-qa/smart-playlists.png"),
        shot.as_bytes(),
        shot.width(),
        shot.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
    ui.hide().unwrap();
}

#[test]
#[ignore = "requires a working native audio output; verifies seek/skip/pause/stop controls"]
fn native_rapid_seeks_skips_pause_resume_and_stop_remain_responsive() {
    use nano_music_player::audio::engine::{AudioCommand, AudioEvent, Engine};

    fn wait(engine: &Engine, predicate: impl Fn(&AudioEvent) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            while let Ok(event) = engine.events.try_recv() {
                assert!(!matches!(event, AudioEvent::Error { .. }), "{event:?}");
                if predicate(&event) {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("native audio control did not complete within three seconds");
    }

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("transport.wav");
    write_tone(&path);
    let engine = Engine::default();
    engine
        .send(AudioCommand::Load {
            id: 10,
            path: path.clone(),
            volume: 0.2,
            paused: true,
        })
        .unwrap();
    wait(&engine, |event| {
        matches!(
            event,
            AudioEvent::Progress {
                id: 10,
                status: PlaybackStatus::Paused,
                ..
            }
        )
    });
    engine.send(AudioCommand::Seek(0.8)).unwrap();
    engine.send(AudioCommand::Seek(2.2)).unwrap();
    wait(
        &engine,
        |event| matches!(event, AudioEvent::Progress { id: 10, status: PlaybackStatus::Paused, position, .. } if (*position - 2.2).abs() < 0.03),
    );
    engine.send(AudioCommand::Pause(false)).unwrap();
    wait(
        &engine,
        |event| matches!(event, AudioEvent::Progress { id: 10, status: PlaybackStatus::Playing, position, .. } if *position > 2.3),
    );
    engine.send(AudioCommand::Pause(true)).unwrap();
    wait(&engine, |event| {
        matches!(
            event,
            AudioEvent::Progress {
                id: 10,
                status: PlaybackStatus::Paused,
                ..
            }
        )
    });
    engine
        .send(AudioCommand::AppendNext {
            token: 8,
            id: 13,
            path: path.clone(),
            gain: Default::default(),
        })
        .unwrap();
    engine.send(AudioCommand::Seek(3.9)).unwrap();
    engine.send(AudioCommand::Pause(false)).unwrap();
    wait(&engine, |event| {
        assert!(
            !matches!(event, AudioEvent::Transitioned { to: 13, .. }),
            "seek must cancel unstarted successor"
        );
        matches!(event, AudioEvent::Finished(10))
    });
    engine
        .send(AudioCommand::Load {
            id: 11,
            path: path.clone(),
            volume: 0.2,
            paused: false,
        })
        .unwrap();
    engine
        .send(AudioCommand::Load {
            id: 12,
            path,
            volume: 0.2,
            paused: false,
        })
        .unwrap();
    wait(&engine, |event| {
        assert!(
            !matches!(event, AudioEvent::Progress { id: 11, .. }),
            "superseded load published progress"
        );
        matches!(
            event,
            AudioEvent::Progress {
                id: 12,
                status: PlaybackStatus::Playing,
                ..
            }
        )
    });
    engine.send(AudioCommand::Stop { handoff: 456 }).unwrap();
    wait(&engine, |event| {
        matches!(event, AudioEvent::Stopped { handoff: 456 })
    });
    assert!(engine.events.try_recv().is_err());
}

#[test]
#[ignore = "requires a working native audio output; verifies the global Shuffle All shortcut"]
fn native_shuffle_all_shortcut_starts_audio_from_an_empty_search() {
    use slint::platform::{Key, WindowEvent};
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    std::fs::create_dir(&music).unwrap();
    for name in ["first.wav", "second.wav"] {
        write_tone(&music.join(name));
    }
    scanner::scan_folders(&tmp.path().join("library.sqlite3"), &[music]).unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(tmp.path().to_path_buf())));
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while controller.borrow().state.tracks.len() < 2 && Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(controller.borrow().state.tracks.len(), 2);
    {
        let mut c = controller.borrow_mut();
        c.state.navigation.open(Screen::Search);
        c.state.query = "no matching songs".into();
        c.execute(&ui, Action::FinishSearchEditing);
    }
    assert!(controller.borrow().state.rows().is_empty());
    ui.invoke_focus_screen();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Shift.into(),
    });
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: "S".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: "S".into() });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Shift.into(),
    });
    assert!(controller.borrow().state.shuffle);
    let wait_for_audio = |expected: Option<usize>| {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            controller.borrow_mut().poll(&ui);
            let c = controller.borrow();
            assert_ne!(c.state.status, PlaybackStatus::Error, "{}", c.state.notice);
            if c.state.status == PlaybackStatus::Playing
                && c.state.position > 0.2
                && expected.is_none_or(|id| c.state.current == Some(id))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Shuffle All did not advance native audio"
            );
            drop(c);
            thread::sleep(Duration::from_millis(10));
        }
    };
    wait_for_audio(None);
    let next = {
        let c = controller.borrow();
        let mut ids = c.state.queue.order.clone();
        ids.sort_unstable();
        assert_eq!(ids, [0, 1]);
        c.state.queue.order[1]
    };
    controller.borrow_mut().execute(&ui, Action::NextTrack);
    wait_for_audio(Some(next));
    controller.borrow_mut().execute(&ui, Action::TogglePlay);
    assert!(controller.borrow().state.paused);
    ui.hide().unwrap();
}
