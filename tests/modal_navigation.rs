use nano_music_player::{
    app::{
        action::Action,
        controller::{Controller, connect},
        navigation::Screen,
    },
    audio::PlaybackStatus,
    input::{Binding, Key, Modifiers},
    library::{Track, lyrics::Lyrics},
    ui::AppWindow,
};
use slint::{
    ComponentHandle, Model,
    platform::{Key as SlintKey, WindowEvent},
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

fn home_shortcut(ui: &AppWindow) {
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: SlintKey::Shift.into(),
    });
    tap(ui, "H");
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: SlintKey::Shift.into(),
    });
}

fn physical_home(ui: &AppWindow) {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let body_height = 296.0 * 76.5 / 39.6 * scale;
    let top = size.height - ui.get_content_height() + (ui.get_content_height() - body_height) / 2.0;
    let position = slint::LogicalPosition::new(size.width / 2.0, top + body_height - 58.0 * scale);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
}

#[test]
fn home_dismisses_overlays_genius_respects_bindings_and_playlist_drafts_survive() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    // The absent source path below tests Genius validation and modal navigation,
    // rather than watcher reconciliation.
    nano_music_player::app::config::Settings {
        monitor_deleted_files: false,
        ..Default::default()
    }
    .save(&directory.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    // Missing fixture files make Genius fail validation before any provider call.
    controller.borrow_mut().state.tracks = vec![Track {
        path: directory.path().join("song.wav"),
        title: "Seed".into(),
        artist: "Fixture".into(),
        album: "Album".into(),
        genre: "Test".into(),
        number: 1,
        duration: 60.0,
        artwork: vec![],
        lyrics: Lyrics::default(),
    }];
    {
        let mut c = controller.borrow_mut();
        c.state.current = Some(0);
        c.state.queue.set(vec![0], 0, false);
        c.state.status = PlaybackStatus::Paused;
        c.state.paused = true;
        c.state.position = 23.0;
        c.state.navigation.open(Screen::Music);
    }
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    ui.invoke_focus_screen();

    ui.invoke_open_genius_preview();
    tap(&ui, "l");
    assert_eq!(ui.get_genius_preview_stage(), 1);
    assert!(!ui.get_genius_busy());
    controller.borrow_mut().execute(&ui, Action::ShowShortcuts);
    tap(&ui, SlintKey::Escape);
    assert!(!ui.get_reference_open());
    assert!(
        ui.get_genius_preview_open(),
        "Escape closes the topmost reference first"
    );
    tap(&ui, "h");
    assert!(!ui.get_genius_preview_open());
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::Music
    );
    assert_eq!(controller.borrow().state.position, 23.0);

    ui.invoke_open_genius_preview();
    ui.set_genius_busy(true);
    tap(&ui, "l");
    assert_eq!(
        ui.get_genius_preview_stage(),
        0,
        "busy Create cannot be activated twice"
    );
    home_shortcut(&ui);
    assert_eq!(ui.get_screen(), 0);
    assert!(!ui.get_genius_preview_open());
    assert!(!ui.get_genius_busy());

    controller.borrow_mut().execute(&ui, Action::ShowShortcuts);
    assert!(ui.get_reference_open());
    home_shortcut(&ui);
    assert!(!ui.get_reference_open());
    assert_eq!(ui.get_screen(), 0);

    controller.borrow_mut().execute(&ui, Action::Shortcuts);
    controller.borrow_mut().execute(&ui, Action::Activate);
    assert!(ui.get_shortcut_editor_open());
    home_shortcut(&ui);
    assert!(!ui.get_shortcut_editor_open());
    assert_eq!(ui.get_screen(), 0);

    controller.borrow_mut().execute(&ui, Action::Shortcuts);
    controller.borrow_mut().execute(&ui, Action::Activate);
    ui.invoke_shortcut_command("replace".into());
    home_shortcut(&ui);
    assert!(
        ui.get_shortcut_editor_open(),
        "shortcut recording retains key capture precedence"
    );
    physical_home(&ui);
    assert!(!ui.get_shortcut_editor_open());
    assert_eq!(ui.get_screen(), 0);

    ui.invoke_open_genius_preview();
    controller
        .borrow_mut()
        .execute(&ui, Action::ConfigureDeepSeek);
    assert!(ui.get_deepseek_settings_open());
    home_shortcut(&ui);
    assert!(
        ui.get_deepseek_settings_open(),
        "uppercase H in a text field stays text"
    );
    assert_eq!(ui.get_deepseek_api_key(), "H");
    controller.borrow_mut().execute(&ui, Action::ShowShortcuts);
    home_shortcut(&ui);
    assert!(!ui.get_reference_open());
    assert!(!ui.get_deepseek_settings_open());
    assert!(!ui.get_genius_preview_open());
    assert_eq!(ui.get_screen(), 0);
    physical_home(&ui);
    assert!(!ui.get_deepseek_settings_open());
    assert!(!ui.get_genius_preview_open());
    assert!(ui.get_deepseek_api_key().is_empty());
    assert!(!directory.path().join("deepseek.json").exists());

    // Modal navigation uses the customized semantic bindings, not hardcoded letters.
    for (action, key) in [
        (Action::Home, Key::Function(6)),
        (Action::NavigateLeft, Key::Letter('b')),
        (Action::NavigateRight, Key::Letter('o')),
    ] {
        let mut c = controller.borrow_mut();
        c.dispatcher.keymap = c
            .dispatcher
            .keymap
            .edit(
                action,
                Some(Binding {
                    action,
                    key,
                    modifiers: Modifiers::default(),
                    optional_shift: false,
                }),
                true,
            )
            .unwrap();
    }
    ui.invoke_open_genius_preview();
    tap(&ui, "h");
    assert!(ui.get_genius_preview_open());
    tap(&ui, "l");
    assert_eq!(ui.get_genius_preview_stage(), 0);
    tap(&ui, "o");
    assert_eq!(ui.get_genius_preview_stage(), 1);
    tap(&ui, "b");
    assert!(!ui.get_genius_preview_open());
    ui.invoke_open_genius_preview();
    tap(&ui, SlintKey::F6);
    assert!(!ui.get_genius_preview_open());
    assert_eq!(ui.get_screen(), 0);

    let path = directory.path().join("song.wav");
    std::fs::write(&path, include_bytes!("fixtures/quiet.wav")).unwrap();
    controller.borrow_mut().execute(&ui, Action::NewPlaylist);
    tap(&ui, "Draft");
    home_shortcut(&ui); // Now unbound, and the name field still receives uppercase H.
    assert_eq!(ui.get_playlist_name(), "DraftH");
    ui.invoke_playlist_action("toggle".into(), path.to_string_lossy().into_owned().into());
    assert_eq!(ui.get_playlist_draft().row_count(), 1);
    physical_home(&ui);
    assert_eq!(ui.get_screen(), 0);
    assert!(!ui.get_playlist_editor_open());
    controller.borrow_mut().execute(&ui, Action::NewPlaylist);
    assert_eq!(ui.get_playlist_name(), "DraftH");
    assert_eq!(ui.get_playlist_draft().row_count(), 1);

    ui.invoke_playlist_action("create".into(), "".into());
    assert!(ui.get_playlist_saving());
    tap(&ui, SlintKey::F6);
    assert_eq!(
        ui.get_screen(),
        0,
        "Home shortcuts work while Save is pending"
    );
    assert!(!ui.get_playlist_editor_open());
    controller.borrow_mut().execute(&ui, Action::NewPlaylist);
    assert!(ui.get_playlist_editor_open());
    assert!(ui.get_playlist_saving());
    physical_home(&ui);
    assert_eq!(ui.get_screen(), 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while ui.get_playlist_saving() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
        controller.borrow_mut().poll(&ui);
    }
    assert_eq!(
        ui.get_screen(),
        0,
        "finishing an explicit save does not pull the user away from Home"
    );
    assert_eq!(controller.borrow().state.playlists.len(), 1);
    assert_eq!(controller.borrow().state.playlists[0].name, "DraftH");
    assert_eq!(controller.borrow().state.position, 23.0);
    assert_eq!(controller.borrow().state.queue.order, vec![0]);
    ui.hide().unwrap();
}
