#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use nano_music_player::ui::{AppWindow, I18n, StatisticsItem, StatisticsSummary};
use slint::{ComponentHandle, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

fn point(ui: &AppWindow, x: f32, y: f32) -> (f32, f32) {
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

fn marked_photo(width: u32, height: u32) -> slint::Image {
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = if x < width / 5 {
                [230, 20, 20]
            } else if x >= width * 4 / 5 {
                [20, 210, 30]
            } else if y < height / 5 {
                [25, 45, 235]
            } else if y >= height * 4 / 5 {
                [220, 20, 210]
            } else {
                [170, 170, 170]
            };
            pixels.make_mut_slice()[(y * width + x) as usize] = Rgba8Pixel { r, g, b, a: 255 };
        }
    }
    slint::Image::from_rgba8(pixels)
}

fn assert_complete_photo(
    ui: &AppWindow,
    pixels: &SharedPixelBuffer<Rgba8Pixel>,
    frame: (f32, f32, f32, f32),
    ratio: f32,
    context: &str,
) {
    let (x, y, w, h) = frame;
    let (x, y) = point(ui, x, y);
    let factor = ui.window().scale_factor();
    let scale = ui.get_display_scale() * factor;
    let mut counts = [0; 4];
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for py in
        (y * factor).ceil() as u32..((y * factor + h * scale).floor() as u32).min(pixels.height())
    {
        for px in (x * factor).ceil() as u32
            ..((x * factor + w * scale).floor() as u32).min(pixels.width())
        {
            let offset = ((py * pixels.width() + px) * 4) as usize;
            let p = &pixels.as_bytes()[offset..offset + 4];
            let color = if p[0] > 180 && p[1] < 80 && p[2] < 80 {
                Some(0)
            } else if p[0] < 80 && p[1] > 160 && p[2] < 90 {
                Some(1)
            } else if p[0] < 90 && p[1] < 100 && p[2] > 180 {
                Some(2)
            } else if p[0] > 170 && p[1] < 90 && p[2] > 160 {
                Some(3)
            } else {
                None
            };
            if let Some(color) = color {
                counts[color] += 1;
                bounds = Some(bounds.map_or((px, py, px, py), |(l, t, r, b)| {
                    (l.min(px), t.min(py), r.max(px), b.max(py))
                }));
            }
        }
    }
    assert!(
        counts.into_iter().all(|n| n > 2),
        "{context}: all four photo edges must remain visible: {counts:?}, ratio={ratio}, frame={frame:?}"
    );
    let (l, t, r, b) = bounds.unwrap();
    let actual = (r - l + 1) as f32 / (b - t + 1) as f32;
    if (actual / ratio - 1.0).abs() >= 0.12 {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/qa/artist-photo-layout/failure.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::save_buffer(
            path,
            pixels.as_bytes(),
            pixels.width(),
            pixels.height(),
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    assert!(
        (actual / ratio - 1.0).abs() < 0.12,
        "{context}: photo must preserve aspect ratio: expected {ratio}, actual {actual}, bounds={bounds:?}, frame={frame:?}, scale={scale}"
    );
}

#[test]
fn artist_photos_keep_all_edges_and_proportions_in_both_modes_and_three_scales() {
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
    ui.set_statistics_page(true);
    ui.set_keyboard_navigation(false);
    ui.set_smooth_slides(false);
    for device in [false, true] {
        for zoom in [0.75, 1.0, 1.5] {
            ui.set_device_mode(device);
            ui.window().set_size(slint::LogicalSize::new(
                if device { 296.0 } else { 240.0 } * zoom,
                if device { 296.0 * 76.5 / 39.6 } else { 432.0 } * zoom + 40.0,
            ));
            ui.show().unwrap();
            for (width, height) in [(120, 180), (240, 80), (128, 128)] {
                let image = marked_photo(width, height);
                ui.set_statistics_summary(StatisticsSummary {
                    detail_title: "Artist Name".into(),
                    detail_subtitle: "38 plays · 39 tracks".into(),
                    detail_artwork: image.clone(),
                    photo_credit: "Photo: Photographer · CC BY-SA 4.0".into(),
                    photo_url: "https://commons.wikimedia.org/wiki/File:Artist.jpg".into(),
                    ..Default::default()
                });
                ui.set_statistics_items(ModelRc::new(VecModel::from(vec![StatisticsItem {
                    title: "Artist Name".into(),
                    artwork: image,
                    rank: 1,
                    value: "38 plays".into(),
                    ..Default::default()
                }])));
                for page in [1, 2, 8] {
                    ui.set_statistics_kind(page);
                    slint::platform::update_timers_and_animations();
                    let pixels = ui.window().take_snapshot().unwrap();
                    let frame = match page {
                        1 => (27.0, 324.0, 32.0, 32.0),
                        2 => (27.0, 104.0, 36.0, 36.0),
                        _ if width < height => (8.0, 86.0, 76.0, 96.0),
                        _ => (0.0, 80.0, 240.0, 128.0),
                    };
                    assert_complete_photo(
                        &ui,
                        &pixels,
                        frame,
                        width as f32 / height as f32,
                        &format!("device={device} zoom={zoom} page={page}"),
                    );
                    if page == 8 {
                        let header = ui.get_list_header_height() / ui.get_display_scale();
                        assert!((header - if width < height { 126.0 } else { 184.0 }).abs() < 0.1);
                        assert!(
                            ui.get_list_viewport_height() - ui.get_list_header_height()
                                >= 144.0 * ui.get_display_scale(),
                            "four artist actions must fit below the photo"
                        );
                    }
                    if zoom == 1.0 && page == 8 {
                        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("target/qa/artist-photo-layout");
                        std::fs::create_dir_all(&directory).unwrap();
                        image::save_buffer(
                            directory.join(format!("{width}x{height}-{device}.png")),
                            pixels.as_bytes(),
                            pixels.width(),
                            pixels.height(),
                            image::ColorType::Rgba8,
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
    ui.hide().unwrap();
}

#[test]
#[ignore = "optional local-photo visual review; set NANO_ARTIST_PHOTO_QA_CONFIG"]
fn render_cached_artist_photos_for_manual_review() {
    let config = std::path::PathBuf::from(
        std::env::var("NANO_ARTIST_PHOTO_QA_CONFIG")
            .expect("set the source configuration directory"),
    );
    let root = config.join("artist-images-v1");
    let queue: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("queue.json")).unwrap()).unwrap();
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
    ui.set_statistics_page(true);
    ui.set_statistics_kind(8);
    ui.set_screen_title("Artist Details".into());
    ui.set_clock_text("Preview".into());
    ui.set_smooth_slides(false);
    ui.set_keyboard_navigation(false);
    ui.set_statistics_items(ModelRc::new(VecModel::from(vec![
        StatisticsItem {
            title: "Play Artist".into(),
            icon: 10,
            chevron: true,
            ..Default::default()
        },
        StatisticsItem {
            title: "Albums".into(),
            icon: 11,
            chevron: true,
            ..Default::default()
        },
        StatisticsItem {
            title: "Top Tracks".into(),
            icon: 12,
            chevron: true,
            ..Default::default()
        },
        StatisticsItem {
            title: "See All Tracks".into(),
            icon: 13,
            chevron: true,
            ..Default::default()
        },
    ])));
    let output =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa/artist-photo-layout");
    std::fs::create_dir_all(&output).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for job in queue["jobs"]
        .as_object()
        .unwrap()
        .values()
        .filter(|j| j["state"] == "ready")
    {
        let asset = job["asset"].as_str().unwrap();
        assert!(
            asset.len() == 68
                && asset.ends_with(".png")
                && asset[..64].bytes().all(|c| c.is_ascii_hexdigit())
        );
        let path = root.join(asset);
        let (w, h) = image::image_dimensions(&path).unwrap();
        let portrait = w < h;
        if !seen.insert(portrait) {
            continue;
        }
        let pixels = image::open(path).unwrap().to_rgba8();
        let image = slint::Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            pixels.as_raw(),
            w,
            h,
        ));
        ui.set_statistics_summary(StatisticsSummary {
            detail_title: job["artist"].as_str().unwrap().into(),
            detail_subtitle: "Photo layout preview".into(),
            detail_artwork: image,
            photo_credit: format!(
                "Photo: {} · {}",
                job["credit"]["author"].as_str().unwrap(),
                job["credit"]["license"].as_str().unwrap()
            )
            .into(),
            photo_url: job["credit"]["description_url"].as_str().unwrap().into(),
            ..Default::default()
        });
        for device in [false, true] {
            ui.set_device_mode(device);
            ui.window().set_size(slint::LogicalSize::new(
                if device { 444.0 } else { 360.0 },
                if device {
                    296.0 * 76.5 / 39.6 * 1.5
                } else {
                    648.0
                } + 40.0,
            ));
            ui.show().unwrap();
            slint::platform::update_timers_and_animations();
            let shot = ui.window().take_snapshot().unwrap();
            let file = output.join(format!(
                "cached-{}-{device}.png",
                if portrait { "portrait" } else { "landscape" }
            ));
            image::save_buffer(
                &file,
                shot.as_bytes(),
                shot.width(),
                shot.height(),
                image::ColorType::Rgba8,
            )
            .unwrap();
            println!("{}", file.display());
        }
        if seen.len() == 2 {
            break;
        }
    }
    assert_eq!(
        seen.len(),
        2,
        "provide cached portrait and landscape photos for this visual review"
    );
    ui.hide().unwrap();
}
