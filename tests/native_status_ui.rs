#![cfg(not(any(target_os = "ios", feature = "mobile")))]
use nano_music_player::{
    app::i18n::{Language, translate},
    ui::{AppWindow, I18n},
};
use slint::{
    ComponentHandle, LogicalPosition,
    platform::{Key, PointerEventButton, WindowEvent},
};

fn click(ui: &AppWindow, x: f32, y: f32) {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let height = ui.get_content_height();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    let position = LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        size.height - height
            + (height - body * scale) / 2.0
            + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    );
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
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
    ui.set_audio_output_wireless(true);
    ui.set_headphone_left_level(64);
    ui.set_headphone_right_level(66);
    ui.set_headphone_case_level(82);
    ui.set_last_media_command("Play · 13:25:02".into());
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
        frame(&ui, &format!("panel-en-{device}"));
        ui.global::<I18n>().set_language(1);

        frame(&ui, &format!("panel-zh-{device}"));
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
}
