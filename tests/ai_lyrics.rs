use nano_music_player::ui::AppWindow;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

#[test]
fn translated_lyrics_romaji_and_active_scroll_stay_line_aligned() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_screen(2);
    ui.set_lyrics_visible(true);
    ui.set_overlay_visible(true);
    ui.set_has_track(true);
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
    ui.set_lyric_lines(ModelRc::new(VecModel::from(
        originals
            .iter()
            .cloned()
            .map(Into::into)
            .collect::<Vec<_>>(),
    )));
    ui.set_translated_lines(ModelRc::new(VecModel::from(
        translated
            .iter()
            .cloned()
            .map(Into::into)
            .collect::<Vec<_>>(),
    )));
    ui.set_romaji_lines(ModelRc::new(VecModel::from(
        romaji.iter().cloned().map(Into::into).collect::<Vec<_>>(),
    )));
    ui.set_translation_visible(false);
    ui.set_romaji_visible(true);
    ui.set_active_lyric(17);
    ui.window().show().unwrap();
    slint::platform::update_timers_and_animations();
    let source_height = ui.get_lyric_content_height();
    ui.set_translation_visible(true);
    slint::platform::update_timers_and_animations();
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
    ui.window().hide().unwrap();
}
