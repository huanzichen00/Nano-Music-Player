#![cfg(not(any(target_os = "ios", feature = "mobile")))]
use nano_music_player::{
    app::i18n::{Language, translate},
    ui::{AppWindow, I18n},
};
use slint::{
    ComponentHandle, LogicalPosition,
    platform::{Key, PointerEventButton, WindowEvent},
};
use std::{cell::Cell, rc::Rc};

fn display_point(ui: &AppWindow, x: f32, y: f32) -> LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let height = ui.get_content_height();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        size.height - height
            + (height - body * scale) / 2.0
            + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    )
}

fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = display_point(ui, x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn assert_rounded_panel_corner(ui: &AppWindow) {
    let snapshot = ui.window().take_snapshot().unwrap();
    let brightness = |x, y| {
        let point = display_point(ui, x, y);
        let scale = ui.window().scale_factor();
        let x = (point.x * scale).round() as usize;
        let y = (point.y * scale).round() as usize;
        let offset = 4 * (y * snapshot.width() as usize + x);
        snapshot.as_bytes()[offset..offset + 3]
            .iter()
            .map(|value| u32::from(*value))
            .sum::<u32>()
    };
    assert!(
        brightness(9.0, 33.0) + 120 < brightness(30.0, 40.0),
        "the outer corner must reveal the backdrop instead of a square surface"
    );
}

fn frame(ui: &AppWindow, name: &str) -> Vec<u8> {
    let snapshot = ui.window().take_snapshot().unwrap();
    let directory = std::env::temp_dir().join("nano-native-status");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        snapshot.as_bytes(),
        snapshot.width(),
        snapshot.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
    snapshot.as_bytes().to_vec()
}

#[test]
fn battery_levels_and_status_panel_work_in_both_desktop_modes() {
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
    ui.set_clock_text("1:25 PM".into());
    ui.set_native_settings_available(true);
    ui.set_audio_output_name("AirPods Pro".into());
    ui.set_audio_output_headphones(true);
    ui.set_audio_output_earbuds(true);
    ui.set_audio_output_wireless(true);
    ui.set_headphone_left_level(64);
    ui.set_headphone_right_level(66);
    ui.set_headphone_case_level(82);
    ui.set_last_media_command("Play · 13:25:02".into());
    let settings_requests = Rc::new(Cell::new(0));
    let requested = Rc::clone(&settings_requests);
    ui.on_command(move |command| {
        if command == "bluetooth_settings" {
            requested.set(requested.get() + 1);
        }
    });
    for device in [false, true] {
        ui.set_device_mode(device);
        ui.window().set_size(slint::LogicalSize::new(
            if device { 296.0 } else { 240.0 },
            if device { 572.0 } else { 432.0 },
        ));
        ui.show().unwrap();
        ui.set_screen(0);
        ui.set_battery_level(-1);
        let unknown = frame(&ui, &format!("unknown-{device}"));
        ui.set_battery_level(17);
        let low = frame(&ui, &format!("low-{device}"));
        assert_ne!(unknown, low);
        ui.set_battery_level(80);
        ui.set_battery_charging(true);
        ui.set_battery_external_power(true);
        assert_ne!(low, frame(&ui, &format!("charging-{device}")));
        click(&ui, 222.0, 12.0);
        assert!(ui.get_native_status_open());
        assert_rounded_panel_corner(&ui);
        frame(&ui, &format!("panel-en-{device}"));
        ui.set_battery_level(100);
        ui.set_battery_charging(false);
        frame(&ui, &format!("panel-on-power-{device}"));
        ui.set_battery_level(80);
        ui.set_battery_charging(true);
        let previous_requests = settings_requests.get();
        click(&ui, 120.0, 371.0);
        assert_eq!(settings_requests.get(), previous_requests + 1);
        assert!(ui.get_native_status_open());
        ui.global::<I18n>().set_language(1);
        frame(&ui, &format!("panel-zh-{device}"));
        ui.global::<I18n>().set_language(2);
        frame(&ui, &format!("panel-ja-{device}"));
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Escape.into(),
        });
        assert!(!ui.get_native_status_open());
        ui.set_screen(1);
        frame(&ui, &format!("light-status-{device}"));
        click(&ui, 222.0, 12.0);
        assert!(ui.get_native_status_open());
        click(&ui, 120.0, 420.0);
        assert!(!ui.get_native_status_open());
        ui.global::<I18n>().set_language(0);
    }

    // A late/missing accessory reading must not fabricate percentages or a connection.
    ui.set_native_status_open(true);
    ui.set_headphone_left_level(0);
    ui.set_headphone_case_level(-1);
    frame(&ui, "panel-low-and-missing-case");
    ui.set_headphone_left_level(-1);
    ui.set_headphone_right_level(-1);
    frame(&ui, "panel-airpods-no-readings");
    ui.set_audio_output_earbuds(false);
    ui.set_audio_output_headphones(false);
    ui.set_audio_output_wireless(false);
    ui.set_audio_output_name("MacBook Speakers".into());
    frame(&ui, "panel-speakers");
    ui.set_audio_output_headphones(true);
    ui.set_audio_output_wireless(true);
    ui.set_headphone_single_level(58);
    ui.set_audio_output_name("Bluetooth Headphones".into());
    frame(&ui, "panel-single-battery");
    ui.set_audio_output_earbuds(true);
    ui.set_headphone_single_level(-1);
    ui.set_audio_output_name("梓辰的 AirPods Pro · 一个很长的设备名称".into());
    ui.global::<I18n>().set_language(1);
    frame(&ui, "panel-long-name");
    ui.set_audio_output_earbuds(false);
    ui.set_audio_output_headphones(false);
    ui.set_audio_output_wireless(false);
    ui.set_audio_output_name("".into());
    ui.set_battery_level(-1);
    frame(&ui, "panel-no-output");
    click(&ui, 207.0, 56.0);
    assert!(!ui.get_native_status_open());
}
