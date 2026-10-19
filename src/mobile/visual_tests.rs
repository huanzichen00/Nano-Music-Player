//! Render the phone's actual shared views across sizes, orientations and IME states.
use super::connect_translations;
use crate::ui::{I18n, LibraryRow, MobileWindow};
use slint::{ComponentHandle, ModelRc, VecModel};

#[test]
fn mobile_display_matrix_keeps_content_and_modal_actions_inside_safe_areas() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ios-preview/matrix");
    std::fs::create_dir_all(&directory).unwrap();
    for (width, height) in [
        (320.0, 568.0),
        (390.0, 844.0),
        (440.0, 956.0),
        (844.0, 390.0),
    ] {
        let ui = MobileWindow::new().unwrap();
        connect_translations(&ui);
        let landscape = width > height;
        ui.set_safe_top(if landscape { 0.0 } else { 44.0 });
        ui.set_safe_bottom(if landscape { 21.0 } else { 34.0 });
        ui.set_safe_left(if landscape { 44.0 } else { 0.0 });
        ui.set_safe_right(if landscape { 44.0 } else { 0.0 });
        ui.show().unwrap();
        ui.window().set_size(slint::LogicalSize::new(width, height));
        ui.set_clock_text("1:25 PM".into());
        ui.set_track_title("Long title with punctuation (remastered) · 中文 日本語".into());
        ui.set_track_artist("Artist · 歌手".into());
        ui.set_track_album("Album".into());
        ui.set_has_track(true);
        ui.set_elapsed("2:14".into());
        ui.set_duration("5:56".into());
        ui.set_progress(0.37);
        ui.set_timed_lyrics(true);
        ui.set_active_lyric(5);
        ui.set_lyric_lines(ModelRc::new(VecModel::from(
            (0..30)
                .map(|id| {
                    format!(
                        "Line {id}: A long lyric wraps without covering the next line. 中文 日本語"
                    )
                    .into()
                })
                .collect::<Vec<slint::SharedString>>(),
        )));
        ui.set_translated_lines(ModelRc::new(VecModel::from(
            (0..30)
                .map(|id| format!("第 {id} 行翻译 · 日本語の歌詞も表示").into())
                .collect::<Vec<slint::SharedString>>(),
        )));
        ui.set_translation_visible(true);
        for body in [true, false] {
            ui.set_device_mode(body);
            for scene in [
                "home",
                "home2",
                "albums",
                "empty",
                "search",
                "player",
                "loading",
                "error",
                "transfer",
                "api-keyboard",
                "lyrics-keyboard",
                "delete",
            ] {
                ui.set_api_settings_open(false);
                ui.set_lyric_editor_open(false);
                ui.set_playlist_delete_open(false);
                ui.set_library_initializing(false);
                ui.set_notice("".into());
                ui.set_keyboard_height(0.0);
                ui.set_rows(ModelRc::new(VecModel::from(
                    (0..24)
                        .map(|index| LibraryRow {
                            title: format!("Album {index} · 中文 日本語 · A very long album name")
                                .into(),
                            subtitle: "Artist name · 歌手 · Subtitle".into(),
                            chevron: true,
                            show_artwork: true,
                            ..Default::default()
                        })
                        .collect::<Vec<_>>(),
                )));
                ui.set_screen_title(scene.into());
                match scene {
                    "home" | "home2" => {
                        ui.set_screen(0);
                        ui.set_home_page(i32::from(scene == "home2"));
                    }
                    "empty" => {
                        ui.set_screen(1);
                        ui.set_rows(ModelRc::new(VecModel::default()));
                    }
                    "albums" => ui.set_screen(8),
                    "search" => {
                        ui.set_screen(3);
                        ui.set_search_query("中文 日本語 long search".into());
                    }
                    "player" => ui.set_screen(2),
                    "loading" => {
                        ui.set_screen(0);
                        ui.set_library_initializing(true);
                        ui.set_notice("Preparing music library…".into());
                    }
                    "error" => {
                        ui.set_screen(1);
                        ui.set_notice("Translation failed; try again".into());
                    }
                    "transfer" => {
                        ui.set_screen(6);
                        ui.set_transfer_running(true);
                        ui.set_transfer_urls(ModelRc::new(VecModel::from(vec![
                            "http://192.168.100.200:63139/#offline-fixture-token".into(),
                        ])));
                        ui.set_transfer_status("Keep the app open while transferring".into());
                    }
                    "api-keyboard" => {
                        ui.set_screen(5);
                        ui.global::<I18n>().set_language(1);
                        ui.set_api_settings_open(true);
                        ui.set_api_model("deepseek-chat".into());
                    }
                    "lyrics-keyboard" => {
                        ui.set_screen(9);
                        ui.set_lyric_editor_open(true);
                        ui.set_lyric_editor_text(
                            "[00:01.000]中文 日本語\n[00:02.000]A long editable line\n".into(),
                        );
                    }
                    "delete" => {
                        ui.set_screen(8);
                        ui.set_playlist_delete_open(true);
                        ui.set_playlist_delete_name(
                            "Long playlist · 我的歌单 · プレイリスト".into(),
                        );
                    }
                    _ => unreachable!(),
                }
                if scene.ends_with("keyboard") {
                    ui.set_keyboard_height(if landscape { 180.0 } else { 300.0 });
                }
                // Let page transitions and deferred layout/follow callbacks
                // settle before checking the actual rendered state.
                ui.window().take_snapshot().unwrap();
                i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(300));
                let pixels = ui.window().take_snapshot().unwrap();
                assert!(ui.get_display_width() > 0.0 && ui.get_display_height() > 0.0);
                assert!(ui.get_device_top() >= ui.get_safe_top() - 1.0);
                assert!(
                    ui.get_device_top() + ui.get_device_height()
                        <= height - ui.get_safe_bottom() + 1.0
                );
                if scene.ends_with("keyboard") || scene == "delete" {
                    assert!(ui.get_modal_height() > 100.0);
                    assert!(
                        ui.get_modal_top() + ui.get_modal_height()
                            <= height - ui.get_keyboard_height().max(ui.get_safe_bottom()) + 1.0
                    );
                }
                let path = directory.join(format!(
                    "{}x{}-{}-{scene}.png",
                    width as u32,
                    height as u32,
                    if body { "body" } else { "screen" }
                ));
                image::save_buffer(
                    path,
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
