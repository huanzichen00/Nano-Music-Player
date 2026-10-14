use nano_music_player::ui::{AppWindow, LibraryRow, ListMetrics};
use slint::{ComponentHandle, ModelRc, VecModel};

#[test]
fn import_row_and_missing_preview_do_not_shrink_the_video_list() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_screen(1);
    ui.set_screen_title("Videos".into());
    ui.set_rows(ModelRc::new(VecModel::from(vec![
        LibraryRow {
            title: "Import Videos…".into(),
            ..Default::default()
        },
        LibraryRow {
            title: "Video without a decoded preview yet".into(),
            subtitle: "4:55 · h264".into(),
            show_artwork: true,
            icon: 9,
            ..Default::default()
        },
    ])));
    ui.show().unwrap();
    slint::platform::update_timers_and_animations();
    let _ = ui.window().take_snapshot().unwrap();
    let scale = ui.get_display_scale();
    assert_eq!(
        ui.get_list_row_height(),
        ui.global::<ListMetrics>().get_artwork_row_height() * scale,
        "the Import row must not force thumbnail rows to use text metrics"
    );
    assert!(
        (ui.get_list_viewport_height() - 406.0 * scale).abs() < 1.0,
        "Videos must use the compact artwork-list header"
    );

    ui.set_rows(ModelRc::new(VecModel::from(vec![LibraryRow {
        title: "Ordinary song".into(),
        ..Default::default()
    }])));
    slint::platform::update_timers_and_animations();
    assert_eq!(
        ui.get_list_row_height(),
        ui.global::<ListMetrics>().get_text_row_height() * scale,
        "ordinary song rows must keep their existing spacing"
    );
    ui.hide().unwrap();
}
