use nano_music_player::{
    app::{
        action::Action,
        controller::{Controller, connect},
        navigation::{Filter, Group, Screen},
    },
    library::{Track, lyrics::Lyrics},
    ui::AppWindow,
};
use slint::{
    ComponentHandle, Model,
    platform::{Key, PointerEventButton, WindowEvent},
};
use std::{cell::RefCell, rc::Rc};

fn tap(ui: &AppWindow, key: impl Into<slint::SharedString>) {
    let text = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

#[test]
fn shuffle_header_pointer_keyboard_and_cover_geometry_use_the_same_model() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    controller.borrow_mut().state.tracks = ["Song A", "Song B"]
        .into_iter()
        .enumerate()
        .map(|(id, title)| Track {
            path: directory.path().join(format!("missing-{id}.wav")),
            title: title.into(),
            artist: "Reference Artist".into(),
            album: "Reference Album".into(),
            genre: "Test".into(),
            number: id as u32 + 1,
            duration: 60.0,
            artwork: vec![],
            lyrics: Lyrics::default(),
        })
        .collect();
    controller.borrow_mut().state.smooth_slides = false;
    controller
        .borrow_mut()
        .state
        .navigation
        .open(Screen::Songs(Filter::All));
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    ui.invoke_focus_screen();
    assert_eq!(ui.get_rows().row_data(0).unwrap().title, "Shuffle");
    assert_eq!(ui.get_selected(), 0);
    tap(&ui, "j");
    assert_eq!(ui.get_selected(), 1);
    tap(&ui, "k");
    assert_eq!(ui.get_selected(), 0);
    ui.set_notice("".into());
    let image = ui.window().take_snapshot().unwrap();
    let preview = std::env::temp_dir().join("nano-qa/list-shuffle.png");
    std::fs::create_dir_all(preview.parent().unwrap()).unwrap();
    image::save_buffer(
        preview,
        image.as_bytes(),
        image.width(),
        image.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
    tap(&ui, "l");
    assert_eq!(ui.get_screen(), 2);
    assert!(controller.borrow().state.shuffle);
    assert_eq!(controller.borrow().state.queue.order.len(), 2);
    // The fixture paths do not exist, so no generated audio can reach the real output device.
    controller.borrow_mut().execute(&ui, Action::Back);
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let body = 296.0 * 76.5 / 39.6 * scale;
    let top = size.height - ui.get_content_height() + (ui.get_content_height() - body) / 2.0;
    let position = slint::LogicalPosition::new(size.width / 2.0, top + (36.0 + 80.0) * scale);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    assert_eq!(ui.get_screen(), 2);
    assert_eq!(controller.borrow().state.queue.position, 0);
    controller
        .borrow_mut()
        .state
        .navigation
        .open(Screen::Groups(Group::Albums));
    controller.borrow_mut().poll(&ui);
    assert_eq!(ui.get_list_row_height(), 58.0 * ui.get_display_scale());
    assert_eq!(ui.get_list_header_height(), 32.0 * ui.get_display_scale());
    assert_eq!(ui.get_rows().row_count(), 2);
    assert!(!ui.get_rows().row_data(0).unwrap().show_artwork);
    assert!(ui.get_rows().row_data(1).unwrap().show_artwork);
    tap(&ui, "f");
    tap(&ui, "r");
    assert_eq!(ui.get_selected(), 1);
    // Escape cannot turn the control into a metadata entry or rename the album.
    tap(&ui, Key::Escape);
}
