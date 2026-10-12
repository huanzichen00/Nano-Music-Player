use nano_music_player::{
    app::{
        action::Action,
        controller::{Controller, connect},
    },
    library::{Track, lyrics::Lyrics},
    ui::AppWindow,
};
use slint::{
    ComponentHandle,
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
    let body_height = if ui.get_device_mode() { 600.0 } else { 432.0 };
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
    scroll(&ui, 60.0, 80.0, -80.0, 0.0);
    assert_eq!(
        ui.get_home_page(),
        1,
        "horizontal scroll over an icon changes Home page"
    );
    scroll(&ui, 60.0, 80.0, 80.0, 0.0);
    assert_eq!(ui.get_home_page(), 0);
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
    for _ in 0..4 {
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
    assert_eq!(ui.get_selected(), 4);

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
    tap(&ui, Key::Return);
    assert_eq!(ui.get_screen(), 2);
    assert_eq!(ui.get_track_title(), "Test Song 01");
    snapshot(&ui, "now-playing");
    click(&ui, 120.0, 200.0);
    assert!(ui.get_lyrics_visible());
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
    click(&ui, 120.0, 200.0);
    assert!(
        !ui.get_lyrics_visible(),
        "tap lyrics to return to artwork without an extra button"
    );
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
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    assert!(
        (size.width / 296.0 - ui.get_content_height() / 600.0).abs() < 0.01,
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
    tap(&ui, "l");
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
    ui.invoke_shortcut_command("clear".into());
    assert_eq!(ui.get_shortcut_keys(), "Unassigned");
    ui.invoke_shortcut_command("restore".into());
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
    assert!(ui.get_artwork().size().width > 0);
    assert!(ui.get_lyrics_visible());
    snapshot(&ui, "lyrics-with-artwork");
    click(&ui, 211.0, 160.0); // Album track list.
    assert_eq!(ui.get_screen_title(), "Test Album");
    tap(&ui, "h");
    assert_eq!(ui.get_screen_title(), "Now Playing");
    click(&ui, 90.0, 160.0); // Lyrics toolbar button.
    assert!(!ui.get_lyrics_visible());
    assert!(ui.get_overlay_visible());
    snapshot(&ui, "cover-controls");
    click(&ui, 120.0, 240.0);
    assert!(!ui.get_overlay_visible());
    snapshot(&ui, "cover-with-artwork");
    ui.hide().unwrap();
}
