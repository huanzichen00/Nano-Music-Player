use nano_music_player::ui::{AppWindow, I18n};
use slint::{
    ComponentHandle,
    platform::{PointerEventButton, WindowEvent},
};
use std::time::{Duration, Instant};

fn save(image: &slint::SharedPixelBuffer<slint::Rgba8Pixel>, name: &str) {
    let directory = std::env::temp_dir().join("nano-qa");
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
fn home_pages_render_real_intermediate_frames_and_ignore_boundary_momentum() {
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
    ui.set_device_mode(false);
    ui.window().set_size(slint::LogicalSize::new(240.0, 432.0));
    ui.set_screen(0);
    let weak = ui.as_weak();
    let requests = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = requests.clone();
    ui.on_home_swipe(move |next| {
        count.set(count.get() + 1);
        let ui = weak.upgrade().unwrap();
        ui.set_home_transition_direction(if next { 1 } else { -1 });
        ui.set_home_page(i32::from(next));
    });
    ui.show().unwrap();
    let before = ui.window().take_snapshot().unwrap();
    let scroll = |delta_x, delta_y| {
        ui.window().dispatch_event(WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(60.0, 80.0),
            delta_x,
            delta_y,
        })
    };
    let started = Instant::now();
    scroll(-10.0, 0.0);
    assert_eq!(ui.get_home_page(), 0);
    scroll(-10.0, 0.0);
    assert_eq!(ui.get_home_page(), 1);
    let event_time = started.elapsed();
    // Process the change handler at the event timestamp, as a native frame does.
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    let initial_offset = ui.get_home_transition_offset();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(70));
    let intermediate_offset = ui.get_home_transition_offset();
    assert!(
        intermediate_offset > 0.0 && intermediate_offset < initial_offset,
        "the visible coordinate must progress between frames"
    );
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(0.0, 0.0),
    });
    let render = Instant::now();
    let middle = ui.window().take_snapshot().unwrap();
    let render_time = render.elapsed();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    let after = ui.window().take_snapshot().unwrap();
    assert_ne!(
        middle.as_bytes(),
        before.as_bytes(),
        "the frame must have moved from its source"
    );
    assert_ne!(
        middle.as_bytes(),
        after.as_bytes(),
        "the frame must not jump straight to its destination"
    );
    let orange_pixels = |frame: &slint::SharedPixelBuffer<slint::Rgba8Pixel>| {
        frame
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|rgba| {
                rgba[0] > 200
                    && rgba[1] > 85
                    && rgba[0] as i32 - rgba[1] as i32 > 50
                    && rgba[2] < 40
            })
            .count()
    };
    assert!(
        orange_pixels(&before) > 200,
        "fixture must show the orange Music artwork"
    );
    assert!(
        orange_pixels(&middle) < orange_pixels(&before) / 3,
        "the source Music artwork must actually move off-screen, beyond dot or selection changes"
    );
    save(&middle, "home-slide-middle");
    save(&after, "home-page-two");
    println!(
        "Home event {:.2}ms; first intermediate software render {:.2}ms",
        event_time.as_secs_f64() * 1000.0,
        render_time.as_secs_f64() * 1000.0
    );
    let accepted = requests.get();
    scroll(-80.0, 0.0);
    scroll(-80.0, 0.0);
    assert_eq!(
        requests.get(),
        accepted,
        "momentum at the last page does not repeat navigation"
    );
    scroll(10.0, 0.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(400));
    scroll(10.0, 0.0);
    assert_eq!(
        ui.get_home_page(),
        1,
        "expired partial gestures do not accumulate"
    );
    scroll(0.0, 30.0);
    scroll(10.0, 0.0);
    assert_eq!(ui.get_home_page(), 1);
    scroll(10.0, 0.0);
    assert_eq!(ui.get_home_page(), 0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    // A swipe beginning on an icon navigates rather than clicking that icon.
    let activated = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = activated.clone();
    ui.on_item_clicked(move |_| flag.set(true));
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: slint::LogicalPosition::new(60.0, 100.0),
        button: PointerEventButton::Left,
    });
    for step in 1..=10 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(60.0 - step as f32 * 4.0, 100.0),
        });
    }
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: slint::LogicalPosition::new(20.0, 100.0),
        button: PointerEventButton::Left,
    });
    assert_eq!(ui.get_home_page(), 1);
    assert!(!activated.get());
    ui.set_smooth_slides(false);
    assert_eq!(ui.get_home_transition_offset(), 0.0);
    scroll(80.0, 0.0);
    assert_eq!(ui.get_home_page(), 0);
    assert_eq!(ui.get_home_transition_offset(), 0.0);
    ui.set_smooth_slides(true);
    scroll(-80.0, 0.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(60));
    let forward_remaining = ui.get_home_transition_offset();
    scroll(80.0, 0.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(
        ((-ui.get_home_transition_offset()) + forward_remaining - initial_offset).abs() < 1.0,
        "rapid reversal retains the current rendered position"
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    assert_eq!(ui.get_home_transition_offset(), 0.0);

    ui.set_reference_open(true);
    let count = requests.get();
    scroll(-80.0, 0.0);
    assert_eq!(requests.get(), count);
    // Exercise the actual dispatcher and controller for H/L, not a mock callback.
    use nano_music_player::app::controller::{Controller, connect};
    let dir = tempfile::tempdir().unwrap();
    let controller = std::rc::Rc::new(std::cell::RefCell::new(Controller::new(
        dir.path().to_owned(),
    )));
    let keyboard_ui = AppWindow::new().unwrap();
    connect(&keyboard_ui, &controller);
    controller.borrow_mut().initialize(&keyboard_ui);
    keyboard_ui.show().unwrap();
    keyboard_ui.invoke_focus_screen();
    keyboard_ui.window().take_snapshot().unwrap();
    let key = |text: &str| {
        keyboard_ui
            .window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.into() });
        keyboard_ui
            .window()
            .dispatch_event(WindowEvent::KeyReleased { text: text.into() });
    };
    let started = Instant::now();
    key("l");
    key("l");
    let keys_elapsed = started.elapsed();
    assert_eq!(keyboard_ui.get_home_page(), 1);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(70));
    assert!(keyboard_ui.get_home_transition_offset() > 0.0);
    let frame = keyboard_ui.window().take_snapshot().unwrap();
    save(&frame, "home-keyboard-slide-middle");
    println!(
        "H/L dispatcher + two keys {:.2}ms; software frame + capture {:.2}ms",
        keys_elapsed.as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64() * 1000.0
    );
    key("h");
    assert_eq!(keyboard_ui.get_home_page(), 0);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    assert_eq!(keyboard_ui.get_home_transition_offset(), 0.0);
}
