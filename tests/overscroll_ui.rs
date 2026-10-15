//! Synthetic Slint events and software frames; these do not certify a native trackpad.
use nano_music_player::ui::{AppWindow, I18n, LibraryRow};
use slint::{
    ComponentHandle, ModelRc, VecModel,
    platform::{PointerEventButton, WindowEvent},
};
use std::{cell::Cell, rc::Rc, time::Duration};

fn tick(ms: u64) {
    for _ in 0..ms.div_ceil(16) {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    }
}

fn point(ui: &AppWindow, x: f32, y: f32) -> slint::LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let s = ui.get_display_scale();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    slint::LogicalPosition::new(
        (size.width - 240.0 * s) / 2.0 + x * s,
        size.height - ui.get_content_height()
            + (ui.get_content_height() - body * s) / 2.0
            + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * s,
    )
}

fn wheel(ui: &AppWindow, dx: f32, dy: f32) {
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: point(ui, 100.0, 190.0),
        delta_x: dx * ui.get_display_scale(),
        delta_y: dy * ui.get_display_scale(),
    });
    tick(16);
}

fn press(ui: &AppWindow, x: f32, y: f32) {
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: point(ui, x, y),
        button: PointerEventButton::Left,
    });
}

fn release(ui: &AppWindow, x: f32, y: f32) {
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: point(ui, x, y),
        button: PointerEventButton::Left,
    });
    tick(16);
}

fn move_to(ui: &AppWindow, x: f32, y: f32) {
    tick(16);
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: point(ui, x, y),
    });
    tick(16);
}

fn frame(ui: &AppWindow, name: &str) -> slint::SharedPixelBuffer<slint::Rgba8Pixel> {
    let frame = ui.window().take_snapshot().unwrap();
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
    frame
}

fn rows(ui: &AppWindow, count: usize) {
    ui.set_rows(ModelRc::new(VecModel::from(
        (0..count)
            .map(|i| LibraryRow {
                title: format!("Elastic song {i:02}").into(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));
    tick(16);
}

#[test]
fn lists_and_home_follow_live_input_and_cancel_without_stale_motion() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    ui.global::<I18n>().on_translate(|_, text| text);
    ui.set_device_mode(false);
    ui.window().set_size(slint::LogicalSize::new(240.0, 432.0));
    ui.set_screen(1);
    ui.set_screen_title("Songs".into());
    ui.set_clock_text("9:41 AM".into());
    let activations = Rc::new(Cell::new(0));
    let count = activations.clone();
    ui.on_item_clicked(move |_| count.set(count.get() + 1));
    let swipes = Rc::new(Cell::new(0));
    let count = swipes.clone();
    let weak = ui.as_weak();
    ui.on_home_swipe(move |next| {
        count.set(count.get() + 1);
        weak.upgrade().unwrap().set_home_page(i32::from(next));
    });
    ui.show().unwrap();
    rows(&ui, 20);

    let before = frame(&ui, "overscroll-list-rest");
    press(&ui, 100.0, 150.0);
    move_to(&ui, 100.0, 160.0);
    move_to(&ui, 100.0, 170.0);
    move_to(&ui, 100.0, 180.0);
    let first = ui.get_list_overshoot();
    assert!(
        first > 0.0 && first < 30.0,
        "content follows held input: overshoot={first}, y={}, scale={}",
        ui.get_list_scroll_y(),
        ui.get_display_scale()
    );
    move_to(&ui, 100.0, 210.0);
    let second = ui.get_list_overshoot();
    assert!(second > first && second - first < first);
    assert_eq!(ui.get_list_scroll_y(), 0.0);
    tick(400);
    assert_eq!(ui.get_list_overshoot(), second, "no rebound before release");
    let pulled = frame(&ui, "overscroll-list-held");
    release(&ui, 100.0, 210.0);
    assert_ne!(before.as_bytes(), pulled.as_bytes());
    assert_eq!(activations.get(), 0, "drag must never activate a song");
    tick(64);
    let rebound = ui.get_list_overshoot();
    assert!(rebound > 0.0 && rebound < second);
    // Reuse exactly the same press coordinates: no stale previous drag position.
    press(&ui, 100.0, 150.0);
    move_to(&ui, 100.0, 138.0);
    assert!(ui.get_list_overshoot() < rebound);
    release(&ui, 100.0, 138.0);
    tick(2000);
    assert_eq!(ui.get_list_overshoot(), 0.0);
    assert!(ui.get_list_scroll_y() <= 0.0);

    // Middle scrolling remains linear; external keyboard/selection positioning
    // supersedes the fling and cannot revive a pending spring.
    ui.set_list_scroll_y(-200.0);
    tick(16);
    wheel(&ui, 0.0, -30.0);
    assert!((ui.get_list_scroll_y() + 200.0 + 30.0 * ui.get_display_scale()).abs() < 0.1);
    let minimum = -592.0 * ui.get_display_scale(); // 20*48 - 368 viewport.
    ui.set_list_scroll_y(minimum);
    tick(16);
    wheel(&ui, 0.0, -40.0);
    assert!(ui.get_list_overshoot() < 0.0);
    assert_eq!(ui.get_list_scroll_y(), minimum);
    let bottom = ui.get_list_overshoot();
    wheel(&ui, 0.0, 8.0);
    assert!(ui.get_list_overshoot() > bottom);
    tick(2000);
    assert_eq!(ui.get_list_overshoot(), 0.0);
    assert_eq!(ui.get_list_scroll_y(), minimum);

    // Both short and empty lists still resist and always restore origin.
    for count in [2, 0] {
        rows(&ui, count);
        assert_eq!(ui.get_list_scroll_y(), 0.0);
        press(&ui, 100.0, 150.0);
        move_to(&ui, 100.0, 180.0);
        assert!(ui.get_list_overshoot() > 0.0);
        release(&ui, 100.0, 180.0);
        tick(2000);
        for delta in [40.0, -120.0] {
            wheel(&ui, 0.0, delta);
            assert!(ui.get_list_overshoot() * delta > 0.0);
            assert_eq!(ui.get_list_scroll_y(), 0.0);
            tick(2000);
            assert_eq!(ui.get_list_overshoot(), 0.0);
        }
    }

    for smooth in [true, false] {
        ui.set_smooth_slides(smooth);
        wheel(&ui, 0.0, 50.0);
        assert!(ui.get_list_overshoot() > 0.0);
        // A semantic navigation can reuse the same presentation and row count.
        ui.set_screen_transition_generation(ui.get_screen_transition_generation() + 1);
        tick(300);
        assert_eq!(ui.get_list_overshoot(), 0.0);
        wheel(&ui, 0.0, 50.0);
        // A modal cancels the underlying animation and guards all input.
        ui.set_reference_open(true);
        tick(16);
        assert_eq!(ui.get_list_overshoot(), 0.0);
        ui.set_reference_open(false);
        tick(16);
        wheel(&ui, 0.0, 60.0);
        ui.set_previous_screen(1);
        ui.set_screen(0);
        ui.set_screen_transition_generation(ui.get_screen_transition_generation() + 1);
        tick(2000);
        assert_eq!(ui.get_list_overshoot(), 0.0);
        assert_eq!(ui.get_transition_offset(), 0.0);
        assert_eq!(ui.get_home_overshoot(), 0.0);

        // First/last page boundary gestures never wrap or fire navigation.
        for page in [0, 1] {
            ui.set_home_page(page);
            tick(300);
            let sign = if page == 0 { 1.0 } else { -1.0 };
            let accepted = swipes.get();
            press(&ui, 100.0, 150.0);
            move_to(&ui, 100.0 + 30.0 * sign, 150.0);
            assert!(ui.get_home_overshoot() * sign > 0.0);
            release(&ui, 100.0 + 30.0 * sign, 150.0);
            assert_eq!(swipes.get(), accepted);
            tick(2000);
            assert_eq!(ui.get_home_overshoot(), 0.0);
            for _ in 0..10 {
                wheel(&ui, sign * 20.0, 0.0);
            }
            assert!(ui.get_home_overshoot() * sign > 0.0);
            assert_eq!(ui.get_home_page(), page);
            assert_eq!(swipes.get(), accepted);
            let before_reverse = ui.get_home_overshoot().abs();
            wheel(&ui, -sign * 5.0, 0.0);
            assert!(ui.get_home_overshoot().abs() < before_reverse);
            assert!(
                ui.get_home_overshoot() * sign > 0.0,
                "reverse without snapping"
            );
            frame(&ui, &format!("overscroll-home-{page}-{smooth}"));
            tick(2000);
            assert_eq!(ui.get_home_overshoot(), 0.0);
        }
        wheel(&ui, -50.0, 0.0);
        ui.window().set_size(slint::LogicalSize::new(260.0, 452.0));
        tick(16);
        assert_eq!(ui.get_home_overshoot(), 0.0);
        ui.window().set_size(slint::LogicalSize::new(240.0, 432.0));
        tick(16);
        // Navigation during a Home rebound cannot replay into the next visit.
        wheel(&ui, -50.0, 0.0);
        ui.set_screen(1);
        tick(2000);
        assert_eq!(ui.get_home_overshoot(), 0.0);
    }

    // The same logical displacement scales consistently in both device modes.
    rows(&ui, 20);
    for device in [false, true] {
        ui.set_device_mode(device);
        let mut logical_overshoot: Option<f32> = None;
        for zoom in [1.0, 1.6] {
            let base = if device {
                (296.0, 296.0 * 76.5 / 39.6)
            } else {
                (240.0, 432.0)
            };
            ui.window()
                .set_size(slint::LogicalSize::new(base.0 * zoom, base.1 * zoom));
            tick(16);
            ui.set_list_scroll_y(0.0);
            tick(16);
            wheel(&ui, 0.0, 50.0);
            let offset = ui.get_list_overshoot() / ui.get_display_scale();
            if let Some(expected) = logical_overshoot {
                assert!((offset - expected).abs() < 0.5);
            }
            logical_overshoot = Some(offset);
            // A geometry change cancels the old animation in logical coordinates.
            ui.window().set_size(slint::LogicalSize::new(
                base.0 * zoom + 20.0,
                base.1 * zoom + 20.0,
            ));
            tick(16);
            assert_eq!(ui.get_list_overshoot(), 0.0);
            tick(2000);
            assert_eq!(ui.get_list_overshoot(), 0.0);
        }
    }
    ui.hide().unwrap();
}
