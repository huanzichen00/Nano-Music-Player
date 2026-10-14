use nano_music_player::ui::AppWindow;
use slint::{
    ComponentHandle, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel,
};

fn lines<T: Into<SharedString>>(strings: impl IntoIterator<Item = T>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        strings.into_iter().map(Into::into).collect::<Vec<_>>(),
    ))
}

fn render(ui: &AppWindow) -> SharedPixelBuffer<Rgba8Pixel> {
    // A real render measures wrapped text; the next frame settles scroll animation.
    let _ = ui.window().take_snapshot().unwrap();
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    slint::platform::update_timers_and_animations();
    ui.window().take_snapshot().unwrap()
}

fn lyric_color_rows(
    ui: &AppWindow,
    pixels: &SharedPixelBuffer<Rgba8Pixel>,
    color: [u8; 3],
) -> Vec<u32> {
    let scale = ui.get_display_scale();
    let factor = ui.window().scale_factor();
    let size = ui.window().size().to_logical(factor);
    let left = (size.width - 240.0 * scale) / 2.0;
    let top =
        size.height - ui.get_content_height() + (ui.get_content_height() - 432.0 * scale) / 2.0;
    // Exclude transport/header icons, which also contain white pixels.
    let x_start = ((left + 10.0 * scale) * factor).ceil() as u32;
    let x_end = ((left + 230.0 * scale) * factor).floor() as u32;
    let y_start = ((top + 184.0 * scale) * factor).ceil() as u32;
    let y_end = ((top + 322.0 * scale) * factor).floor() as u32;
    (y_start..y_end)
        .filter(|y| {
            (x_start..x_end).any(|x| {
                let pixel = pixels.as_slice()[(y * pixels.width() + x) as usize];
                [pixel.r, pixel.g, pixel.b] == color
            })
        })
        .collect()
}

#[test]
fn translated_lyrics_romaji_and_active_scroll_stay_line_aligned() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_device_mode(false);
    ui.set_smooth_slides(false);
    ui.window().set_size(slint::LogicalSize::new(480.0, 864.0));
    ui.set_screen(2);
    ui.set_lyrics_visible(true);
    ui.set_overlay_visible(true);
    ui.set_has_track(true);
    ui.set_lyric_lines(lines([
        "Original lyric stays visible and wraps onto a separate line at the device width",
    ]));
    ui.set_translated_lines(lines([
        "译文在原文下方独立显示，并且换行后仍然不会覆盖原文。",
    ]));
    ui.set_romaji_lines(lines(["romaji pronunciation"]));
    ui.set_romaji_visible(true);
    ui.set_translation_visible(true);
    ui.set_active_lyric(0);
    ui.window().show().unwrap();

    let translated_pixels = render(&ui);
    let original_rows = lyric_color_rows(&ui, &translated_pixels, [255, 255, 255]);
    let translated_rows = lyric_color_rows(&ui, &translated_pixels, [255, 228, 173]);
    assert!(
        !original_rows.is_empty(),
        "translation keeps original glyphs visible"
    );
    assert!(
        !translated_rows.is_empty(),
        "translation glyphs must be rendered"
    );
    assert!(
        original_rows.last().unwrap() < translated_rows.first().unwrap(),
        "wrapped original and translated glyphs must occupy separate lines without overlap"
    );
    assert!(lyric_color_rows(&ui, &translated_pixels, [201, 236, 255]).is_empty());
    assert_eq!(
        ui.get_displayed_lyric_lines().row_data(0),
        ui.get_lyric_lines().row_data(0)
    );
    assert!(!ui.get_romaji_displayed());
    assert!(ui.get_romaji_visible(), "T keeps the saved R preference");

    ui.set_translation_visible(false);
    let romaji_pixels = render(&ui);
    let romaji_rows = lyric_color_rows(&ui, &romaji_pixels, [201, 236, 255]);
    assert!(!romaji_rows.is_empty(), "T off restores enabled romaji");
    assert!(original_rows.last().unwrap() < romaji_rows.first().unwrap());
    assert!(lyric_color_rows(&ui, &romaji_pixels, [255, 228, 173]).is_empty());
    assert!(ui.get_romaji_displayed());

    // Empty/short translation models retain every original and add no blank rows.
    ui.set_romaji_visible(false);
    ui.set_lyric_lines(lines([
        "First original",
        "Second original",
        "Third original",
    ]));
    let originals_only = render(&ui);
    let originals_only_height = ui.get_lyric_content_height();
    ui.set_translation_visible(true);
    for missing in [vec![], vec![""], vec!["", "", ""]] {
        ui.set_translated_lines(lines(missing));
        let untranslated = render(&ui);
        assert_eq!(untranslated.as_bytes(), originals_only.as_bytes());
        assert_eq!(ui.get_lyric_content_height(), originals_only_height);
        assert_eq!(ui.get_displayed_lyric_lines().row_count(), 3);
        assert!(!ui.get_romaji_displayed());
    }
    ui.set_translation_visible(false);
    assert!(
        !ui.get_romaji_displayed(),
        "T off preserves disabled romaji too"
    );

    // Only rows with translations expand, preserving the original row indices.
    ui.set_translated_lines(lines(["", "第二行译文"]));
    ui.set_translation_visible(true);
    let partial = render(&ui);
    assert!(ui.get_lyric_content_height() > originals_only_height);
    assert!(!lyric_color_rows(&ui, &partial, [240, 213, 164]).is_empty());
    ui.set_translated_lines(lines(["", "第二行译文", "", "orphan translation"]));
    assert_eq!(render(&ui).as_bytes(), partial.as_bytes());

    let originals: Vec<String> = (0..18)
        .map(|i| format!("Original lyric line {i} with words that wrap at the device width"))
        .collect();
    let translated: Vec<String> = (0..18)
        .map(|i| {
            format!("译文行 {i} 包含较长的翻译内容，需要在设备宽度内换行显示并保持与原歌词对应。")
                .repeat(2)
        })
        .collect();
    let romaji: Vec<String> = (0..18)
        .map(|i| format!("romaji line {i} with a readable pronunciation"))
        .collect();
    ui.set_lyric_lines(lines(originals));
    ui.set_translated_lines(lines(translated));
    ui.set_romaji_lines(lines(romaji));
    ui.set_translation_visible(false);
    ui.set_romaji_visible(true);
    ui.set_active_lyric(17);
    let _ = render(&ui);
    let source_height = ui.get_lyric_content_height();
    ui.set_translation_visible(true);
    let final_pixels = render(&ui);
    let translated_height = ui.get_lyric_content_height();
    assert_eq!(ui.get_lyric_lines().row_count(), 18);
    assert_eq!(ui.get_translated_lines().row_count(), 18);
    assert_eq!(ui.get_romaji_lines().row_count(), 18);
    assert!(
        translated_height > source_height,
        "wrapped translations must contribute to measured lyric height"
    );
    assert_eq!(ui.get_active_lyric(), 17);
    assert!(
        ui.get_lyric_scroll_y() < 0.0,
        "active final lyric should scroll into view"
    );
    assert!(
        !lyric_color_rows(&ui, &final_pixels, [255, 255, 255]).is_empty(),
        "the active final original must actually be visible after translation expands the rows"
    );
    ui.set_active_lyric(0);
    let _ = render(&ui);
    assert_eq!(
        ui.get_lyric_scroll_y(),
        0.0,
        "backward seek returns to the first pair"
    );
    ui.set_romaji_visible(false);
    ui.set_active_lyric(17);
    ui.set_translated_lines(lines(Vec::<String>::new()));
    let _ = render(&ui);
    assert!(ui.get_lyric_scroll_y() < 0.0);
    // Toggling empty translations changes no row geometry, so there is no y-change
    // notification to repair an accidental scroll reset.
    for visible in [false, true, false] {
        ui.set_translation_visible(visible);
        let pixels = render(&ui);
        assert_eq!(ui.get_active_lyric(), 17);
        assert!(
            ui.get_lyric_scroll_y() < 0.0,
            "T keeps the active lyric in view"
        );
        assert!(!lyric_color_rows(&ui, &pixels, [255, 255, 255]).is_empty());
    }
    ui.window().hide().unwrap();
}
