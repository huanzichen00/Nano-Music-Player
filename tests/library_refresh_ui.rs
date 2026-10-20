use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        navigation::Screen,
        state::Target,
    },
    audio::PlaybackStatus,
    library::{database::Database, scanner},
    ui::AppWindow,
};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
    fs,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

fn copy_audio(path: &Path) {
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quiet.wav"),
        path,
    )
    .unwrap();
}

fn settle(ui: &AppWindow, controller: &RefCell<Controller>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        controller.borrow_mut().poll(ui);
        if !ui.get_library_busy() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "refresh timed out: {}",
            controller.borrow().state.notice
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn startup_discovers_new_files_and_explicit_refresh_uses_menu_and_settings_without_watchers() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("music");
    fs::create_dir(&root).unwrap();
    let first = root.join("first.wav");
    let second = root.join("second.wav");
    let third = root.join("third.wav");
    copy_audio(&first);
    let database = directory.path().join("library.sqlite3");
    scanner::scan_folders(&database, std::slice::from_ref(&root)).unwrap();
    Settings {
        library_roots: vec![root.clone()],
        ..Default::default()
    }
    .save(&directory.path().join("settings.json"))
    .unwrap();
    copy_audio(&second); // This file was never explicitly imported.
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().into())));
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    settle(&ui, &controller);
    assert_eq!(controller.borrow().state.tracks.len(), 2);
    copy_audio(&third);
    for _ in 0..5 {
        controller.borrow_mut().poll(&ui);
    }
    assert_eq!(
        controller.borrow().state.tracks.len(),
        2,
        "no automatic watcher runs between refreshes"
    );

    let active = controller
        .borrow()
        .state
        .tracks
        .iter()
        .position(|track| track.path == first)
        .unwrap();
    let row = {
        let mut c = controller.borrow_mut();
        c.state.current = Some(active);
        c.state.status = PlaybackStatus::Playing;
        c.state.position = 0.25;
        c.state.queue.set(vec![active], 0, false);
        c.state.navigation.open(Screen::Settings);
        c.execute(&ui, Action::FinishSearchEditing);
        c.state
            .rows()
            .iter()
            .position(|row| matches!(row.target, Target::Action(Action::RefreshLibrary)))
            .unwrap()
    };
    ui.invoke_item_clicked(row as i32);
    assert!(ui.get_library_busy());
    assert!(controller.borrow().state.library_refreshing);
    ui.invoke_command("refresh_library".into()); // Duplicate requests are ignored.
    ui.invoke_command("import_folder".into()); // No competing import or picker.
    assert!(!controller.borrow().state.importing);
    settle(&ui, &controller);
    {
        let c = controller.borrow();
        assert_eq!(c.state.tracks.len(), 3);
        assert_eq!(c.state.current, Some(active));
        assert_eq!(c.state.status, PlaybackStatus::Playing);
        assert_eq!(c.state.position, 0.25);
        assert_eq!(c.state.queue.order, [active]);
        assert!(!c.state.library_refreshing);
        assert_eq!(c.state.navigation.current().screen, Screen::Settings);
    }
    fs::remove_file(&second).unwrap();
    ui.invoke_command("refresh_library".into());
    settle(&ui, &controller);
    assert!(
        controller
            .borrow()
            .state
            .unavailable_tracks
            .contains(&second)
    );
    assert_eq!(Database::open(&database).unwrap().load().unwrap().len(), 2);

    let disconnected = directory.path().join("disconnected");
    fs::rename(&root, &disconnected).unwrap();
    ui.invoke_command("refresh_library".into());
    settle(&ui, &controller);
    assert_eq!(
        Database::open(&database).unwrap().load().unwrap().len(),
        2,
        "unavailable volume preserves cache"
    );
    assert_eq!(controller.borrow().state.current, Some(active));
    fs::rename(disconnected, root).unwrap();
}
