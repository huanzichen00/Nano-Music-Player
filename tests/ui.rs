use nano_music_player::{
    app::{
        action::Action,
        controller::{Controller, connect},
    },
    library::{Track, lyrics::Lyrics},
    ui::AppWindow,
};
use slint::{
    ComponentHandle, Model,
    platform::{Key, WindowEvent},
};
use std::{cell::RefCell, rc::Rc};

fn tap(ui: &AppWindow, key: impl Into<slint::SharedString>) {
    let key = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key });
}

fn primary(ui: &AppWindow, key: impl Into<slint::SharedString>) {
    // Platform events are already normalized by the Slint native backend.
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    tap(ui, key);
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
}

fn display_point(ui: &AppWindow, x: f32, y: f32) -> slint::LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let content_height = ui.get_content_height();
    let body_height = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    let top = size.height - content_height + (content_height - body_height * scale) / 2.0;
    slint::LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        top + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    )
}

fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = display_point(ui, x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
}

fn scroll(ui: &AppWindow, x: f32, y: f32, delta_x: f32, delta_y: f32) {
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: display_point(ui, x, y),
        delta_x,
        delta_y,
    });
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
}

fn drag(ui: &AppWindow, start: (f32, f32), end: (f32, f32)) {
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: display_point(ui, start.0, start.1),
        button: slint::platform::PointerEventButton::Left,
    });
    // Gesture recognizers defer forwarding presses briefly to distinguish taps
    // from swipes. Model a real drag over several frames, not an instant jump.
    for step in 1..=10 {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(20));
        let fraction = step as f32 / 10.0;
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: display_point(
                ui,
                start.0 + (end.0 - start.0) * fraction,
                start.1 + (end.1 - start.1) * fraction,
            ),
        });
    }
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: display_point(ui, end.0, end.1),
        button: slint::platform::PointerEventButton::Left,
    });
}

fn snapshot(ui: &AppWindow, name: &str) {
    let image = ui
        .window()
        .take_snapshot()
        .expect("headless software rendering");
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

fn wait_for_library_artwork(ui: &AppWindow, controller: &RefCell<Controller>, row: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        controller.borrow_mut().poll(ui);
        if ui
            .get_rows()
            .row_data(row)
            .is_some_and(|row| row.artwork.size().width > 0)
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "library artwork did not arrive"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn wait_for_playlist_save(ui: &AppWindow, controller: &RefCell<Controller>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while ui.get_playlist_saving() {
        controller.borrow_mut().poll(ui);
        assert!(
            std::time::Instant::now() < deadline,
            "playlist save timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn choose_language(
    ui: &AppWindow,
    controller: &RefCell<Controller>,
    language: nano_music_player::app::i18n::Language,
) {
    let mut controller = controller.borrow_mut();
    controller
        .state
        .navigation
        .open(nano_music_player::app::navigation::Screen::Languages);
    controller.state.navigation.select(language.index());
    controller.execute(ui, Action::Activate);
    controller.execute(ui, Action::Back);
}

#[test]
fn actual_slint_key_capture_search_focus_scroll_and_display_modes() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    // These synthetic tracks deliberately have no source files. File deletion
    // reconciliation is covered separately with real filesystem fixtures.
    nano_music_player::app::config::Settings {
        monitor_deleted_files: false,
        ..Default::default()
    }
    .save(&directory.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(
        directory.path().to_path_buf(),
    )));
    controller.borrow_mut().state.merge_library(
        (0..20)
            .map(|id| Track {
                path: directory.path().join(format!("{id}.wav")),
                title: format!("Test Song {id:02}"),
                artist: "Test Artist".to_owned(),
                album: "Test Album".to_owned(),
                genre: "Test".to_owned(),
                number: id,
                duration: 60.0,
                artwork: Vec::new(),
                lyrics: Lyrics::parse(
                    "[00:00.00]First line\n[00:10.250]Second line\n[00:20.5]Third line",
                ),
            })
            .collect(),
    );
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    ui.invoke_focus_screen();
    controller.borrow_mut().poll(&ui);
    snapshot(&ui, "home");
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: display_point(&ui, 60.0, 80.0),
        delta_x: -80.0,
        delta_y: 0.0,
    });
    assert_eq!(
        ui.get_home_page(),
        1,
        "horizontal scroll over an icon changes Home page"
    );
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(60));
    assert!(
        ui.get_home_transition_offset() > 0.0,
        "leftward swipe enters the next page from the right"
    );
    assert!(ui.get_home_outgoing_transition_offset() < 0.0);
    // The outgoing Videos icon must not activate Settings on the new Home page.
    let outgoing_icon_x = 170.0 + ui.get_home_outgoing_transition_offset() / ui.get_display_scale();
    click(&ui, outgoing_icon_x, 80.0);
    assert_eq!(
        ui.get_screen(),
        0,
        "departing Home icons must not activate a different screen"
    );
    assert_eq!(ui.get_home_page(), 1);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: display_point(&ui, 60.0, 80.0),
        delta_x: 80.0,
        delta_y: 0.0,
    });
    assert_eq!(ui.get_home_page(), 0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(80));
    assert!(
        ui.get_home_transition_offset() < 0.0,
        "rightward Home swipe brings the prior page in from the left"
    );
    assert!(ui.get_home_outgoing_transition_offset() > 0.0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    controller.borrow_mut().state.navigation.select(5);
    controller.borrow_mut().poll(&ui);
    tap(&ui, Key::RightArrow);
    assert_eq!(ui.get_home_page(), 1);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(60));
    assert!(ui.get_home_transition_offset() > 0.0);
    assert!(ui.get_home_outgoing_transition_offset() < 0.0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    assert_eq!(ui.get_previous_rows().row_count(), 0);
    tap(&ui, Key::LeftArrow);
    assert_eq!(ui.get_home_page(), 0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(60));
    assert!(ui.get_home_transition_offset() < 0.0);
    assert!(ui.get_home_outgoing_transition_offset() > 0.0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    controller.borrow_mut().execute(&ui, Action::Settings);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(10));
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(60));
    assert!(
        ui.get_transition_offset() > 0.0,
        "forward navigation enters from the right"
    );
    assert!(ui.get_previous_transition_offset() < 0.0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    controller.borrow_mut().execute(&ui, Action::Back);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(60));
    assert!(
        ui.get_transition_offset() < 0.0,
        "Back navigation enters from the left"
    );
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    controller.borrow_mut().state.navigation.select(0);
    controller.borrow_mut().execute(&ui, Action::Activate); // Selects Music from Home.
    controller.borrow_mut().state.navigation.select(4); // Albums inside Music.
    controller.borrow_mut().execute(&ui, Action::Activate);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(110));
    assert_eq!(ui.get_previous_screen(), 1);
    assert_eq!(ui.get_previous_screen_title(), "Music");
    assert_eq!(
        ui.get_screen(),
        1,
        "Music and Albums share the same presentation class"
    );
    assert!(
        ui.get_transition_offset() > 0.0,
        "semantic navigation identity starts the library-to-library transition"
    );
    assert!(ui.get_previous_rows().row_count() > ui.get_rows().row_count());
    snapshot(&ui, "slide-music-albums");
    controller.borrow_mut().execute(&ui, Action::Back);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(60));
    assert_eq!(ui.get_previous_screen_title(), "Albums");
    assert_eq!(ui.get_screen_title(), "Music");
    assert!(
        ui.get_transition_offset() < 0.0,
        "a new Back request retargets an in-flight library transition"
    );
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    assert_eq!(ui.get_previous_rows().row_count(), 0);
    controller.borrow_mut().execute(&ui, Action::Settings);
    ui.invoke_item_clicked(0);
    let smooth_slides = controller
        .borrow()
        .state
        .rows()
        .iter()
        .position(|row| {
            matches!(
                row.target,
                nano_music_player::app::state::Target::Action(Action::ToggleSmoothSlides)
            )
        })
        .unwrap();
    controller
        .borrow_mut()
        .state
        .navigation
        .select(smooth_slides);
    controller.borrow_mut().execute(&ui, Action::Activate);
    assert!(!controller.borrow().state.smooth_slides);
    assert_eq!(
        ui.get_transition_offset(),
        0.0,
        "turning Smooth Slides off cancels the active transition"
    );
    assert!(
        !nano_music_player::app::config::Settings::load(&directory.path().join("settings.json"))
            .unwrap()
            .smooth_slides
    );
    controller.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(
        ui.get_transition_offset(),
        0.0,
        "disabled slides switch screens immediately"
    );
    controller.borrow_mut().execute(&ui, Action::Home);
    tap(&ui, "j");
    assert_eq!(ui.get_selected(), 2);
    tap(&ui, "k");
    assert_eq!(ui.get_selected(), 0);
    tap(&ui, Key::DownArrow);
    assert_eq!(ui.get_selected(), 2);
    tap(&ui, Key::UpArrow);
    assert_eq!(ui.get_selected(), 0);
    tap(&ui, Key::RightArrow);
    assert_eq!(ui.get_selected(), 1);
    tap(&ui, Key::LeftArrow);
    assert_eq!(ui.get_selected(), 0);
    tap(&ui, Key::Return);
    assert_eq!(ui.get_screen_title(), "Music");
    assert_eq!(ui.get_rows().row_data(1).unwrap().title, "Genius Mixes");
    // The manual's right-swipe Back applies to library levels. A multi-frame
    // swipe over an album row must not activate that row and restores parent selection.
    controller.borrow_mut().state.navigation.select(4);
    controller.borrow_mut().poll(&ui);
    controller.borrow_mut().execute(&ui, Action::Activate);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    assert_eq!(ui.get_screen_title(), "Albums");
    drag(&ui, (70.0, 150.0), (130.0, 152.0));
    assert_eq!(ui.get_screen_title(), "Music");
    assert_eq!(ui.get_selected(), 4, "Back restores the parent selection");
    // Accumulated trackpad deltas also navigate Back from the current library level.
    let scale = ui.get_display_scale();
    scroll(&ui, 120.0, 150.0, 18.0 * scale, 0.0);
    assert_eq!(ui.get_screen_title(), "Music", "short scroll is ignored");
    scroll(&ui, 120.0, 150.0, 18.0 * scale, 0.0);
    assert_eq!(
        ui.get_screen_title(),
        "iPod",
        "rightward scroll returns Back"
    );
    assert_eq!(ui.get_selected(), 0, "Back restores the Home selection");
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    controller.borrow_mut().execute(&ui, Action::Activate);
    controller.borrow_mut().state.navigation.select(5);
    controller.borrow_mut().execute(&ui, Action::Activate);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    assert_eq!(ui.get_screen_title(), "Songs");
    scroll(&ui, 120.0, 150.0, 4.0 * scale, -45.0 * scale);
    assert_eq!(
        ui.get_screen_title(),
        "Songs",
        "vertical scroll stays in list"
    );
    assert!(ui.get_list_scroll_y() < 0.0, "vertical scroll is preserved");
    controller.borrow_mut().execute(&ui, Action::Back);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
    assert_eq!(ui.get_screen_title(), "Music");
    ui.set_notice("".into());
    snapshot(&ui, "music-genius-entry");
    click(&ui, 120.0, 98.0);
    assert!(ui.get_genius_preview_open());
    assert_eq!(ui.get_screen_title(), "Music");
    tap(&ui, Key::Return);
    assert_eq!(
        ui.get_genius_preview_stage(),
        0,
        "a song is required for a seed"
    );
    assert!(controller.borrow().state.current.is_none());
    tap(&ui, Key::Escape);
    assert!(!ui.get_genius_preview_open());
    controller.borrow_mut().state.navigation.select(0);
    controller.borrow_mut().poll(&ui);
    for _ in 0..5 {
        tap(&ui, Key::DownArrow);
    }
    tap(&ui, Key::Return);
    assert_eq!(ui.get_screen_title(), "Songs");
    for _ in 0..9 {
        tap(&ui, Key::DownArrow);
    }
    assert_eq!(ui.get_selected(), 9);
    assert!(ui.get_list_scroll_y() < 0.0, "selection scrolls into view");
    let previous_scroll = ui.get_list_scroll_y();
    scroll(&ui, 120.0, 180.0, 0.0, -100.0);
    assert!(
        ui.get_list_scroll_y() < previous_scroll,
        "wheel/two-finger scroll moves the library"
    );
    assert_eq!(
        ui.get_selected(),
        9,
        "scrolling preserves keyboard selection"
    );
    snapshot(&ui, "songs");
    tap(&ui, Key::Escape);
    assert_eq!(ui.get_screen_title(), "Music");
    assert_eq!(ui.get_selected(), 5);

    primary(&ui, "f");
    assert_eq!(ui.get_screen(), 3);
    assert!(ui.get_search_focused());
    let volume = controller.borrow().state.volume;
    for key in ["s", "m", "r", "h", "n", "l", " "] {
        tap(&ui, key);
    }
    primary(&ui, Key::UpArrow);
    primary(&ui, "2");
    assert_eq!(controller.borrow().state.volume, volume);
    assert!(!controller.borrow().state.shuffle);
    assert!(!controller.borrow().state.muted);
    assert!(ui.get_device_mode());
    assert_eq!(ui.get_search_text(), "smrhnl ");
    // Cmd+Up was correctly passed through to native editing and moved the caret to the start.
    for _ in 0..10 {
        tap(&ui, Key::RightArrow);
    }
    tap(&ui, Key::Backspace);
    assert_eq!(ui.get_search_text(), "smrhnl");
    tap(&ui, Key::Return);
    assert!(!ui.get_search_focused());
    assert_eq!(ui.get_screen(), 3);
    primary(&ui, "f");
    primary(&ui, "a");
    tap(&ui, Key::Backspace);
    for key in ["T", "e", "s", "t"] {
        tap(&ui, key);
    }
    assert_eq!(ui.get_search_text(), "Test");
    snapshot(&ui, "search");
    tap(&ui, Key::Escape);
    assert!(!ui.get_search_focused());
    primary(&ui, Key::UpArrow);
    assert!(controller.borrow().state.volume > volume);
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyPressRepeated {
        text: Key::UpArrow.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    assert!(controller.borrow().state.volume > volume + 0.05);
    tap(&ui, "m");
    assert!(controller.borrow().state.muted);
    ui.window()
        .dispatch_event(WindowEvent::KeyPressRepeated { text: "m".into() });
    assert!(
        controller.borrow().state.muted,
        "holding M must not toggle repeatedly"
    );
    tap(&ui, "m");
    assert!(!controller.borrow().state.muted);
    tap(&ui, Key::DownArrow);
    // Search results have no Shuffle row.
    // The first Down selected the second matching song already.
    tap(&ui, Key::Return);
    assert_eq!(ui.get_screen(), 2);
    assert_eq!(ui.get_track_title(), "Test Song 01");
    snapshot(&ui, "now-playing");
    assert!(
        ui.get_lyrics_visible(),
        "lyrics display at track start by default"
    );
    controller.borrow_mut().state.position = 11.0;
    controller.borrow_mut().execute(&ui, Action::NowPlaying);
    assert_eq!(ui.get_active_lyric(), 1);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    snapshot(&ui, "lyrics");
    scroll(&ui, 120.0, 200.0, 0.0, 40.0);
    assert!(
        ui.get_lyrics_visible(),
        "scrolling lyrics does not toggle artwork"
    );
    snapshot(&ui, "lyrics-scrolled");
    {
        let current = controller.borrow().state.current;
        let queue = controller.borrow().state.queue.order.clone();
        let status = controller.borrow().state.status;
        let scale = ui.get_display_scale();
        scroll(&ui, 120.0, 200.0, -80.0 * scale, 0.0);
        scroll(&ui, 120.0, 200.0, 5.0 * scale, 0.0);
        assert_eq!(ui.get_screen(), 2, "leftward and short scrolling stay put");
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(600));
        scroll(&ui, 120.0, 200.0, 30.0 * scale, 0.0);
        assert_eq!(ui.get_screen(), 2, "old wheel deltas expire");
        scroll(&ui, 120.0, 200.0, 3.0 * scale, 40.0 * scale);
        assert_eq!(
            ui.get_screen(),
            2,
            "predominantly vertical scrolling stays in lyrics"
        );
        scroll(&ui, 120.0, 200.0, 18.0 * scale, 0.0);
        assert_eq!(
            ui.get_screen(),
            2,
            "vertical motion clears horizontal accumulation"
        );
        scroll(&ui, 120.0, 200.0, 18.0 * scale, 0.0);
        assert_eq!(ui.get_screen_title(), "Search");
        let parent_selection = ui.get_selected();
        assert_eq!(controller.borrow().state.current, current);
        assert_eq!(controller.borrow().state.queue.order, queue);
        assert_eq!(controller.borrow().state.status, status);
        assert_eq!(controller.borrow().state.position, 11.0);

        controller.borrow_mut().execute(&ui, Action::NowPlaying);
        drag(&ui, (90.0, 200.0), (150.0, 200.0));
        assert_eq!(
            ui.get_screen_title(),
            "Search",
            "rightward lyrics swipe goes Back"
        );
        assert_eq!(ui.get_selected(), parent_selection);
        controller.borrow_mut().execute(&ui, Action::NowPlaying);
        let volume = controller.borrow().state.volume;
        drag(&ui, (60.0, 121.0), (165.0, 121.0));
        assert_eq!(ui.get_screen(), 2, "seek drag must not navigate Back");
        assert!(controller.borrow().state.position > 11.0);
        drag(&ui, (60.0, 413.0), (160.0, 413.0));
        assert_eq!(ui.get_screen(), 2, "volume drag must not navigate Back");
        assert!(controller.borrow().state.volume > volume);
        ui.invoke_volume_changed(volume);
        controller.borrow_mut().state.position = 11.0;
        controller.borrow_mut().execute(&ui, Action::NowPlaying);
        drag(&ui, (110.0, 45.0), (120.0, 45.0));
        assert_eq!(ui.get_screen(), 2, "short drag stays put");
        drag(&ui, (90.0, 45.0), (150.0, 45.0));
        assert_eq!(ui.get_screen_title(), "Search", "metadata swipe goes Back");
        controller.borrow_mut().execute(&ui, Action::NowPlaying);
        ui.invoke_open_genius_preview();
        scroll(&ui, 120.0, 200.0, 80.0 * scale, 0.0);
        click(&ui, 20.0, 13.0);
        assert_eq!(ui.get_screen(), 2, "modal blocks playback navigation");
        ui.invoke_genius_action("close".into());
        snapshot(&ui, "now-playing-back");
        click(&ui, 20.0, 13.0);
        assert_eq!(
            ui.get_screen_title(),
            "Search",
            "visible Back returns to parent"
        );
        assert_eq!(ui.get_selected(), parent_selection);
        controller.borrow_mut().execute(&ui, Action::NowPlaying);
    }
    click(&ui, 120.0, 200.0);
    assert!(
        !ui.get_lyrics_visible(),
        "tap lyrics to return to artwork without an extra button"
    );
    drag(&ui, (90.0, 200.0), (150.0, 200.0));
    assert_eq!(ui.get_screen_title(), "Search", "artwork swipe goes Back");
    controller.borrow_mut().execute(&ui, Action::NowPlaying);
    tap(&ui, "y");
    assert!(ui.get_lyrics_visible());
    tap(&ui, Key::LeftArrow);
    assert!(controller.borrow().state.position < 11.0);
    tap(&ui, "h");
    assert_eq!(ui.get_screen_title(), "Search");
    ui.invoke_command("home".into());
    assert_eq!(ui.get_screen(), 0);
    tap(&ui, "s");
    assert!(
        controller.borrow().state.shuffle,
        "playback remains available on Home"
    );

    ui.invoke_command("show_shortcuts".into());
    assert!(ui.get_reference_open());
    tap(&ui, Key::DownArrow);
    assert_eq!(ui.get_selected(), 0);
    snapshot(&ui, "shortcuts");
    tap(&ui, Key::Escape);
    assert!(!ui.get_reference_open());
    primary(&ui, "2");
    assert!(!ui.get_device_mode());
    snapshot(&ui, "screen-mode");
    ui.window().set_size(slint::LogicalSize::new(240.0, 460.0));
    snapshot(&ui, "minimum-size");
    assert!(ui.get_display_scale() >= 0.8);
    primary(&ui, "+");
    primary(&ui, "-");
    primary(&ui, "1");
    assert!(ui.get_device_mode());
    ui.window().set_size(slint::LogicalSize::new(400.0, 700.0));
    controller.borrow_mut().poll(&ui);
    let dragging = ui.window().size().to_logical(ui.window().scale_factor());
    assert_eq!(
        dragging,
        slint::LogicalSize::new(400.0, 700.0),
        "poll must not fight an active native resize"
    );
    std::thread::sleep(std::time::Duration::from_millis(160));
    controller.borrow_mut().poll(&ui);
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    assert!(
        (size.width / 296.0 - ui.get_content_height() / (296.0 * 76.5 / 39.6)).abs() < 0.01,
        "mouse resizing keeps a tight device aspect ratio"
    );
    // Settings is reachable through its Home icon and the platform shortcut.
    ui.invoke_command("home".into());
    tap(&ui, "l");
    assert_eq!(ui.get_selected(), 1);
    tap(&ui, "l");
    assert_eq!(ui.get_home_page(), 1);
    tap(&ui, "l");
    assert_eq!(ui.get_selected(), 1);
    tap(&ui, Key::Return);
    assert_eq!(ui.get_screen_title(), "Settings");
    snapshot(&ui, "settings");
    ui.invoke_item_clicked(4);
    assert_eq!(ui.get_screen_title(), "Shortcuts");
    snapshot(&ui, "customize-shortcuts");
    tap(&ui, "j"); // Move selection down.
    tap(&ui, "l");
    assert!(ui.get_shortcut_editor_open());
    snapshot(&ui, "shortcut-editor");
    click(&ui, 65.0, 256.0);
    assert!(ui.get_shortcut_recording());
    let before = controller.borrow().dispatcher.keymap.bindings.clone();
    tap(&ui, " "); // Conflicts with Play / Pause and cannot execute playback.
    assert!(ui.get_shortcut_message().contains("already assigned"));
    assert_eq!(controller.borrow().dispatcher.keymap.bindings, before);
    tap(&ui, "x");
    assert!(!ui.get_shortcut_recording());
    assert_eq!(ui.get_shortcut_keys(), "X");
    click(&ui, 173.0, 256.0);
    assert!(ui.get_shortcut_recording());
    tap(&ui, "d");
    assert_eq!(ui.get_shortcut_keys(), "X / D");
    snapshot(&ui, "shortcut-saved");
    ui.invoke_shortcut_command("done".into());
    let saved =
        nano_music_player::input::Keymap::load_or_create(&directory.path().join("shortcuts.json"))
            .unwrap();
    assert_eq!(
        saved.bindings,
        controller.borrow().dispatcher.keymap.bindings
    );
    assert_eq!(saved.labels(Action::SelectNext), "X / D");
    tap(&ui, "j");
    assert_eq!(
        ui.get_selected(),
        1,
        "replaced aliases stop working immediately"
    );
    tap(&ui, "x");
    assert_eq!(ui.get_selected(), 2);
    tap(&ui, "d");
    assert_eq!(ui.get_selected(), 3);
    tap(&ui, "k");
    tap(&ui, "k");
    tap(&ui, "l");
    click(&ui, 65.0, 298.0);
    assert_eq!(ui.get_shortcut_keys(), "Unassigned");
    click(&ui, 173.0, 298.0);
    assert!(ui.get_shortcut_keys().contains("J"));
    ui.invoke_shortcut_command("replace".into());
    tap(&ui, slint::platform::Key::Escape);
    assert!(!ui.get_shortcut_recording());
    assert!(ui.get_shortcut_editor_open());
    tap(&ui, slint::platform::Key::Escape);
    assert!(!ui.get_shortcut_editor_open());
    tap(&ui, "h");
    assert_eq!(ui.get_screen_title(), "Settings");
    primary(&ui, ",");
    assert_eq!(ui.get_screen_title(), "Settings");

    // The artwork is retained under the lyrics, even when no timed lyrics exist.
    let artwork = image::RgbaImage::from_fn(480, 480, |x, y| {
        let circle = (i64::from(x) - 240).pow(2) + (i64::from(y) - 240).pow(2) < 130_i64.pow(2);
        if circle {
            image::Rgba([225, 182, 106, 255])
        } else {
            image::Rgba([36, (55 + y / 6) as u8, (68 + x / 6) as u8, 255])
        }
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    artwork
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    {
        let mut controller = controller.borrow_mut();
        controller.state.tracks[1].artwork = bytes.into_inner();
        controller.state.tracks[1].lyrics = Lyrics::parse(
            "[00:00]A small original lyric fixture\n[00:10]With artwork behind the words\n[00:20]And room to scroll the next line\n[00:30]Another line for the reader\n[00:40]The last line of this test",
        );
        controller.state.current = None;
        controller.execute(&ui, Action::NowPlaying);
        controller.state.start(1);
        controller.state.status = nano_music_player::audio::PlaybackStatus::Playing;
        controller.state.position = 11.0;
        controller.execute(&ui, Action::NowPlaying);
    }
    ui.invoke_command("toggle_overlay".into());
    if !ui.get_overlay_visible() {
        ui.invoke_command("toggle_overlay".into());
    }
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    // Now Playing artwork may be decoded by the independent background worker.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while ui.get_artwork().size().width == 0 && std::time::Instant::now() < deadline {
        controller.borrow_mut().poll(&ui);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(ui.get_artwork().size().width > 0);
    assert!(ui.get_lyrics_visible());
    snapshot(&ui, "lyrics-with-artwork");
    click(&ui, 200.0, 160.0); // Album track list.
    assert_eq!(ui.get_screen_title(), "Test Album");
    tap(&ui, "h");
    assert_eq!(ui.get_screen_title(), "Now Playing");
    click(&ui, 120.0, 270.0); // Cover tap hides secondary controls and lyrics.
    assert!(!ui.get_lyrics_visible());
    assert!(!ui.get_overlay_visible());
    snapshot(&ui, "cover-with-artwork");
    // Per-track lyrics management operates on the chosen track, persists removal and restores sidecars.
    let audio_path = directory.path().join("lyrics-track.mp3");
    std::fs::copy("tests/fixtures/quiet.mp3", &audio_path).unwrap();
    std::fs::write(
        audio_path.with_extension("txt"),
        "Original plain text\n\nAnother line",
    )
    .unwrap();
    {
        let mut controller = controller.borrow_mut();
        controller.state.tracks[1].path = audio_path.clone();
        let db = nano_music_player::library::database::Database::open(
            &directory.path().join("library.sqlite3"),
        )
        .unwrap();
        db.save(&controller.state.tracks[1], 0).unwrap();
        controller.execute(&ui, Action::ManageLyrics);
    }
    assert_eq!(ui.get_screen(), 7);
    assert_eq!(
        ui.get_rows().row_data(0).unwrap().title,
        "Import / Replace Lyrics…"
    );
    assert_eq!(ui.get_rows().row_data(1).unwrap().title, "Remove Lyrics");
    assert!(
        ui.get_rows()
            .iter()
            .any(|row| row.title == "Generate Romaji")
    );
    snapshot(&ui, "manage-lyrics");
    ui.invoke_command("remove_lyrics".into());
    for _ in 0..100 {
        controller.borrow_mut().poll(&ui);
        if controller.borrow().state.tracks[1].lyrics.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(controller.borrow().state.tracks[1].lyrics.is_empty());
    assert_eq!(
        nano_music_player::library::database::Database::open(
            &directory.path().join("library.sqlite3")
        )
        .unwrap()
        .lyrics_association(&audio_path)
        .unwrap(),
        nano_music_player::library::database::LyricsAssociation::Removed
    );
    ui.invoke_command("automatic_lyrics".into());
    for _ in 0..100 {
        controller.borrow_mut().poll(&ui);
        if !controller.borrow().state.tracks[1].lyrics.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        controller.borrow().state.tracks[1].lyrics.lines(),
        ["Original plain text", "", "Another line"]
    );
    controller.borrow_mut().execute(&ui, Action::NowPlaying);
    assert!(ui.get_lyrics_visible());
    assert!(!ui.get_timed_lyrics());
    snapshot(&ui, "plain-lyrics");
    // Seek synchronizes both highlight and scrolling back to the start.
    {
        let mut controller = controller.borrow_mut();
        controller.state.tracks[1].lyrics = Lyrics::parse(
            "[00:00]First\n[00:10]A longer second line which wraps across several lines in the small display\n[00:20]Third\n[00:30]Fourth\n[00:40]Fifth\n[00:50]Sixth",
        );
        controller.state.current = None;
        controller.execute(&ui, Action::NowPlaying);
        controller.state.start(1);
        controller.state.position = 51.0;
        controller.execute(&ui, Action::NowPlaying);
    }
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    let _ = ui.window().take_snapshot().unwrap();
    assert_eq!(ui.get_active_lyric(), 5);
    assert!(ui.get_lyric_scroll_y() < 0.0);
    ui.invoke_seek_changed(0.0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    assert_eq!(ui.get_active_lyric(), 0);
    assert_eq!(ui.get_lyric_scroll_y(), 0.0);
    drag(&ui, (120.0, 285.0), (120.0, 205.0));
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    assert_eq!(
        ui.get_screen(),
        2,
        "vertical lyrics drag stays in Now Playing"
    );
    assert!(ui.get_lyrics_visible());
    assert!(
        ui.get_lyric_scroll_y() < 0.0,
        "vertical drag still scrolls lyrics"
    );
    ui.invoke_seek_changed(0.0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));

    // Geometry regression: arbitrary user resizes converge to the same logical proportions.
    for mode in [Action::ScreenMode, Action::DeviceMode] {
        controller.borrow_mut().execute(&ui, mode);
        for (width, height) in [(300.0, 900.0), (650.0, 500.0), (240.0, 432.0)] {
            ui.window().set_size(slint::LogicalSize::new(width, height));
            controller.borrow_mut().poll(&ui);
            std::thread::sleep(std::time::Duration::from_millis(160));
            controller.borrow_mut().poll(&ui);
            let size = ui.window().size().to_logical(ui.window().scale_factor());
            let ratio = if mode == Action::ScreenMode {
                240.0 / 432.0
            } else {
                39.6 / 76.5
            };
            assert!((size.width / ui.get_content_height() - ratio).abs() < 0.002);
        }
    }
    controller.borrow_mut().execute(&ui, Action::ScreenMode);
    ui.set_keyboard_navigation(false);
    ui.set_overlay_visible(false);
    ui.set_lyrics_visible(false);
    // Contain must leave letterboxing, rather than crop portrait/landscape edges.
    for (width, height, name) in [
        (100, 200, "portrait-cover"),
        (200, 100, "landscape-cover"),
        (100, 100, "square-cover"),
    ] {
        let pixels = [219u8, 52, 111, 255].repeat((width * height) as usize);
        ui.set_artwork(slint::Image::from_rgba8(slint::SharedPixelBuffer::<
            slint::Rgba8Pixel,
        >::clone_from_slice(
            &pixels, width, height
        )));
        snapshot(&ui, name);
        let image = ui.window().take_snapshot().unwrap();
        let sample = |x, y| {
            let p = display_point(&ui, x, y);
            let offset = (p.y.round() as usize * image.width() as usize + p.x.round() as usize) * 4;
            image.as_bytes()[offset..offset + 3].to_vec()
        };
        assert_eq!(sample(120.0, 202.0), [219, 52, 111]);
        assert_eq!(sample(25.0, 202.0) == [219, 52, 111], width >= height);
        assert_eq!(sample(120.0, 100.0) == [219, 52, 111], height >= width);
    }
    // Progress is anchored at x=0: an early position fills the left, never the center.
    ui.set_overlay_visible(true);
    for (progress, left_filled, right_filled) in
        [(0.0, false, false), (0.25, true, false), (1.0, true, true)]
    {
        ui.set_progress(progress);
        let image = ui.window().take_snapshot().unwrap();
        let is_filled = |x| {
            let p = display_point(&ui, x, 121.0);
            let offset = (p.y.round() as usize * image.width() as usize + p.x.round() as usize) * 4;
            image.as_bytes()[offset] > 150
        };
        assert_eq!(is_filled(35.0), left_filled, "left at {progress}");
        assert_eq!(is_filled(195.0), right_filled, "right at {progress}");
    }
    // The F prefix belongs to list navigation and consumes letters that otherwise control playback.
    {
        let mut controller = controller.borrow_mut();
        let mut track = controller.state.tracks[1].clone();
        track.path = directory.path().join("zebra.mp3");
        track.title = "Zebra".into();
        track.album = "Zebra Album".into();
        track.artist = "Zebra Artist".into();
        controller.state.merge_library(vec![track]);
        controller
            .state
            .navigation
            .open(nano_music_player::app::navigation::Screen::Songs(
                nano_music_player::app::navigation::Filter::All,
            ));
        controller.execute(&ui, Action::FinishSearchEditing);
    }
    tap(&ui, "f");
    tap(&ui, "z");
    assert_eq!(ui.get_selected(), 21);
    assert!(ui.get_list_scroll_y() < 0.0);
    let shuffle = controller.borrow().state.shuffle;
    tap(&ui, "f");
    tap(&ui, "s"); // No title starts with S; must not shuffle.
    assert_eq!(controller.borrow().state.shuffle, shuffle);
    assert_eq!(ui.get_selected(), 21);
    tap(&ui, "f");
    tap(&ui, Key::Escape);
    assert_eq!(ui.get_screen_title(), "Songs");
    tap(&ui, "f");
    primary(&ui, "f"); // Modified keys cancel the prefix and retain native search behavior.
    assert!(ui.get_search_focused());
    tap(&ui, "f");
    tap(&ui, "z");
    assert!(ui.get_search_text().ends_with("fz"));
    tap(&ui, Key::Escape);
    ui.invoke_command("home".into());
    tap(&ui, "f");
    tap(&ui, "l"); // F has no prefix effect on Home.
    assert_eq!(ui.get_selected(), 1);
    {
        let mut controller = controller.borrow_mut();
        controller
            .state
            .navigation
            .open(nano_music_player::app::navigation::Screen::Groups(
                nano_music_player::app::navigation::Group::Albums,
            ));
        controller.execute(&ui, Action::FinishSearchEditing);
    }
    assert_eq!(ui.get_rows().row_data(0).unwrap().title, "All Songs");
    assert!(ui.get_rows().row_data(0).unwrap().show_artwork);
    assert!(ui.get_rows().row_data(1).unwrap().show_artwork);
    wait_for_library_artwork(&ui, &controller, 1);
    assert!(ui.get_rows().row_data(1).unwrap().artwork.size().width > 0);
    snapshot(&ui, "album-thumbnails");
    tap(&ui, "f");
    tap(&ui, "z");
    assert_eq!(ui.get_selected(), 2);
    {
        let mut controller = controller.borrow_mut();
        controller
            .state
            .navigation
            .open(nano_music_player::app::navigation::Screen::Groups(
                nano_music_player::app::navigation::Group::Artists,
            ));
        controller.execute(&ui, Action::FinishSearchEditing);
    }
    wait_for_library_artwork(&ui, &controller, 0);
    snapshot(&ui, "artist-thumbnails");
    assert!(ui.get_rows().row_data(0).unwrap().artwork.size().width > 0);

    // Larger cover rows use the rendered geometry for keyboard/F-prefix scrolling.
    {
        let mut controller = controller.borrow_mut();
        let template = controller.state.tracks[1].clone();
        controller.state.merge_library(
            (0..8)
                .map(|id| {
                    let mut track = template.clone();
                    track.path = directory.path().join(format!("gallery-{id}.mp3"));
                    track.album = format!("Gallery Album {id:02}");
                    track
                })
                .collect(),
        );
        controller
            .state
            .navigation
            .open(nano_music_player::app::navigation::Screen::Groups(
                nano_music_player::app::navigation::Group::Albums,
            ));
        controller.execute(&ui, Action::FinishSearchEditing);
    }
    assert!((ui.get_list_row_height() - 58.0 * ui.get_display_scale()).abs() < 0.01);
    assert!((ui.get_list_viewport_height() - 406.0 * ui.get_display_scale()).abs() < 0.01);
    for row in 1..8 {
        wait_for_library_artwork(&ui, &controller, row);
    }
    snapshot(&ui, "larger-album-thumbnails");
    tap(&ui, "f");
    tap(&ui, "z");
    assert_eq!(ui.get_selected(), 10);
    assert!((ui.get_list_scroll_y() - (406.0 - 11.0 * 58.0) * ui.get_display_scale()).abs() < 0.01);
    for row in 4..11 {
        wait_for_library_artwork(&ui, &controller, row);
    }
    snapshot(&ui, "larger-album-jump");

    // Language changes through Settings persist while catalog-like metadata remains literal.
    use nano_music_player::app::{config::Settings, i18n::Language, navigation::Screen};
    let original_lyrics = controller.borrow().state.tracks[1].lyrics.clone();
    {
        let mut controller = controller.borrow_mut();
        controller.state.tracks[1].title = "Music".into();
        controller.state.tracks[1].artist = "Settings".into();
        controller.state.tracks[1].album = "Lyrics".into();
        controller.state.current = None;
        controller.execute(&ui, Action::NowPlaying);
        controller.state.start(1);
        controller.execute(&ui, Action::NowPlaying);
    }
    for (language, title, settings_title) in [
        (Language::SimplifiedChinese, "语言", "设置"),
        (Language::Japanese, "言語", "設定"),
        (Language::English, "Language", "Settings"),
    ] {
        ui.invoke_command("settings".into());
        ui.invoke_item_clicked(3);
        assert_eq!(
            controller.borrow().state.navigation.current().screen,
            Screen::Languages
        );
        ui.invoke_item_clicked(language.index() as i32);
        assert_eq!(ui.get_screen_title(), title);
        assert_eq!(
            ui.get_rows().row_data(language.index()).unwrap().checked,
            true
        );
        assert_eq!(
            Settings::load(&directory.path().join("settings.json"))
                .unwrap()
                .language,
            language
        );
        snapshot(&ui, &format!("languages-{}", language.code()));
        ui.invoke_command("settings".into());
        assert_eq!(ui.get_screen_title(), settings_title);
        snapshot(&ui, &format!("settings-{}", language.code()));
        ui.invoke_command("now_playing".into());
        assert_eq!(ui.get_track_title(), "Music");
        assert_eq!(ui.get_track_artist(), "Settings");
        assert_eq!(ui.get_track_album(), "Lyrics");
        assert_eq!(controller.borrow().state.tracks[1].lyrics, original_lyrics);
        ui.invoke_command("home".into());
        snapshot(&ui, &format!("home-{}", language.code()));
    }
    // R is contextual: only the song interface toggles romaji; inputs keep their letters.
    controller.borrow_mut().execute(&ui, Action::NowPlaying);
    let romaji_before = controller.borrow().state.romaji_visible;
    let repeat_before = controller.borrow().state.repeat;
    tap(&ui, "r");
    assert_eq!(controller.borrow().state.romaji_visible, !romaji_before);
    assert_eq!(controller.borrow().state.repeat, repeat_before);
    tap(&ui, "r");
    assert_eq!(controller.borrow().state.romaji_visible, romaji_before);
    controller.borrow_mut().state.navigation.select(3); // Now Playing volume context.
    tap(&ui, "r");
    assert_eq!(controller.borrow().state.romaji_visible, !romaji_before);
    tap(&ui, "r");
    ui.invoke_command("home".into());
    tap(&ui, "r");
    assert_eq!(controller.borrow().state.romaji_visible, romaji_before);
    assert_ne!(controller.borrow().state.repeat, repeat_before);
    ui.invoke_command("search".into());
    let search_before = ui.get_search_text().to_string();
    tap(&ui, "r");
    assert!(ui.get_search_text().to_string().contains('r'));
    assert_ne!(ui.get_search_text().to_string(), search_before);
    assert_eq!(controller.borrow().state.romaji_visible, romaji_before);
    tap(&ui, Key::Escape);

    controller.borrow_mut().execute(&ui, Action::DeviceMode);
    ui.window().set_size(slint::LogicalSize::new(592.0, 1172.0));
    controller.borrow_mut().poll(&ui);
    ui.invoke_command("configure_deep_seek".into());
    assert!(ui.get_deepseek_settings_open());
    assert_eq!(ui.get_deepseek_api_key(), "");
    ui.invoke_focus_screen();
    let repeat_before = controller.borrow().state.repeat;
    for key in ["r", "s", "m"] {
        tap(&ui, key);
    }
    assert_eq!(ui.get_deepseek_api_key(), "rsm");
    assert_eq!(controller.borrow().state.romaji_visible, romaji_before);
    assert_eq!(controller.borrow().state.repeat, repeat_before);
    let masked = ui.window().take_snapshot().unwrap();
    ui.set_deepseek_api_key("abc".into());
    let masked_replacement = ui.window().take_snapshot().unwrap();
    assert_eq!(
        masked.as_bytes(),
        masked_replacement.as_bytes(),
        "equal-length keys render as identical masks"
    );
    snapshot(&ui, "deepseek-masked-key");
    click(&ui, 175.0, 362.0); // Save from the dialog.
    assert!(!ui.get_deepseek_settings_open());
    assert!(ui.get_deepseek_key_configured());
    ui.invoke_command("configure_deep_seek".into());
    assert_eq!(
        ui.get_deepseek_api_key(),
        "",
        "never prefill a configured key"
    );
    snapshot(&ui, "deepseek-saved-key");
    tap(&ui, Key::Return); // Blank retains the configured key.
    assert!(!ui.get_deepseek_settings_open());
    ui.invoke_command("configure_deep_seek".into());
    tap(&ui, Key::Escape);
    assert!(!ui.get_deepseek_settings_open());

    // Romaji uses the same lyric index and measured row, including wrapped readings.
    controller.borrow_mut().execute(&ui, Action::DeviceMode);
    ui.window().set_size(slint::LogicalSize::new(592.0, 1172.0));
    controller.borrow_mut().poll(&ui);
    ui.set_screen(2);
    ui.set_has_track(true);
    ui.set_keyboard_navigation(false);
    ui.set_notice("".into());
    ui.set_track_title("夜の散歩".into());
    ui.set_track_artist("Demo Artist".into());
    ui.set_track_album("Romaji Preview".into());
    ui.set_clock_text("9:41".into());
    ui.set_elapsed("0:19".into());
    ui.set_duration("4:18".into());
    ui.set_track_count("2 of 5".into());
    ui.set_progress(19.0 / 258.0);
    ui.set_artwork(slint::Image::from_rgba8(slint::SharedPixelBuffer::<
        slint::Rgba8Pixel,
    >::clone_from_slice(
        artwork.as_raw(), 480, 480
    )));
    ui.set_overlay_visible(true);
    ui.set_lyrics_visible(true);
    ui.set_lyric_lines(slint::ModelRc::new(slint::VecModel::from(
        [
            "静かな夜に",
            "街の灯りが揺れる",
            "風はそっと通り過ぎる",
            "小さな夢を抱いて",
            "また明日へ歩こう",
            "星を見上げながら",
        ]
        .map(slint::SharedString::from)
        .to_vec(),
    )));
    ui.set_romaji_lines(slint::ModelRc::new(slint::VecModel::from(
        [
            "Shizuka na yoru ni",
            "Machi no akari ga yureru",
            "Kaze wa sotto toorisugiru",
            "Chiisana yume o daite",
            "Mata ashita e arukou",
            "Hoshi o miagenagara",
        ]
        .map(slint::SharedString::from)
        .to_vec(),
    )));
    ui.set_romaji_visible(true);
    ui.set_active_lyric(0);
    ui.set_lyric_scroll_y(0.0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    snapshot(&ui, "romaji-device-preview");
    let playback_before = controller.borrow().state.status;
    let queue_before = controller.borrow().state.queue.current();
    click(&ui, 90.0, 163.0); // Genius, between Repeat and Shuffle.
    assert!(ui.get_genius_preview_open());
    assert_eq!(ui.get_genius_preview_stage(), 0);
    snapshot(&ui, "genius-create-preview");
    tap(&ui, Key::Return);
    assert_eq!(ui.get_genius_preview_stage(), 1);
    let frozen = ui.window().take_snapshot().unwrap();
    ui.set_track_title("A different playing song".into());
    assert_eq!(
        frozen.as_bytes(),
        ui.window().take_snapshot().unwrap().as_bytes(),
        "preview keeps its captured seed when the playing title changes"
    );
    ui.set_track_title("夜の散歩".into());
    snapshot(&ui, "genius-ready-preview");
    tap(&ui, "r");
    assert_eq!(ui.get_genius_preview_variation(), 0); // A failed Create retries from the frozen seed.
    assert_eq!(controller.borrow().state.romaji_visible, romaji_before);
    tap(&ui, Key::Return);
    assert_eq!(ui.get_genius_preview_stage(), 1); // No result exists to play in this offline fixture.
    assert_eq!(controller.borrow().state.status, playback_before);
    assert_eq!(controller.borrow().state.queue.current(), queue_before);
    tap(&ui, "s");
    assert_eq!(ui.get_genius_preview_stage(), 1); // Empty results cannot claim a saved playlist.
    snapshot(&ui, "genius-save-preview");
    click(&ui, 24.0, 16.0); // Close in the compact Genius header.
    assert!(!ui.get_genius_preview_open());
    ui.invoke_open_genius_preview();
    tap(&ui, Key::Escape);
    assert!(!ui.get_genius_preview_open());
    ui.set_active_lyric(5);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    let _ = ui.window().take_snapshot().unwrap();
    assert!(
        ui.get_lyric_scroll_y() < 0.0,
        "romaji rows scroll with the active Japanese line"
    );
    ui.set_active_lyric(0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    let _ = ui.window().take_snapshot().unwrap();
    assert_eq!(
        ui.get_lyric_scroll_y(),
        0.0,
        "backward highlight returns both lines to the top"
    );
    ui.set_romaji_visible(false);
    snapshot(&ui, "romaji-hidden");
    ui.set_romaji_visible(true);
    ui.set_romaji_lines(slint::ModelRc::new(slint::VecModel::from(vec![
        slint::SharedString::from(""),
    ])));
    snapshot(&ui, "romaji-empty-entries");

    // Playlist creation uses the controller, real fixture paths, and persistent storage.
    use nano_music_player::{app::navigation::Filter, library::playlists};
    for track in &controller.borrow().state.tracks {
        std::fs::write(&track.path, include_bytes!("fixtures/quiet.wav")).unwrap();
    }
    let romaji_before = controller.borrow().state.romaji_visible;
    let repeat_before = controller.borrow().state.repeat;
    ui.invoke_command("new_playlist".into());
    controller.borrow_mut().poll(&ui);
    assert!(ui.get_playlist_editor_open());
    for letter in "Evening Mix".chars() {
        tap(&ui, letter.to_string());
    }
    assert_eq!(ui.get_playlist_name(), "Evening Mix");
    tap(&ui, "r");
    assert_eq!(ui.get_playlist_name(), "Evening Mixr");
    tap(&ui, Key::Backspace);
    assert_eq!(controller.borrow().state.romaji_visible, romaji_before);
    assert_eq!(controller.borrow().state.repeat, repeat_before);
    tap(&ui, Key::Return); // Leave name editing for song selection.
    tap(&ui, Key::DownArrow);
    tap(&ui, " ");
    tap(&ui, Key::DownArrow);
    tap(&ui, " ");
    assert_eq!(ui.get_playlist_draft().row_count(), 2);
    snapshot(&ui, "playlist-select-preview");
    tap(&ui, Key::Return);
    assert!(ui.get_playlist_reviewing());
    snapshot(&ui, "playlist-review-preview");
    tap(&ui, Key::DownArrow);
    click(&ui, 42.0, 343.0); // Move the selected second song above the first.
    assert_eq!(
        ui.get_playlist_draft().row_data(0).unwrap().title,
        "Test Song 01"
    );
    assert_eq!(
        ui.get_playlist_draft().row_data(1).unwrap().title,
        "Test Song 00"
    );
    let removed = ui.get_playlist_draft().row_data(0).unwrap().id;
    click(&ui, 191.0, 343.0);
    assert_eq!(ui.get_playlist_draft().row_count(), 1);
    tap(&ui, Key::Escape);
    ui.invoke_playlist_action("toggle".into(), removed);
    tap(&ui, Key::Return);
    tap(&ui, Key::DownArrow);
    click(&ui, 42.0, 343.0);
    assert_eq!(
        ui.get_playlist_draft().row_data(0).unwrap().title,
        "Test Song 01"
    );
    for language in [
        Language::SimplifiedChinese,
        Language::Japanese,
        Language::English,
    ] {
        choose_language(&ui, &controller, language);
        assert_eq!(ui.get_playlist_name(), "Evening Mix");
        snapshot(&ui, &format!("playlist-review-{}", language.code()));
    }
    snapshot(&ui, "playlist-review-preview");
    let queue_before_playlist = controller.borrow().state.queue.current();
    click(&ui, 178.0, 409.0);
    assert!(ui.get_playlist_saving());
    tap(&ui, Key::Escape);
    assert!(
        ui.get_playlist_editor_open(),
        "save cannot be cancelled after submission"
    );
    wait_for_playlist_save(&ui, &controller);
    assert!(!ui.get_playlist_editor_open());
    assert!(!ui.get_playlist_saving());
    assert_eq!(controller.borrow().state.playlists.len(), 1);
    assert_eq!(
        controller.borrow().state.queue.current(),
        queue_before_playlist
    );
    let saved = playlists::load(&directory.path().join("library.sqlite3")).unwrap();
    assert_eq!(saved[0].name, "Evening Mix");
    assert_eq!(saved[0].tracks.len(), 2);
    assert!(matches!(
        controller.borrow().state.navigation.current().screen,
        Screen::Songs(Filter::Playlist { .. })
    ));
    snapshot(&ui, "playlist-saved-list");
    // Opening the parent list renders the compact playlist rows with their note glyph.
    ui.invoke_command("back".into());
    controller.borrow_mut().poll(&ui);
    // The fixed Add toolbar participates in exactly the same J/K selection as the rows.
    assert_eq!(ui.get_selected(), 0);
    assert_eq!(ui.get_rows().row_data(0).unwrap().title, "Add");
    tap(&ui, "j");
    assert_eq!(ui.get_selected(), 1);
    assert_eq!(ui.get_rows().row_data(1).unwrap().title, "Edit");
    tap(&ui, "k");
    assert_eq!(ui.get_selected(), 0);
    tap(&ui, Key::Return);
    assert!(ui.get_playlist_editor_open());
    tap(&ui, "j");
    assert_eq!(
        ui.get_playlist_name(),
        "j",
        "keyboard entry starts in the name field"
    );
    tap(&ui, Key::Escape);
    assert!(!ui.get_playlist_editor_open());
    assert_eq!(
        controller.borrow().state.queue.current(),
        queue_before_playlist
    );
    click(&ui, 60.0, 42.0); // Add toolbar uses the same semantic row activation.
    assert!(ui.get_playlist_editor_open());
    tap(&ui, "k");
    assert_eq!(
        ui.get_playlist_name(),
        "k",
        "pointer entry keeps name editing focused"
    );
    tap(&ui, Key::Escape);
    snapshot(&ui, "playlist-list-preview");
    for language in [
        Language::SimplifiedChinese,
        Language::Japanese,
        Language::English,
    ] {
        choose_language(&ui, &controller, language);
        let saved_row = ui
            .get_rows()
            .iter()
            .find(|row| row.title == "Evening Mix")
            .unwrap();
        assert_eq!(saved_row.title, "Evening Mix");
        assert_eq!(
            saved_row.subtitle,
            match language {
                Language::English => "2 songs",
                Language::SimplifiedChinese => "2 首歌曲",
                Language::Japanese => "2 曲",
            }
        );
        assert_eq!(ui.get_playlist_name(), "k");
        snapshot(&ui, &format!("playlist-list-{}", language.code()));
    }
    tap(&ui, "r");
    assert_eq!(
        controller.borrow().state.romaji_visible,
        romaji_before,
        "playlist modal does not inherit the song-interface R shortcut"
    );
    ui.invoke_command("new_playlist".into());
    controller.borrow_mut().poll(&ui);
    assert!(ui.get_playlist_editor_open());
    assert_eq!(controller.borrow().state.playlists.len(), 1);
    let queue_before_cancel = controller.borrow().state.queue.current();
    tap(&ui, Key::Escape);
    assert!(!ui.get_playlist_editor_open());
    assert_eq!(
        controller.borrow().state.queue.current(),
        queue_before_cancel
    );
    assert_eq!(controller.borrow().state.playlists.len(), 1);

    // Invalid names and storage failures retain the same editable draft for retry.
    ui.invoke_command("new_playlist".into());
    let candidate = ui.get_playlist_candidates().row_data(0).unwrap().id;
    ui.invoke_playlist_action("toggle".into(), candidate.clone());
    for (name, error) in [
        (" ", "Choose a playlist name"),
        ("EVENING MIX", "A playlist with this name already exists"),
    ] {
        ui.set_playlist_name(name.into());
        ui.invoke_playlist_action("create".into(), "".into());
        wait_for_playlist_save(&ui, &controller);
        assert!(ui.get_playlist_editor_open());
        assert_eq!(ui.get_playlist_name(), name);
        assert_eq!(ui.get_playlist_draft().row_data(0).unwrap().id, candidate);
        assert_eq!(ui.get_playlist_message(), error);
    }
    let database = directory.path().join("library.sqlite3");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_playlist_ui BEFORE INSERT ON playlist_tracks BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
    ui.set_playlist_name("Music".into());
    ui.invoke_playlist_action("create".into(), "".into());
    wait_for_playlist_save(&ui, &controller);
    assert!(ui.get_playlist_editor_open());
    assert_eq!(
        ui.get_playlist_message(),
        "Could not save playlist. Try again."
    );
    assert_eq!(playlists::load(&database).unwrap().len(), 1);
    assert_eq!(ui.get_playlist_draft().row_data(0).unwrap().id, candidate);
    connection
        .execute_batch("DROP TRIGGER fail_playlist_ui")
        .unwrap();
    ui.invoke_playlist_action("create".into(), "".into());
    wait_for_playlist_save(&ui, &controller);
    assert!(!ui.get_playlist_editor_open());
    let saved = playlists::load(&database).unwrap();
    assert_eq!(saved.len(), 2);
    let mut restarted = Controller::new(directory.path().to_path_buf());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while restarted.state.playlists.len() != 2 {
        restarted.poll(&ui);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(restarted.state.playlists, saved);
    controller.borrow_mut().execute(&ui, Action::Back);
    choose_language(&ui, &controller, Language::SimplifiedChinese);
    let literal_playlist = controller.borrow().state.rows().iter().position(|row|
        matches!(&row.target, nano_music_player::app::state::Target::Screen(Screen::Songs(Filter::Playlist { name, .. })) if name == "Music")).unwrap();
    assert_eq!(
        ui.get_rows().row_data(literal_playlist).unwrap().title,
        "Music",
        "playlist names remain literal even when matching UI keys"
    );
    choose_language(&ui, &controller, Language::English);
    // Selecting songs alone enables Review; a blank name becomes a unique native-style name.
    controller.borrow_mut().execute(&ui, Action::NewPlaylist);
    let candidate = ui.get_playlist_candidates().row_data(0).unwrap().id;
    ui.invoke_playlist_action("toggle".into(), candidate.clone());
    assert_eq!(ui.get_playlist_name(), "");
    click(&ui, 178.0, 409.0);
    assert!(ui.get_playlist_reviewing());
    assert_eq!(ui.get_playlist_name(), "New Playlist 1");
    snapshot(&ui, "playlist-automatic-name");
    click(&ui, 178.0, 409.0);
    wait_for_playlist_save(&ui, &controller);
    assert!(!ui.get_playlist_editor_open());
    assert!(
        playlists::load(&database)
            .unwrap()
            .iter()
            .any(|p| p.name == "New Playlist 1")
    );
    controller.borrow_mut().execute(&ui, Action::NewPlaylist);
    ui.invoke_playlist_action("toggle".into(), candidate);
    tap(&ui, Key::Return); // Selecting a song leaves name input; Enter now reviews.
    assert!(ui.get_playlist_reviewing());
    assert!(!ui.get_playlist_saving());
    assert_eq!(ui.get_playlist_name(), "New Playlist 2");
    tap(&ui, Key::Escape);
    tap(&ui, Key::Escape);
    assert!(
        !ui.get_playlist_editor_open(),
        "automatic-name draft cancellation closes the editor"
    );
    controller.borrow_mut().execute(&ui, Action::NowPlaying);

    // Empty/loading states come from a fresh library rather than a disconnected adapter.
    let empty_directory = tempfile::tempdir().unwrap();
    let mut empty = Controller::new(empty_directory.path().to_path_buf());
    empty.initialize(&ui);
    empty.execute(&ui, Action::NewPlaylist);
    assert!(
        ui.get_playlist_library_loading(),
        "loading state: open={}, saving={}, candidates={}",
        ui.get_playlist_editor_open(),
        ui.get_playlist_saving(),
        ui.get_playlist_candidates().row_count()
    );
    snapshot(&ui, "playlist-loading");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while ui.get_playlist_library_loading() {
        empty.poll(&ui);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(ui.get_playlist_candidates().row_count(), 0);
    snapshot(&ui, "playlist-empty");
    ui.set_playlist_editor_open(false);
    controller.borrow_mut().initialize(&ui);

    // Settings fonts apply immediately, persist, and retain metadata/lyrics verbatim.
    use nano_music_player::app::typography::Font;
    let track_before = controller.borrow().state.tracks[1].clone();
    ui.invoke_command("settings".into());
    ui.invoke_item_clicked(0);
    let font_row = controller
        .borrow()
        .state
        .rows()
        .iter()
        .position(|row| {
            matches!(
                row.target,
                nano_music_player::app::state::Target::Screen(Screen::Fonts)
            )
        })
        .unwrap();
    ui.invoke_item_clicked(font_row as i32);
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::Fonts
    );
    snapshot(&ui, "font-settings");
    for font in Font::ALL {
        ui.invoke_item_clicked(font.index() as i32);
        assert_eq!(ui.get_ui_font_family(), font.family());
        assert_eq!(
            Settings::load(&directory.path().join("settings.json"))
                .unwrap()
                .font,
            font
        );
        assert_eq!(
            controller.borrow().state.tracks[1].title,
            track_before.title
        );
        assert_eq!(
            controller.borrow().state.tracks[1].lyrics,
            track_before.lyrics
        );
    }
    tap(&ui, Key::Escape);
    ui.invoke_item_clicked(font_row as i32);
    assert_eq!(ui.get_selected(), Font::JetBrainsMono.index() as i32);
    let reopened = Controller::new(directory.path().to_path_buf());
    assert_eq!(reopened.state.font, Font::JetBrainsMono);

    ui.invoke_command("now_playing".into());
    ui.set_keyboard_navigation(false);
    ui.set_notice("".into());
    ui.set_has_track(true);
    ui.set_track_title("Font preview".into());
    ui.set_overlay_visible(true);
    ui.set_lyrics_visible(true);
    ui.set_romaji_visible(false);
    ui.set_active_lyric(0);
    ui.set_lyric_scroll_y(0.0);
    ui.set_lyric_lines(slint::ModelRc::new(slint::VecModel::from(
        [
            "s e a o n · Ss seas skies",
            "静かな夜に · 安静的夜晚",
            "Some stars shine softly",
        ]
        .map(slint::SharedString::from)
        .to_vec(),
    )));
    let mut font_pixels = Vec::new();
    for (name, font) in [
        ("classic", Font::Classic),
        ("maple", Font::MapleMono),
        ("jetbrains", Font::JetBrainsMono),
    ] {
        ui.set_ui_font_family(font.family().into());
        snapshot(&ui, &format!("lyrics-font-{name}"));
        let rendered = ui.window().take_snapshot().unwrap();
        let point = display_point(&ui, 10.0, 180.0);
        let pixels_per_logical = ui.window().scale_factor();
        let x = (point.x * pixels_per_logical).round() as usize;
        let y = (point.y * pixels_per_logical).round() as usize;
        let scale = ui.get_display_scale() * pixels_per_logical;
        let width = (220.0 * scale).round() as usize;
        let height = (104.0 * scale).round() as usize;
        let bytes = rendered.as_bytes();
        let stride = rendered.width() as usize * 4;
        font_pixels.push(
            (y..y + height)
                .flat_map(|row| {
                    bytes[row * stride + x * 4..row * stride + (x + width) * 4]
                        .iter()
                        .copied()
                })
                .collect::<Vec<_>>(),
        );
    }
    assert_ne!(
        font_pixels[0], font_pixels[1],
        "bundled Maple must change rendered glyphs"
    );
    assert_ne!(
        font_pixels[1], font_pixels[2],
        "bundled fonts must render distinct glyphs"
    );

    // The persisted native window level is independent of playback and display mode.
    assert!(ui.get_keep_on_top());
    ui.invoke_command("settings".into());
    ui.invoke_item_clicked(0);
    for expected in [false, true] {
        let size = ui.window().size();
        let always_on_top = controller
            .borrow()
            .state
            .rows()
            .iter()
            .position(|row| {
                matches!(
                    row.target,
                    nano_music_player::app::state::Target::Action(Action::ToggleAlwaysOnTop)
                )
            })
            .unwrap();
        ui.invoke_item_clicked(always_on_top as i32);
        assert_eq!(ui.get_keep_on_top(), expected);
        assert_eq!(ui.window().size(), size);
        assert_eq!(
            Settings::load(&directory.path().join("settings.json"))
                .unwrap()
                .always_on_top,
            expected
        );
        assert_eq!(
            Controller::new(directory.path().to_path_buf())
                .state
                .always_on_top,
            expected
        );
        for mode in [Action::ScreenMode, Action::DeviceMode] {
            controller.borrow_mut().execute(&ui, mode);
            assert_eq!(ui.get_keep_on_top(), expected);
        }
    }

    // Real transparent corners, rather than a rounded shell over an opaque window.
    ui.invoke_command("home".into());
    ui.set_notice("".into());
    for zoom in [0.75, 1.25, 2.0] {
        controller.borrow_mut().state.zoom = zoom;
        controller.borrow_mut().execute(&ui, Action::DeviceMode);
        snapshot(&ui, &format!("rounded-device-{zoom}"));
        let rendered = ui.window().take_snapshot().unwrap();
        let size = ui.window().size().to_logical(ui.window().scale_factor());
        let scale = ui.get_display_scale();
        let width = 296.0 * scale;
        let height = 296.0 * 76.5 / 39.6 * scale;
        let left = (size.width - width) / 2.0;
        let top = size.height - ui.get_content_height() + (ui.get_content_height() - height) / 2.0;
        let alpha = |x: f32, y: f32| {
            let x = (x * ui.window().scale_factor()).round() as usize;
            let y = (y * ui.window().scale_factor()).round() as usize;
            rendered.as_bytes()[(y * rendered.width() as usize + x) * 4 + 3]
        };
        for (x_sign, y_sign) in [(false, false), (true, false), (false, true), (true, true)] {
            let point = |inset: f32| {
                (
                    left + if x_sign {
                        width - inset * scale
                    } else {
                        inset * scale
                    },
                    top + if y_sign {
                        height - inset * scale
                    } else {
                        inset * scale
                    },
                )
            };
            let (x, y) = point(1.0);
            assert_eq!(
                alpha(x, y),
                0,
                "exterior corner must be transparent at {zoom}"
            );
            let (x, y) = point(20.0);
            assert_eq!(
                alpha(x, y),
                255,
                "shell inside the curve must remain opaque"
            );
        }
        assert_eq!(alpha(left + width / 2.0, top + scale * 2.0), 255);
        controller.borrow_mut().execute(&ui, Action::ScreenMode);
        let rendered = ui.window().take_snapshot().unwrap();
        assert!(
            rendered
                .as_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255),
            "Screen Mode remains opaque after switching from Device Mode"
        );
    }
    ui.hide().unwrap();
}
