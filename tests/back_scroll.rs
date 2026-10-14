use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        navigation::{Filter, Group, Screen},
    },
    ui::AppWindow,
};
use slint::{ComponentHandle, platform::WindowEvent};
use std::{cell::RefCell, rc::Rc, time::Duration};

fn point(ui: &AppWindow, x: f32, y: f32) -> slint::LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let s = ui.get_display_scale();
    let height = ui.get_content_height();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    slint::LogicalPosition::new(
        (size.width - 240.0 * s) / 2.0 + x * s,
        size.height - height
            + (height - body * s) / 2.0
            + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * s,
    )
}

fn tick(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

#[test]
fn one_trackpad_scroll_burst_goes_back_once_across_screen_transitions() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    Settings {
        monitor_deleted_files: false,
        device_mode: false,
        ..Default::default()
    }
    .save(&tmp.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(tmp.path().to_path_buf())));
    controller
        .borrow_mut()
        .state
        .merge_library(vec![nano_music_player::library::Track {
            path: tmp.path().join("song.wav"),
            title: "Example song".into(),
            artist: "Artist".into(),
            album: "Example".into(),
            genre: "Test".into(),
            number: 1,
            duration: 10.0,
            artwork: Vec::new(),
            lyrics: Default::default(),
        }]);
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    ui.invoke_focus_screen();
    {
        let mut c = controller.borrow_mut();
        c.state.navigation.open(Screen::Music);
        c.state.navigation.open(Screen::Groups(Group::Albums));
        c.state.navigation.open(Screen::Songs(Filter::Album {
            name: "Example".into(),
            artist: Some("Artist".into()),
        }));
        c.execute(&ui, Action::FinishSearchEditing);
    }
    tick(0);
    tick(300);
    ui.window().take_snapshot().unwrap();
    assert_eq!(
        ui.get_transition_offset(),
        0.0,
        "initial transition must settle"
    );
    let scroll = |dx, dy| {
        let s = ui.get_display_scale();
        ui.window().dispatch_event(WindowEvent::PointerScrolled {
            position: point(&ui, 120.0, 100.0),
            delta_x: dx * s,
            delta_y: dy * s,
        });
        tick(0);
    };
    scroll(60.0, 0.0);
    assert_eq!(ui.get_screen_title(), "Albums");
    tick(260); // The screen has settled, but this gesture's momentum continues.
    scroll(60.0, 0.0);
    assert_eq!(
        ui.get_screen_title(),
        "Albums",
        "momentum must not trigger a second Back"
    );
    for _ in 0..5 {
        tick(150);
        scroll(60.0, 0.0);
    }
    assert_eq!(
        ui.get_screen_title(),
        "Albums",
        "continuous momentum keeps the same gesture latched"
    );
    tick(550); // A new gesture after a quiet interval can navigate again.
    scroll(60.0, 0.0);
    assert_eq!(ui.get_screen_title(), "Music");
    // Keyboard Back remains immediate and independent of the scroll latch.
    controller.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::Home
    );
    tick(0);
    tick(300);
    scroll(-60.0, 0.0);
    assert_eq!(
        ui.get_home_page(),
        0,
        "Back momentum must not turn a Home page"
    );
    tick(550);
    scroll(-60.0, 0.0);
    assert_eq!(ui.get_home_page(), 1, "a new Home gesture remains usable");
}
