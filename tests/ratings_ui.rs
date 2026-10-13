use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        navigation::{Filter, Screen},
    },
    audio::PlaybackStatus,
    input::{Binding, Key, Keymap, Modifiers},
    library::{Track, statistics::StatisticsStore},
    ui::AppWindow,
};
use slint::{ComponentHandle, platform::WindowEvent};
use std::{
    cell::RefCell,
    rc::Rc,
    thread,
    time::{Duration, Instant},
};

fn point(ui: &AppWindow, x: f32, y: f32) -> slint::LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let content = ui.get_content_height();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    let top = size.height - content + (content - body * scale) / 2.0;
    slint::LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        top + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    )
}
fn pointer(ui: &AppWindow, pressed: bool, x: f32, y: f32) {
    let position = point(ui, x, y);
    let button = slint::platform::PointerEventButton::Left;
    ui.window().dispatch_event(if pressed {
        WindowEvent::PointerPressed { position, button }
    } else {
        WindowEvent::PointerReleased { position, button }
    });
}
fn tap(ui: &AppWindow, key: &str) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
}
fn wait(c: &RefCell<Controller>, ui: &AppWindow, condition: impl Fn(&Controller) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        c.borrow_mut().poll(ui);
        if condition(&c.borrow()) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(condition(&c.borrow()), "rating worker timed out");
}
fn fixture(path: std::path::PathBuf, title: &str, number: u32) -> Track {
    Track {
        path,
        title: title.into(),
        artist: "Artist".into(),
        album: "Album".into(),
        number,
        genre: String::new(),
        duration: 180.0,
        artwork: Vec::new(),
        lyrics: Default::default(),
    }
}
fn snapshot(ui: &AppWindow, name: &str) {
    let shot = ui.window().take_snapshot().unwrap();
    let dir = std::env::temp_dir().join("nano-qa");
    std::fs::create_dir_all(&dir).unwrap();
    image::save_buffer(
        dir.join(name),
        shot.as_bytes(),
        shot.width(),
        shot.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

#[test]
fn pdf_rating_strip_tap_drag_clear_persistence_and_keyboard_guards() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    Settings {
        monitor_deleted_files: false,
        smooth_slides: false,
        ..Default::default()
    }
    .save(&tmp.path().join("settings.json"))
    .unwrap();
    let db = tmp.path().join("library.sqlite3");
    let path = tmp.path().join("first.wav");
    let other = tmp.path().join("second.wav");
    let tracks = vec![
        fixture(path.clone(), "First song", 1),
        fixture(other.clone(), "Second song", 2),
    ];
    let ui = AppWindow::new().unwrap();
    let c = Rc::new(RefCell::new(Controller::new(tmp.path().to_path_buf())));
    c.borrow_mut().state.merge_library(tracks.clone());
    connect(&ui, &c);
    c.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    wait(&c, &ui, |c| c.state.ratings_ready);
    {
        let mut c = c.borrow_mut();
        c.state.start(0); // Discard audio effect; exercise the real UI without output hardware.
        c.state.status = PlaybackStatus::Playing;
        c.state.queue.set(vec![0, 1], 0, false);
        c.execute(&ui, Action::NowPlaying);
    }
    assert!(!ui.get_rating_visible());
    ui.invoke_item_clicked(6); // The existing album track-list control from PDF p.23.
    assert!(ui.get_rating_visible());
    assert_eq!(ui.get_rating_title(), "First song");
    assert_eq!(ui.get_rating(), 0);
    assert_eq!(ui.get_list_header_height(), 80.0 * ui.get_display_scale());
    snapshot(&ui, "rating-before.png");
    pointer(&ui, true, 150.0, 96.0);
    pointer(&ui, false, 150.0, 96.0);
    assert_eq!(ui.get_rating(), 4);
    assert_eq!(c.borrow().state.queue.order, vec![0, 1]);
    assert_eq!(
        c.borrow().state.position,
        0.0,
        "rating does not seek or restart playback"
    );
    snapshot(&ui, "rating-four-stars.png");
    pointer(&ui, true, 60.0, 96.0);
    for x in [90.0, 120.0, 150.0, 180.0] {
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: point(&ui, x, 96.0),
        });
    }
    pointer(&ui, false, 180.0, 96.0);
    assert_eq!(ui.get_rating(), 5);
    assert!(
        matches!(
            c.borrow().state.navigation.current().screen,
            Screen::Songs(Filter::Album { .. })
        ),
        "rating drag must not trigger horizontal Back"
    );
    pointer(&ui, true, 30.0, 96.0);
    pointer(&ui, false, 30.0, 96.0);
    assert_eq!(ui.get_rating(), 0);
    snapshot(&ui, "rating-unrated.png");
    tap(&ui, "3");
    assert_eq!(ui.get_rating(), 3);
    // Simulate an automatic transition: the visible song's rating stays pinned.
    c.borrow_mut().state.current = Some(1);
    c.borrow_mut().state.queue.position = 1;
    c.borrow_mut().poll(&ui);
    tap(&ui, "4");
    assert_eq!(c.borrow().state.ratings[&path], 4);
    assert!(!c.borrow().state.ratings.contains_key(&other));
    ui.invoke_rating_changed(other.to_string_lossy().into_owned().into(), 5);
    ui.invoke_rating_changed(path.to_string_lossy().into_owned().into(), 6);
    assert_eq!(
        ui.get_rating(),
        4,
        "wrong target/out-of-range edits are rejected"
    );
    wait(&c, &ui, |_| {
        StatisticsStore::open(&db)
            .unwrap()
            .ratings()
            .unwrap()
            .get(&path)
            == Some(&4)
    });
    // Restart reloads the confirmed rating independently of the metadata cache.
    drop(c);
    assert_eq!(
        StatisticsStore::open(&db).unwrap().ratings().unwrap()[&path],
        4
    );
    let restarted = Rc::new(RefCell::new(Controller::new(tmp.path().to_path_buf())));
    restarted.borrow_mut().state.merge_library(tracks);
    connect(&ui, &restarted);
    restarted.borrow_mut().initialize(&ui);
    wait(&restarted, &ui, |c| c.state.ratings_ready);
    {
        let mut c = restarted.borrow_mut();
        c.state.start(0);
        c.execute(&ui, Action::NowPlaying);
        c.execute(&ui, Action::TrackList);
    }
    assert_eq!(ui.get_rating(), 4);
    // An intermediate successful save is the rollback value for a later failure.
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_rating_ui BEFORE INSERT ON track_ratings WHEN NEW.stars=5 BEGIN SELECT RAISE(ABORT, 'failure'); END;").unwrap();
    tap(&ui, "2");
    tap(&ui, "5");
    assert_eq!(ui.get_rating(), 5);
    wait(&restarted, &ui, |c| {
        c.state.notice == "Could not save rating"
    });
    assert_eq!(ui.get_rating(), 2);
    assert_eq!(
        StatisticsStore::open(&db).unwrap().ratings().unwrap()[&path],
        2
    );
    connection
        .execute_batch("DROP TRIGGER fail_rating_ui")
        .unwrap();
    // Modal and text-entry input cannot change ratings.
    ui.set_genius_preview_open(true);
    ui.invoke_rating_changed(path.to_string_lossy().into_owned().into(), 3);
    assert_eq!(ui.get_rating(), 2);
    ui.set_genius_preview_open(false);
    restarted.borrow_mut().execute(&ui, Action::Search);
    tap(&ui, "5");
    assert_eq!(ui.get_search_text(), "5");
    assert_eq!(restarted.borrow().state.ratings[&path], 2);
    assert!(!ui.get_rating_visible());
    // A customized binding takes precedence over numeric rating convenience keys.
    let mut keys = Keymap::default();
    keys.bindings.push(Binding {
        action: Action::ToggleShuffle,
        key: Key::Digit('5'),
        modifiers: Modifiers::default(),
        optional_shift: false,
    });
    keys.save(&tmp.path().join("shortcuts.json")).unwrap();
    let custom = Rc::new(RefCell::new(Controller::new(tmp.path().to_path_buf())));
    custom
        .borrow_mut()
        .state
        .merge_library(vec![fixture(path.clone(), "First song", 1)]);
    connect(&ui, &custom);
    custom.borrow_mut().initialize(&ui);
    wait(&custom, &ui, |c| c.state.ratings_ready);
    {
        let mut c = custom.borrow_mut();
        c.state.start(0);
        c.execute(&ui, Action::NowPlaying);
        c.execute(&ui, Action::TrackList);
    }
    ui.invoke_focus_screen();
    tap(&ui, "5");
    assert!(custom.borrow().state.shuffle);
    assert_eq!(custom.borrow().state.ratings[&path], 2);
    custom.borrow_mut().execute(&ui, Action::Back);
    assert!(!ui.get_rating_visible());
    // An unbound controller can close immediately after a queued write; Drop
    // must drain the worker rather than depend on another UI poll.
    let close_tmp = tempfile::tempdir().unwrap();
    let close_path = close_tmp.path().join("close.wav");
    {
        let mut close = Controller::new(close_tmp.path().to_path_buf());
        close
            .state
            .merge_library(vec![fixture(close_path.clone(), "Close test", 1)]);
        close.state.ratings_ready = true;
        close.state.start(0);
        close.execute(&ui, Action::NowPlaying);
        close.execute(&ui, Action::TrackList);
        close.set_rating(&ui, &close_path, 4);
    }
    assert_eq!(
        StatisticsStore::open(&close_tmp.path().join("library.sqlite3"))
            .unwrap()
            .ratings()
            .unwrap()[&close_path],
        4
    );
    ui.hide().unwrap();
}
