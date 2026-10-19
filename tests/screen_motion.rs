use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
    },
    library::Track,
    ui::AppWindow,
};
use slint::{
    ComponentHandle, Model,
    platform::{Key, PointerEventButton, WindowEvent},
};
use std::{cell::RefCell, rc::Rc, time::Duration};

fn tick(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn raw_key(ui: &AppWindow, text: impl Into<slint::SharedString>) {
    let text = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn key(ui: &AppWindow, text: impl Into<slint::SharedString>) {
    raw_key(ui, text);
    tick(0);
}

fn point(ui: &AppWindow, x: f32, y: f32) -> slint::LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let s = ui.get_display_scale();
    let content_height = ui.get_content_height();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    slint::LogicalPosition::new(
        (size.width - 240.0 * s) / 2.0 + x * s,
        size.height - content_height
            + (content_height - body * s) / 2.0
            + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * s,
    )
}

fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = point(ui, x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    tick(0);
}

fn wheel(ui: &AppWindow, delta_x: f32) {
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: point(ui, 60.0, 80.0),
        delta_x: delta_x * ui.get_display_scale(),
        delta_y: 0.0,
    });
    tick(0);
}

fn frame(ui: &AppWindow) -> slint::SharedPixelBuffer<slint::Rgba8Pixel> {
    // Freeze time-dependent chrome, including the three-second startup notice.
    // Its expiry is independent of screen transitions and the mock animation clock.
    ui.set_clock_text("9:41 AM".into());
    ui.set_notice("".into());
    // Invalidate the software renderer's cached frame as a native input event does.
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(0.0, 0.0),
    });
    ui.window().take_snapshot().unwrap()
}

fn save(frame: &slint::SharedPixelBuffer<slint::Rgba8Pixel>, name: &str) {
    let directory = std::env::temp_dir().join("nano-qa");
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

fn settled(ui: &AppWindow) {
    tick(300);
    assert_eq!(ui.get_transition_offset(), 0.0);
    assert_eq!(ui.get_previous_transition_offset(), 0.0);
    assert_eq!(ui.get_previous_rows().row_count(), 0);
    assert_eq!(ui.get_home_transition_offset(), 0.0);
}

fn slide(
    ui: &AppWindow,
    before: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    direction: f32,
    name: &str,
) {
    let initial = ui.get_transition_offset();
    assert!(
        initial * direction > 0.0,
        "{name}: starts from the requested side"
    );
    tick(35);
    let offset = ui.get_transition_offset();
    assert!(
        offset * direction > 0.0 && offset.abs() < initial.abs(),
        "{name}: progresses between frames"
    );
    assert!(ui.get_previous_transition_offset() * direction < 0.0);
    let middle = frame(ui);
    settled(ui);
    let after = frame(ui);
    save(&middle, &format!("{name}-middle"));
    save(&after, &format!("{name}-settled"));
    assert_ne!(
        middle.as_bytes(),
        before.as_bytes(),
        "{name}: source must move"
    );
    assert_ne!(
        middle.as_bytes(),
        after.as_bytes(),
        "{name}: actual intermediate composition must render"
    );
}

#[test]
fn repeated_library_slides_settle_and_leave_home_controls_and_pages_usable() {
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
        monitor_deleted_files: false,
        device_mode: false,
        ..Default::default()
    }
    .save(&directory.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    controller.borrow_mut().state.merge_library(
        (0..12)
            .map(|id| Track {
                path: directory.path().join(format!("{id}.wav")),
                title: format!("Song {id:02}"),
                artist: "Test Artist".into(),
                album: format!("Album {}", id / 3),
                genre: "Test".into(),
                number: id + 1,
                duration: 60.0,
                artwork: Vec::new(),
                lyrics: Default::default(),
            })
            .collect(),
    );
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    ui.invoke_focus_screen();
    tick(0);
    let home = frame(&ui);
    save(&home, "home-baseline");
    for cycle in 0..3 {
        let before = frame(&ui);
        click(&ui, 60.0, 80.0); // Music icon remains usable after returning Home.
        assert_eq!(ui.get_screen_title(), "Music");
        slide(&ui, &before, 1.0, &format!("screen-{cycle}-music"));
        for _ in 0..4 {
            key(&ui, "j");
        }
        let before = frame(&ui);
        key(&ui, "l");
        assert_eq!(ui.get_screen_title(), "Albums");
        slide(&ui, &before, 1.0, &format!("screen-{cycle}-albums"));
        let before = frame(&ui);
        click(&ui, 120.0, 90.0); // A settled list must re-enable row activation.
        assert_eq!(ui.get_screen_title(), "Album 0");
        slide(&ui, &before, 1.0, &format!("screen-{cycle}-album"));
        let before = frame(&ui);
        key(&ui, "h");
        assert_eq!(ui.get_screen_title(), "Albums");
        slide(&ui, &before, -1.0, &format!("screen-{cycle}-back"));
        ui.invoke_command("home".into());
        tick(0);
        settled(&ui);
        let returned_home = frame(&ui);
        save(&returned_home, &format!("home-return-{cycle}"));
        assert!(
            returned_home.as_bytes() == home.as_bytes(),
            "Home has no residual outgoing screen or offset"
        );
    }
    key(&ui, Key::Return);
    settled(&ui);
    for _ in 0..4 {
        key(&ui, "j");
    }
    key(&ui, "l");
    tick(210);
    // The old deadline can fire before the next frame processes Back's change handler.
    raw_key(&ui, "h");
    tick(30);
    assert_eq!(ui.get_screen_title(), "Music");
    assert_eq!(ui.get_previous_screen_title(), "Albums");
    assert!(
        ui.get_previous_rows().row_count() > 0,
        "an old completion cannot release the latest transition's outgoing rows"
    );
    tick(25);
    assert!(
        ui.get_transition_offset() < 0.0,
        "Back keeps moving past the old deadline"
    );
    save(&frame(&ui), "screen-interrupted-back-middle");
    settled(&ui);
    key(&ui, "l");
    tick(210);
    // Also cover commands which publish a generation without dispatching another input frame.
    controller.borrow_mut().execute(&ui, Action::Back);
    tick(30);
    assert!(ui.get_previous_rows().row_count() > 0);
    settled(&ui);
    ui.invoke_command("home".into());
    tick(0);
    settled(&ui);
    key(&ui, "l");
    key(&ui, "l");
    tick(200);
    assert_eq!(ui.get_home_page(), 1);
    key(&ui, "l"); // Settings is the second icon on page two.
    key(&ui, Key::Return);
    assert_eq!(ui.get_screen_title(), "Settings");
    settled(&ui);
    ui.invoke_command("home".into());
    tick(0);
    assert_eq!(ui.get_home_page(), 0);
    assert_eq!(
        ui.get_home_transition_offset(),
        0.0,
        "returning Home must position its icons immediately, without replaying hidden page motion"
    );
    tick(35);
    save(&frame(&ui), "screen-return-home-middle");
    wheel(&ui, -80.0);
    assert_eq!(
        ui.get_home_page(),
        0,
        "Home wheel input waits until the screen is settled"
    );
    settled(&ui);
    assert_eq!(frame(&ui).as_bytes(), home.as_bytes());
    save(&frame(&ui), "screen-return-home-settled");
    wheel(&ui, -10.0);
    assert_eq!(
        ui.get_home_page(),
        0,
        "ignored transition input leaves no accumulated gesture"
    );
    wheel(&ui, -10.0);
    assert_eq!(ui.get_home_page(), 1);
    tick(70);
    assert!(
        ui.get_home_transition_offset() > 0.0,
        "Home pages still animate after returning"
    );
    settled(&ui);
    key(&ui, "l");
    wheel(&ui, -80.0);
    assert_eq!(
        ui.get_selected(),
        1,
        "boundary momentum preserves Home selection"
    );

    // Turning motion off mid-flight must finish both hosts and release controls.
    key(&ui, Key::Return);
    tick(35);
    assert!(ui.get_transition_offset() > 0.0);
    key(&ui, Key::Return); // Appearance is the first Settings level.
    let toggle = ui
        .get_rows()
        .iter()
        .position(|row| row.title == "Page Animation")
        .unwrap();
    for _ in 0..toggle {
        key(&ui, "j");
    }
    key(&ui, Key::Return);
    assert!(!ui.get_smooth_slides());
    let immediate = frame(&ui);
    settled(&ui);
    assert_eq!(
        frame(&ui).as_bytes(),
        immediate.as_bytes(),
        "disabled motion has no delayed residual frame"
    );
    key(&ui, Key::Return); // Re-enable on the same selected setting.
    assert!(ui.get_smooth_slides());
    ui.invoke_command("home".into());
    tick(0);
    settled(&ui);
    let before = frame(&ui);
    click(&ui, 60.0, 80.0);
    slide(&ui, &before, 1.0, "screen-after-motion-toggle");
    ui.invoke_command("home".into());
    tick(0);
    settled(&ui);
    assert_eq!(frame(&ui).as_bytes(), home.as_bytes());
}
