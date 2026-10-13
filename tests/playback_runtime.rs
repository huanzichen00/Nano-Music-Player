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
        monitor_roots: vec![music],
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
    while Instant::now() < deadline {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
        slint::platform::update_timers_and_animations();
        controller.borrow_mut().poll(&ui);
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
    assert_eq!(
        controller.borrow().state.status,
        PlaybackStatus::Stopped,
        "queue must finish naturally"
    );
    for _ in 0..10 {
        controller.borrow_mut().poll(&ui);
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
        assert_eq!(c.state.rows().len(), 6);
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
