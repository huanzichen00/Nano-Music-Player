#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use nano_music_player::ui::{AppWindow, I18n, StatisticsItem};
use slint::{ComponentHandle, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

fn display_point(ui: &AppWindow, x: f32, y: f32) -> (f32, f32) {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let content = ui.get_content_height();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    (
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        size.height - content
            + (content - body * scale) / 2.0
            + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    )
}

// Measure actual colored pixels, so a correctly sized but implicitly centered
// Rectangle fails this regression even though its data model is correct.
fn assert_left_fill(
    ui: &AppWindow,
    image: &SharedPixelBuffer<Rgba8Pixel>,
    rect: (f32, f32, f32, f32),
    ratio: f32,
    colored: impl Fn(&[u8]) -> bool,
) {
    let (x, y, width, height) = rect;
    let factor = ui.window().scale_factor();
    let scale = ui.get_display_scale() * factor;
    let start = display_point(ui, x, y);
    let left = start.0 * factor;
    let top = start.1 * factor;
    let mut bounds: Option<(u32, u32)> = None;
    for py in (top.floor() as u32)..((top + height * scale).ceil() as u32).min(image.height()) {
        for px in ((left - 1.0).max(0.0).floor() as u32)
            ..((left + width * scale + 1.0).ceil() as u32).min(image.width())
        {
            let offset = 4 * (py * image.width() + px) as usize;
            if colored(&image.as_bytes()[offset..offset + 4]) {
                bounds = Some(bounds.map_or((px, px), |(lo, hi)| (lo.min(px), hi.max(px))));
            }
        }
    }
    if ratio == 0.0 {
        assert!(bounds.is_none(), "zero fill must not draw colored pixels");
        return;
    }
    let (first, last) = bounds.expect("nonzero fill must be visible");
    assert!(
        (first as f32 - left).abs() <= 2.0,
        "fill must start on the left: ratio={ratio}, expected={left}, actual={first}"
    );
    let right = left + width * ratio * scale;
    assert!(
        (last as f32 + 1.0 - right).abs() <= 2.0,
        "fill must end at its fraction: ratio={ratio}, expected={right}, actual={last}"
    );
}

fn render(ui: &AppWindow) -> SharedPixelBuffer<Rgba8Pixel> {
    slint::platform::update_timers_and_animations();
    ui.window().take_snapshot().unwrap()
}

fn save(image: &SharedPixelBuffer<Rgba8Pixel>, name: &str) {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa/fill-alignment");
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
fn statistics_and_accessory_fills_keep_their_left_edge_in_both_modes() {
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
    ui.set_screen(1);
    ui.set_keyboard_navigation(false);
    ui.set_audio_output_name("AirPods Pro".into());
    ui.set_audio_output_earbuds(true);
    ui.set_native_settings_available(true);
    for device in [false, true] {
        for zoom in [0.75, 1.0, 1.5] {
            ui.set_device_mode(device);
            ui.window().set_size(slint::LogicalSize::new(
                if device { 296.0 } else { 240.0 } * zoom,
                if device { 296.0 * 76.5 / 39.6 } else { 432.0 } * zoom + 40.0,
            ));
            ui.show().unwrap();
            ui.set_statistics_page(true);
            for page in [1, 2, 3, 6] {
                ui.set_statistics_kind(page);
                for ratio in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    ui.set_statistics_items(ModelRc::new(VecModel::from(vec![StatisticsItem {
                        title: "Album".into(),
                        rank: 1,
                        ratio,
                        ..Default::default()
                    }])));
                    let rect = match page {
                        1 => (70.0, 352.0, 144.0, 4.0),
                        6 => (140.0, 218.5, 90.0, 7.0),
                        _ => (70.0, 136.0, 144.0, 4.0),
                    };
                    let image = render(&ui);
                    assert_left_fill(&ui, &image, rect, ratio, |p| {
                        if page == 6 {
                            p[0] < 200 && p[1] > 140 && p[1] < 230 && p[2] > 240
                        } else {
                            p[0] < 70 && p[1] > 100 && p[2] > 220
                        }
                    });
                }
            }
            ui.set_native_status_open(true);
            for level in [0, 25, 50, 75, 100] {
                ui.set_headphone_left_level(level);
                ui.set_headphone_right_level(level);
                ui.set_headphone_case_level(level);
                let image = render(&ui);
                for x in [42.0, 106.0, 170.0] {
                    assert_left_fill(
                        &ui,
                        &image,
                        (x, 258.0, 28.0, 5.0),
                        level as f32 / 100.0,
                        |p| p[0] < 70 && p[1] > 130 && p[2] < 160,
                    );
                }
            }
            ui.set_native_status_open(false);
        }
    }
    ui.set_statistics_kind(3);
    ui.set_statistics_items(ModelRc::new(VecModel::from(
        [1.0, 0.75, 0.5]
            .map(|ratio| StatisticsItem {
                title: "Album".into(),
                value: "Plays".into(),
                ratio,
                ..Default::default()
            })
            .to_vec(),
    )));
    save(&render(&ui), "statistics");
    ui.set_native_status_open(true);
    ui.set_headphone_left_level(100);
    ui.set_headphone_right_level(78);
    ui.set_headphone_case_level(74);
    save(&render(&ui), "device-status");
    ui.hide().unwrap();
}
