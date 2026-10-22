#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        navigation::{Filter, Screen},
        state::Target,
    },
    library::{Track, lyrics::Lyrics, romaji::Romaji},
    ui::AppWindow,
};
use slint::{
    ComponentHandle,
    platform::{Key, WindowEvent},
};
use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};

fn tick(milliseconds: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(milliseconds));
}

fn key(ui: &AppWindow, text: impl Into<slint::SharedString>) {
    let text = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
    tick(0);
}

fn home_key(ui: &AppWindow) {
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Shift.into(),
    });
    key(ui, "H");
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Shift.into(),
    });
}

fn event(text: &str, repeat: bool) -> slint::language::KeyEvent {
    let mut event = slint::language::KeyEvent::default();
    event.text = text.into();
    event.repeat = repeat;
    event
}

fn song(directory: &Path, index: usize) -> Track {
    let path = directory.join(format!("{index}.mp3"));
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quiet.mp3"),
        &path,
    )
    .unwrap();
    Track {
        path,
        title: if index == 0 {
            "A Playing song".into()
        } else {
            "B Selected song — 遥远的演奏会 🎵 — Original extended studio recording".into()
        },
        artist: "Fixture Artist".into(),
        album: "Fixture Album".into(),
        genre: String::new(),
        number: index as u32 + 1,
        duration: 60.0,
        artwork: vec![],
        lyrics: Lyrics::parse("[00:01]君の声\n[00:02]Hello\n[00:03]空へ"),
    }
}

fn open(ui: &AppWindow, controller: &Rc<RefCell<Controller>>, screen: Screen, selected: usize) {
    let mut controller = controller.borrow_mut();
    controller.state.navigation.open(screen);
    controller.state.navigation.select(selected);
    controller.initialize(ui);
    ui.invoke_focus_screen();
}

#[test]
fn information_shortcuts_and_romaji_follow_controller_selection_and_modal_priority() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    Settings {
        device_mode: false,
        smooth_slides: false,
        romaji_visible: false,
        ..Default::default()
    }
    .save(&directory.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    ui.invoke_focus_screen();

    // Empty libraries still expose real device information; D has no fake song.
    key(&ui, "d");
    assert!(!ui.get_track_details_open());
    assert_eq!(controller.borrow().state.notice, "Choose a song first");
    key(&ui, "b");
    assert!(ui.get_native_status_open());
    key(&ui, Key::Escape);
    assert!(!ui.get_native_status_open());

    controller
        .borrow_mut()
        .state
        .merge_library(vec![song(directory.path(), 0), song(directory.path(), 1)]);
    controller.borrow_mut().state.current = Some(0);
    open(&ui, &controller, Screen::Songs(Filter::All), 2);
    let song_screen = controller
        .borrow()
        .state
        .navigation
        .current()
        .screen
        .clone();
    let selected_title = controller.borrow().state.tracks[1].title.clone();
    key(&ui, "d");
    assert!(ui.get_track_details_open());
    assert_eq!(ui.get_details_title(), selected_title);
    assert_eq!(ui.get_details_artist(), "Fixture Artist");
    let _ = ui.window().take_snapshot().unwrap();
    ui.invoke_reset_details_scroll();
    tick(790);
    assert_eq!(ui.get_details_title_offset(), 0.0);
    tick(600);
    assert!(ui.get_details_title_offset() < 0.0);
    assert_eq!(ui.get_details_artist_offset(), 0.0);
    let selected = controller.borrow().state.navigation.selected();
    key(&ui, "j");
    assert_eq!(controller.borrow().state.navigation.selected(), selected);
    key(&ui, Key::Backspace);
    assert!(!ui.get_track_details_open());
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        song_screen
    );
    assert_eq!(controller.borrow().state.navigation.selected(), selected);

    // B and D replace one another, and holding either key never toggles twice.
    key(&ui, "b");
    assert!(ui.get_native_status_open());
    assert!(ui.invoke_key_event(event("b", true), false));
    assert!(ui.get_native_status_open());
    key(&ui, "d");
    assert!(!ui.get_native_status_open());
    assert!(ui.get_track_details_open());
    assert!(ui.invoke_key_event(event("d", true), false));
    assert!(ui.get_track_details_open());
    key(&ui, "b");
    assert!(ui.get_native_status_open());
    assert!(!ui.get_track_details_open());
    key(&ui, "b");
    assert!(!ui.get_native_status_open());

    // Details pin the displayed song even when the current playback changes.
    open(&ui, &controller, Screen::NowPlaying, 0);
    controller.borrow_mut().state.current = Some(0);
    key(&ui, "d");
    assert_eq!(ui.get_details_title(), "A Playing song");
    controller.borrow_mut().state.current = Some(1);
    controller.borrow_mut().execute(&ui, Action::VolumeUp);
    assert_eq!(ui.get_details_title(), "A Playing song");
    key(&ui, Key::Escape);
    assert!(!ui.get_track_details_open());
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::NowPlaying
    );
    key(&ui, "d");
    home_key(&ui);
    assert!(!ui.get_track_details_open());
    assert!(!ui.get_native_status_open());
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::Home
    );

    open(&ui, &controller, Screen::Songs(Filter::All), 1);
    for text in ["b", "d"] {
        assert!(!ui.invoke_key_event(event(text, false), true));
        assert!(!ui.get_native_status_open());
        assert!(!ui.get_track_details_open());
    }
    controller.borrow_mut().execute(&ui, Action::ShowShortcuts);
    for text in ["b", "d"] {
        key(&ui, text);
        assert!(ui.get_reference_open());
        assert!(!ui.get_native_status_open());
        assert!(!ui.get_track_details_open());
    }
    key(&ui, Key::Escape);
    assert!(!ui.get_reference_open());
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        song_screen
    );

    // Real editor keystrokes remain text, rather than opening information panels.
    controller
        .borrow_mut()
        .execute(&ui, Action::ConfigureDeepSeek);
    ui.invoke_focus_screen();
    key(&ui, "b");
    key(&ui, "d");
    assert_eq!(ui.get_deepseek_api_key(), "bd");
    assert!(ui.get_deepseek_settings_open());
    assert!(!ui.get_native_status_open());
    assert!(!ui.get_track_details_open());
    key(&ui, Key::Escape);
    assert!(!ui.get_deepseek_settings_open());

    controller.borrow_mut().execute(&ui, Action::NewPlaylist);
    assert!(ui.get_playlist_editor_open());
    ui.invoke_focus_screen();
    key(&ui, "b");
    key(&ui, "d");
    assert_eq!(ui.get_playlist_name(), "bd");
    assert!(!ui.get_native_status_open());
    assert!(!ui.get_track_details_open());
    key(&ui, Key::Escape);
    assert!(!ui.get_playlist_editor_open());

    ui.invoke_open_genius_preview();
    assert!(ui.get_genius_preview_open());
    for text in ["b", "d"] {
        key(&ui, text);
        assert!(ui.get_genius_preview_open());
        assert!(!ui.get_native_status_open());
        assert!(!ui.get_track_details_open());
    }
    key(&ui, Key::Escape);
    assert!(!ui.get_genius_preview_open());

    // Recording a conflicting D chord keeps ownership in the shortcut editor.
    open(&ui, &controller, Screen::Shortcuts, 0);
    let row = controller
        .borrow()
        .state
        .rows()
        .iter()
        .position(|row| matches!(row.target, Target::Shortcut(Action::ToggleMute)))
        .unwrap();
    controller.borrow_mut().state.navigation.select(row);
    controller.borrow_mut().execute(&ui, Action::Activate);
    assert!(ui.get_shortcut_editor_open());
    controller.borrow_mut().shortcut_command(&ui, "add");
    key(&ui, "d");
    assert!(ui.get_shortcut_editor_open());
    assert!(ui.get_shortcut_recording());
    assert!(!ui.get_track_details_open());
    assert!(!ui.get_native_status_open());
    key(&ui, Key::Escape);
    key(&ui, Key::Escape);
    assert!(!ui.get_shortcut_editor_open());

    // A deterministic local validation error proves R requests missing/stale
    // annotations, without relying on credentials or sending provider traffic.
    std::fs::write(
        directory.path().join("deepseek.json"),
        r#"{"model":"invalid/model","api_key":""}"#,
    )
    .unwrap();
    controller.borrow_mut().state.current = Some(0);
    open(&ui, &controller, Screen::NowPlaying, 0);
    let original = controller.borrow().state.tracks[0].lyrics.clone();
    key(&ui, "r");
    assert_eq!(
        controller.borrow().state.notice,
        "Invalid DeepSeek model name"
    );
    assert!(!controller.borrow().state.romaji_visible);
    assert!(controller.borrow().state.romaji_generating.is_none());
    let path = controller.borrow().state.tracks[0].path.clone();
    let cached = Romaji::new(
        original.clone(),
        vec!["kimi no koe".into(), "".into(), "sora e".into()],
        "fixture".into(),
    )
    .unwrap();
    controller
        .borrow_mut()
        .state
        .romaji
        .insert(path.clone(), cached);
    key(&ui, "r");
    assert!(controller.borrow().state.romaji_visible);
    key(&ui, "r");
    assert!(!controller.borrow().state.romaji_visible);
    assert!(controller.borrow().state.romaji_generating.is_none());
    assert_eq!(controller.borrow().state.tracks[0].lyrics, original);
    assert!(
        !Settings::load(&directory.path().join("settings.json"))
            .unwrap()
            .romaji_visible
    );

    controller.borrow_mut().state.tracks[0].lyrics = Lyrics::parse("[00:01]新しい歌詞");
    key(&ui, "r");
    assert_eq!(
        controller.borrow().state.notice,
        "Invalid DeepSeek model name"
    );
    assert!(!controller.borrow().state.romaji_visible);
    controller.borrow_mut().state.romaji_generating = Some(directory.path().join("other.mp3"));
    key(&ui, "r");
    assert!(
        !controller.borrow().state.romaji_visible,
        "a different song's request must not enable this song"
    );
    controller.borrow_mut().state.romaji_generating = None;

    // Settings' explicit preference toggle never tries to generate annotations.
    open(&ui, &controller, Screen::Settings, 0);
    controller.borrow_mut().state.notice = "settings toggle".into();
    controller.borrow_mut().execute(&ui, Action::ToggleRomaji);
    assert!(controller.borrow().state.romaji_visible);
    assert_eq!(controller.borrow().state.notice, "settings toggle");
    assert!(controller.borrow().state.romaji_generating.is_none());
    assert_eq!(
        controller.borrow().state.tracks[0].lyrics,
        Lyrics::parse("[00:01]新しい歌詞")
    );
    ui.hide().unwrap();
}
