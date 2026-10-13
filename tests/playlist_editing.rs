use nano_music_player::{
    app::{
        action::Action,
        controller::{Controller, connect},
        i18n::Language,
        navigation::{Filter, Screen},
    },
    audio::PlaybackStatus,
    library::{Track, lyrics::Lyrics, playlists},
    ui::AppWindow,
};
use slint::{
    ComponentHandle, Model,
    platform::{Key, PointerEventButton, WindowEvent},
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

fn tap(ui: &AppWindow, key: impl Into<slint::SharedString>) {
    let text = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn click(ui: &AppWindow, x: f32, y: f32) {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let height = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    let top =
        size.height - ui.get_content_height() + (ui.get_content_height() - height * scale) / 2.0;
    let position = slint::LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        top + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    );
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn drag(ui: &AppWindow, start: (f32, f32), end: (f32, f32)) {
    drag_with_hold(ui, start, end, 0);
}

fn drag_with_hold(ui: &AppWindow, start: (f32, f32), end: (f32, f32), edge_ticks: usize) {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let height = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    let top =
        size.height - ui.get_content_height() + (ui.get_content_height() - height * scale) / 2.0;
    let point = |x: f32, y: f32| {
        slint::LogicalPosition::new(
            (size.width - 240.0 * scale) / 2.0 + x * scale,
            top + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
        )
    };
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: point(start.0, start.1),
        button: PointerEventButton::Left,
    });
    for step in 1..=10 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
        let fraction = step as f32 / 10.0;
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: point(
                start.0 + (end.0 - start.0) * fraction,
                start.1 + (end.1 - start.1) * fraction,
            ),
        });
    }
    for _ in 0..edge_ticks {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(50));
    }
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: point(end.0, end.1),
        button: PointerEventButton::Left,
    });
}

fn settle(ui: &AppWindow, controller: &Rc<RefCell<Controller>>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        controller.borrow_mut().poll(ui);
        if !ui.get_playlist_saving() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn snapshot(ui: &AppWindow, name: &str) {
    let image = ui.window().take_snapshot().unwrap();
    let directory = std::env::temp_dir().join("nano-qa");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        image.as_bytes(),
        image.width(),
        image.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

#[test]
fn saved_playlist_editing_is_atomic_cancellable_and_preserves_playback() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("library.sqlite3");
    let paths: Vec<_> = ["first.wav", "second.wav", "added.wav"]
        .map(|name| directory.path().join(name))
        .into();
    for path in &paths {
        std::fs::write(path, b"offline fixture").unwrap();
    }
    let missing = directory.path().join("missing.wav");
    let original = playlists::create(
        &database,
        "Evening Mix",
        &[paths[1].clone(), missing.clone(), paths[0].clone()],
    )
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    {
        let mut c = controller.borrow_mut();
        c.state.tracks = paths
            .iter()
            .enumerate()
            .map(|(index, path)| Track {
                path: path.clone(),
                title: ["First Song", "Second Song", "Added Song"][index].into(),
                artist: "Reference Artist".into(),
                album: "Album".into(),
                genre: "Test".into(),
                number: index as u32 + 1,
                duration: 60.0,
                artwork: vec![],
                lyrics: Lyrics::default(),
            })
            .collect();
        c.state.playlists = vec![original.clone()];
        c.state.current = Some(0);
        c.state.queue.set(vec![0, 1], 0, false);
        c.state.status = PlaybackStatus::Paused;
        c.state.paused = true;
        c.state.position = 23.0;
        c.state.smooth_slides = false;
        c.state.navigation.open(Screen::Songs(Filter::Playlist {
            id: original.id,
            name: original.name.clone(),
        }));
    }
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    ui.invoke_focus_screen();
    assert!(ui.get_playlist_content());
    click(&ui, 120.0, 42.0); // Compact Edit toolbar inside the saved playlist.
    assert!(
        ui.get_playlist_editing() && ui.get_playlist_editor_open() && ui.get_playlist_reviewing()
    );
    assert_eq!(ui.get_playlist_name(), "Evening Mix");
    assert_eq!(ui.get_playlist_draft().row_count(), 3);
    assert_eq!(
        ui.get_playlist_draft().row_data(1).unwrap().artist,
        "Unavailable song"
    );
    snapshot(&ui, "playlist-edit-review");
    drag(&ui, (226.0, 88.0), (226.0, 184.0));
    assert_eq!(
        ui.get_playlist_draft().row_data(2).unwrap().id,
        paths[1].to_string_lossy()
    );
    assert_eq!(
        ui.get_playlist_draft().row_data(0).unwrap().id,
        missing.to_string_lossy()
    );
    assert_eq!(
        playlists::load(&database).unwrap()[0],
        original,
        "drag changes only the draft"
    );
    drag(&ui, (226.0, 184.0), (226.0, 88.0));
    assert_eq!(
        ui.get_playlist_draft().row_data(0).unwrap().id,
        paths[1].to_string_lossy()
    );
    drag(&ui, (226.0, 88.0), (226.0, 91.0));
    assert_eq!(
        ui.get_playlist_draft().row_data(0).unwrap().id,
        paths[1].to_string_lossy()
    );
    click(&ui, 16.0, 88.0); // Minus asks for Delete, without modifying the draft yet.
    assert_eq!(ui.get_playlist_draft().row_count(), 3);
    snapshot(&ui, "playlist-edit-delete");
    click(&ui, 202.0, 88.0); // Delete confirms removal from the draft only.
    assert_eq!(ui.get_playlist_draft().row_count(), 2);
    assert_eq!(playlists::load(&database).unwrap()[0], original);
    click(&ui, 25.0, 16.0); // Cancel discards all unsaved changes.
    assert!(!ui.get_playlist_editor_open());
    assert_eq!(playlists::load(&database).unwrap()[0], original);
    // Edit, Shuffle, then songs form a single keyboard selection sequence.
    assert_eq!(ui.get_selected(), 0);
    tap(&ui, "j");
    assert_eq!(ui.get_selected(), 1);
    tap(&ui, "j");
    assert_eq!(ui.get_selected(), 2);
    tap(&ui, "k");
    tap(&ui, "k");
    assert_eq!(ui.get_selected(), 0);
    tap(&ui, "l");
    assert!(ui.get_playlist_editor_open() && ui.get_playlist_reviewing());
    click(&ui, 175.0, 48.0); // Add opens the existing name and available-song picker.
    assert!(!ui.get_playlist_reviewing());
    ui.set_playlist_name("夜の Mix".into());
    ui.invoke_playlist_action("toggle".into(), paths[2].to_string_lossy().as_ref().into());
    tap(&ui, Key::Return);
    assert!(ui.get_playlist_reviewing());
    tap(&ui, "j");
    tap(&ui, "k"); // Keyboard selection remains usable in ordered review.
    ui.invoke_playlist_action("up".into(), paths[2].to_string_lossy().as_ref().into());
    let expected = vec![
        paths[1].clone(),
        missing.clone(),
        paths[2].clone(),
        paths[0].clone(),
    ];
    for language in [
        Language::SimplifiedChinese,
        Language::Japanese,
        Language::English,
    ] {
        controller.borrow_mut().state.language = language;
        controller.borrow_mut().poll(&ui);
        assert_eq!(ui.get_playlist_name(), "夜の Mix");
        snapshot(&ui, &format!("playlist-edit-{}", language.code()));
    }
    // Home preserves the editing identity; New Playlist resumes rather than creating a copy.
    controller.borrow_mut().execute(&ui, Action::Home);
    controller.borrow_mut().execute(&ui, Action::NewPlaylist);
    assert!(ui.get_playlist_reviewing() && ui.get_playlist_editing());
    click(&ui, 55.0, 48.0); // Done starts a stable-ID update.
    assert!(ui.get_playlist_saving());
    controller.borrow_mut().execute(&ui, Action::Home);
    settle(&ui, &controller);
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::Home
    );
    let saved = playlists::load(&database).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, original.id);
    assert_eq!(saved[0].name, "夜の Mix");
    assert_eq!(saved[0].tracks, expected);
    assert_eq!(controller.borrow().state.queue.order, vec![0, 1]);
    assert_eq!(controller.borrow().state.position, 23.0);
    assert!(controller.borrow().state.paused);

    controller
        .borrow_mut()
        .state
        .navigation
        .open(Screen::Songs(Filter::Playlist {
            id: original.id,
            name: saved[0].name.clone(),
        }));
    controller.borrow_mut().poll(&ui);
    controller.borrow_mut().execute(&ui, Action::EditPlaylist);
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_edit_ui BEFORE INSERT ON playlist_tracks BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
    ui.set_playlist_name("Failed name".into());
    ui.invoke_playlist_action("create".into(), "".into());
    settle(&ui, &controller);
    assert!(ui.get_playlist_editor_open());
    assert_eq!(
        ui.get_playlist_message(),
        "Could not save playlist. Try again."
    );
    assert_eq!(playlists::load(&database).unwrap(), saved);
    connection
        .execute_batch("DROP TRIGGER fail_edit_ui")
        .unwrap();
    ui.invoke_playlist_action("create".into(), "".into());
    settle(&ui, &controller);
    assert_eq!(ui.get_screen_title(), "Failed name");
    assert_eq!(controller.borrow().state.playlists.len(), 1);
    // Empty playlists still expose Edit and can receive songs later.
    controller.borrow_mut().execute(&ui, Action::EditPlaylist);
    for path in &expected {
        ui.invoke_playlist_action("remove".into(), path.to_string_lossy().as_ref().into());
    }
    click(&ui, 55.0, 48.0);
    settle(&ui, &controller);
    assert_eq!(ui.get_rows().row_count(), 2);
    assert!(ui.get_playlist_content());
    click(&ui, 120.0, 42.0);
    assert!(ui.get_playlist_editor_open());
    assert_eq!(ui.get_playlist_draft().row_count(), 0);
    click(&ui, 25.0, 16.0);
    // Holding a grip near the viewport edge reaches rows below the initial screen.
    let long_paths: Vec<_> = (0..12)
        .map(|index| directory.path().join(format!("long-{index:02}.wav")))
        .collect();
    let long = playlists::create(&database, "Long draft", &long_paths).unwrap();
    controller.borrow_mut().state.playlists.push(long.clone());
    controller
        .borrow_mut()
        .state
        .navigation
        .open(Screen::Songs(Filter::Playlist {
            id: long.id,
            name: long.name.clone(),
        }));
    controller.borrow_mut().poll(&ui);
    tap(&ui, "l");
    assert_eq!(ui.get_playlist_draft().row_count(), 12);
    drag_with_hold(&ui, (226.0, 88.0), (226.0, 361.0), 30);
    let target = ui
        .get_playlist_draft()
        .iter()
        .position(|song| song.id.as_str() == long_paths[0].to_string_lossy())
        .unwrap();
    assert!(
        target >= 9,
        "edge auto-scroll reaches rows beyond the initial viewport: {target}"
    );
    assert_eq!(
        playlists::load(&database)
            .unwrap()
            .iter()
            .find(|p| p.id == long.id)
            .unwrap()
            .tracks,
        long_paths
    );
    click(&ui, 25.0, 16.0);
    let mut restarted = Controller::new(directory.path().to_owned());
    let deadline = Instant::now() + Duration::from_secs(5);
    while restarted.state.playlists.is_empty() {
        restarted.poll(&ui);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(restarted.state.playlists[0].id, original.id);
    assert!(
        restarted
            .state
            .playlists
            .iter()
            .find(|p| p.id == original.id)
            .unwrap()
            .tracks
            .is_empty()
    );
}
