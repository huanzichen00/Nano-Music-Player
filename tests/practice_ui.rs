#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use nano_music_player::{
    app::i18n::{Language, translate},
    ui::{AppWindow, I18n, PracticeMarkerRow},
};
use slint::{
    ComponentHandle, LogicalPosition, ModelRc, VecModel,
    platform::{Key, PointerEventButton, WindowEvent},
};
use std::{cell::RefCell, rc::Rc};

fn point(ui: &AppWindow, x: f32, y: f32) -> LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        size.height - ui.get_content_height()
            + (ui.get_content_height() - body * scale) / 2.0
            + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    )
}

fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = point(ui, x, y);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(120));
    slint::platform::update_timers_and_animations();
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn snapshot(ui: &AppWindow, name: &str) {
    slint::platform::update_timers_and_animations();
    let frame = ui.window().take_snapshot().unwrap();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa/practice");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        frame.as_bytes(),
        frame.width(),
        frame.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

#[test]
fn hidden_practice_updates_preserve_the_main_keyboard_focus() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();
    let received = Rc::new(RefCell::new(Vec::new()));
    let keys = received.clone();
    ui.on_key_event(move |event, _| {
        keys.borrow_mut().push(event.text.to_string());
        true
    });
    ui.show().unwrap();
    ui.invoke_focus_screen();
    let key = |text: &str| {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: text.into() });
    };
    key("k");
    assert_eq!(&*received.borrow(), &["k"]);

    // Track publication updates practice metadata even when the user has never
    // opened the panel. Its hidden editor must not capture the next shortcut.
    ui.set_practice_title("First song".into());
    ui.set_practice_title("Second song".into());
    slint::platform::update_timers_and_animations();
    key("5");
    assert_eq!(&*received.borrow(), &["k", "5"]);

    ui.invoke_close_practice_editor();
    ui.invoke_dismiss_practice_editor();
    key("r");
    assert_eq!(&*received.borrow(), &["k", "5", "r"]);

    ui.set_practice_open(true);
    ui.set_practice_open(false);
    slint::platform::update_timers_and_animations();
    ui.set_practice_title("Third song".into());
    slint::platform::update_timers_and_animations();
    key("d");
    assert_eq!(&*received.borrow(), &["k", "5", "r", "d"]);
}

#[test]
fn desktop_practice_panel_owns_pointer_input_and_edits_points_in_both_modes() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    ui.global::<I18n>()
        .on_translate(|language, key| translate(Language::ALL[language as usize], &key).into());
    ui.set_screen(2);
    ui.set_has_track(true);
    ui.set_track_title("Practice fixture".into());
    ui.set_practice_has_track(true);
    ui.set_practice_enabled(true);
    ui.set_practice_title("Practice fixture".into());
    ui.set_practice_position("00:18.500".into());
    ui.set_practice_markers(ModelRc::new(VecModel::from(vec![PracticeMarkerRow {
        id: "1".into(),
        name: "Chorus".into(),
        time: "01:02.000".into(),
        progress: 0.3,
    }])));
    let commands = Rc::new(RefCell::new(Vec::<(String, String, String)>::new()));
    let received = commands.clone();
    let weak = ui.as_weak();
    ui.on_practice_command(move |command, id, payload| {
        received
            .borrow_mut()
            .push((command.to_string(), id.to_string(), payload.to_string()));
        let ui = weak.upgrade().unwrap();
        match command.as_str() {
            "open" => ui.set_practice_open(true),
            "close" => ui.set_practice_open(false),
            "delay" => ui.set_practice_delay(payload.parse().unwrap()),
            "lead_in" => ui.set_practice_lead_in(payload.parse().unwrap()),
            _ => {}
        }
    });
    let underlying = Rc::new(RefCell::new(Vec::new()));
    let activated = underlying.clone();
    ui.on_item_clicked(move |index| activated.borrow_mut().push(index));
    for device in [false, true] {
        ui.set_device_mode(device);
        ui.window().set_size(slint::LogicalSize::new(
            if device { 296.0 } else { 240.0 },
            if device { 572.0 } else { 432.0 },
        ));
        ui.show().unwrap();
        snapshot(&ui, &format!("player-{device}"));
        click(&ui, 212.0, 34.0);
        assert!(
            ui.get_practice_open(),
            "desktop player has a practice entry"
        );
        snapshot(&ui, &format!("panel-en-{device}"));
        click(&ui, 222.0, 12.0);
        assert!(
            !ui.get_native_status_open(),
            "practice also shields the status bar"
        );
        click(&ui, 210.0, 140.0);
        assert_eq!(commands.borrow().last().unwrap().0, "delay");
        click(&ui, 210.0, 170.0);
        assert_eq!(commands.borrow().last().unwrap().0, "lead_in");
        click(&ui, 45.0, 261.0);
        assert_eq!(commands.borrow().last().unwrap().0, "start");
        click(&ui, 193.0, 261.0);
        assert_eq!(commands.borrow().last().unwrap().0, "add");
        click(&ui, 115.0, 333.0);
        assert_eq!(commands.borrow().last().unwrap().0, "jump");
        assert_eq!(commands.borrow().last().unwrap().1, "1");
        click(&ui, 50.0, 361.0);
        assert_eq!(commands.borrow().last().unwrap().0, "locate");
        click(&ui, 127.0, 361.0);
        assert!(ui.get_practice_editing());
        assert!(ui.get_practice_editor_focused());
        snapshot(&ui, &format!("edit-{device}"));
        click(&ui, 62.0, 279.0);
        assert_eq!(
            commands.borrow().last().unwrap(),
            &("save".into(), "1".into(), "01:02.000\nChorus".into())
        );
        assert!(!ui.get_practice_editing());
        click(&ui, 193.0, 361.0);
        assert_eq!(commands.borrow().last().unwrap().0, "delete");
        click(&ui, 192.0, 298.0);
        assert!(ui.get_practice_editing());
        ui.set_practice_error("Invalid time".into());
        click(&ui, 62.0, 279.0);
        assert!(
            ui.get_practice_editing(),
            "validation errors retain the draft"
        );
        ui.set_practice_error("".into());
        click(&ui, 62.0, 279.0);
        assert_eq!(
            commands.borrow().last().unwrap(),
            &("save".into(), "".into(), "00:18.500\n".into())
        );
        assert!(!ui.get_practice_editing());
        assert!(
            underlying.borrow().is_empty(),
            "panel clicks must not activate player controls"
        );
        ui.set_practice_pending(true);
        ui.set_practice_countdown(3);
        for language in [1, 2] {
            ui.global::<I18n>().set_language(language);
            snapshot(&ui, &format!("countdown-{language}-{device}"));
        }
        click(&ui, 197.0, 215.0);
        assert_eq!(commands.borrow().last().unwrap().0, "cancel");
        ui.set_practice_pending(false);
        ui.set_practice_countdown(0);
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Escape.into(),
        });
        assert!(!ui.get_practice_open());
        ui.set_practice_pending(true);
        ui.set_practice_countdown(3);
        snapshot(&ui, &format!("player-countdown-{device}"));
        click(&ui, 120.0, 202.0);
        assert_eq!(commands.borrow().last().unwrap().0, "cancel");
        assert!(underlying.borrow().is_empty());
        // Preparation also overlays lists without navigating to a practice page.
        ui.set_screen(1);
        for count in [3, 2, 1] {
            ui.set_practice_countdown(count);
            snapshot(&ui, &format!("list-countdown-{count}-{device}"));
            assert_eq!(ui.get_screen(), 1);
            assert!(!ui.get_practice_open());
        }
        click(&ui, 120.0, 216.0);
        assert_eq!(commands.borrow().last().unwrap().0, "cancel");
        assert!(underlying.borrow().is_empty());
        ui.set_practice_pending(false);
        ui.set_practice_countdown(0);
        ui.set_screen(2);
        ui.global::<I18n>().set_language(0);
    }
}
