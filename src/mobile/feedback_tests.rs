use super::{Controller, connect_controller, connect_translations, storage::Storage};
use crate::ui::MobileWindow;
use slint::{
    ComponentHandle,
    platform::{PointerEventButton, WindowEvent},
};
use std::{cell::RefCell, rc::Rc, time::Duration};

#[test]
fn notices_expire_repeat_and_clear_on_navigation_and_paste_button_reaches_the_editor() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = MobileWindow::new().unwrap();
    connect_translations(&ui);
    ui.show().unwrap();
    ui.window().set_size(slint::LogicalSize::new(390.0, 844.0));
    let directory = tempfile::tempdir().unwrap();
    let controller = Rc::new(RefCell::new(
        Controller::new(Storage::new(directory.path().into()).unwrap()).unwrap(),
    ));
    connect_controller(&ui, controller.clone());
    for _ in 0..2 {
        controller.borrow_mut().notice = "Translation failed; try again".into();
        controller.borrow_mut().present(&ui);
        ui.window().take_snapshot().unwrap();
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(5));
        assert!(!ui.get_notice().is_empty());
        i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(2));
        assert!(
            ui.get_notice().is_empty(),
            "expired notices must clear the controller as well as the display"
        );
    }
    controller.borrow_mut().notice = "Old error".into();
    controller.borrow_mut().command(&ui, "back");
    assert!(ui.get_notice().is_empty());
    crate::platform::ios::copy_text(ui.window(), "  sk-offline-paste-test  ").unwrap();
    controller.borrow_mut().open_api_settings(&ui);
    ui.window().take_snapshot().unwrap();
    let position = slint::LogicalPosition::new(
        ui.get_modal_left() + ui.get_modal_width() - 136.0,
        ui.get_modal_top() + 26.0,
    );
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    assert_eq!(ui.get_api_key(), "sk-offline-paste-test");
    assert!(ui.get_api_settings_open());
}
