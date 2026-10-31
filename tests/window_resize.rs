use nano_music_player::{app::controller::Controller, ui::AppWindow};
use slint::{
    ComponentHandle,
    platform::{
        Platform, Renderer, WindowAdapter, WindowEvent,
        software_renderer::{RepaintBufferType, SoftwareRenderer},
    },
};
use std::{
    cell::{Cell, OnceCell},
    rc::Rc,
    time::{Duration, Instant},
};

// TestingBackend assumes a fixed scale of 1 and synchronous resize delivery.
// Model native DPI-aware requests whose resize acknowledgement arrives later.
struct NativeWindow {
    window: slint::Window,
    renderer: SoftwareRenderer,
    native_scale_factor: Cell<f32>,
    physical_size: Cell<slint::PhysicalSize>,
    pending: Cell<Option<slint::PhysicalSize>>,
}

impl NativeWindow {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|weak| Self {
            window: slint::Window::new(weak.clone()),
            renderer: SoftwareRenderer::new_with_repaint_buffer_type(RepaintBufferType::NewBuffer),
            native_scale_factor: Cell::new(1.0),
            physical_size: Cell::new(slint::PhysicalSize::default()),
            pending: Cell::new(None),
        })
    }

    fn acknowledge_resize(&self) {
        if let Some(size) = self.pending.take() {
            self.physical_size.set(size);
            self.window.dispatch_event(WindowEvent::Resized {
                size: size.to_logical(self.window.scale_factor()),
            });
        }
    }
}

impl WindowAdapter for NativeWindow {
    fn window(&self) -> &slint::Window {
        &self.window
    }

    fn renderer(&self) -> &dyn Renderer {
        &self.renderer
    }

    fn size(&self) -> slint::PhysicalSize {
        self.physical_size.get()
    }

    fn set_size(&self, size: slint::WindowSize) {
        self.pending
            .set(Some(size.to_physical(self.native_scale_factor.get())));
    }
}

struct DpiPlatform {
    start: Instant,
    native: Rc<OnceCell<Rc<NativeWindow>>>,
}

impl Platform for DpiPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = NativeWindow::new();
        assert!(self.native.set(window.clone()).is_ok());
        Ok(window)
    }

    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

fn settle(controller: &mut Controller, ui: &AppWindow, native: &NativeWindow) {
    controller.poll(ui);
    native.acknowledge_resize();
    std::thread::sleep(Duration::from_millis(160));
    controller.poll(ui);
    native.acknowledge_resize();
}

#[test]
fn backing_scale_changes_preserve_the_players_logical_size_and_zoom() {
    let native = Rc::new(OnceCell::new());
    slint::platform::set_platform(Box::new(DpiPlatform {
        start: Instant::now(),
        native: native.clone(),
    }))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let mut controller = Controller::new(directory.path().to_owned());
    controller.initialize(&ui);
    ui.show().unwrap();
    let native = native.get().unwrap();
    native.acknowledge_resize();
    settle(&mut controller, &ui, native);
    let size = || ui.window().size().to_logical(ui.window().scale_factor());
    let original_size = size();
    let original_zoom = controller.state.zoom;
    for scale_factor in [0.5, 1.0, 2.0, 1.0] {
        native.native_scale_factor.set(scale_factor);
        // Native backing-scale and resize notifications need not arrive in the
        // same poll. Keep the old physical size until the app reacts to the DPI
        // event, as can happen while a manager moves the window off-screen.
        ui.window()
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
        settle(&mut controller, &ui, native);
        assert!(
            (controller.state.zoom - original_zoom).abs() < 0.001,
            "a backing-scale transition must not become a user zoom: {} -> {}",
            original_zoom,
            controller.state.zoom
        );
        assert!(
            (size().width - original_size.width).abs() <= 1.0,
            "DPI {scale_factor}: {:?} became {:?}",
            original_size,
            size()
        );
        assert!(
            (size().height - original_size.height).abs() <= 1.0,
            "DPI {scale_factor}: {:?} became {:?}",
            original_size,
            size()
        );
    }

    // A WM frame can arrive immediately before an unrelated settings save.
    // Persist the accepted logical request, never this unclassified native size.
    let external = slint::PhysicalSize::new(1000, 1400);
    native.physical_size.set(external);
    ui.window().dispatch_event(WindowEvent::Resized {
        size: external.to_logical(ui.window().scale_factor()),
    });
    controller.save_settings(&ui);
    let saved =
        nano_music_player::app::config::Settings::load(&directory.path().join("settings.json"))
            .unwrap();
    assert!((saved.zoom - original_zoom).abs() < 0.001);
    assert!((saved.window_width.unwrap() - original_size.width).abs() <= 1.0);
    assert!((saved.window_height.unwrap() - original_size.height).abs() <= 1.0);
    ui.window().set_size(original_size);
    native.acknowledge_resize();
    settle(&mut controller, &ui, native);

    // A genuine drag after the transition must still update zoom and preserve
    // the device proportions, rather than leaving the window size locked.
    let dragged_width = original_size.width + 60.0;
    ui.window()
        .set_size(slint::LogicalSize::new(dragged_width, original_size.height));
    native.acknowledge_resize();
    settle(&mut controller, &ui, native);
    assert!(
        (size().width - dragged_width).abs() <= 1.0,
        "requested {dragged_width}, actual {:?}, zoom {} -> {}",
        size(),
        original_zoom,
        controller.state.zoom
    );
    assert!(controller.state.zoom > original_zoom);
    assert!(
        (size().width / ui.get_content_height() - 39.6 / 76.5).abs() < 0.002,
        "aspect: native {:?}, content {}, zoom {}",
        size(),
        ui.get_content_height(),
        controller.state.zoom
    );

    // The recording display has a separate aspect and zoom, and must survive
    // asynchronous backing changes without overwriting the portrait geometry.
    let portrait_size = size();
    let portrait_zoom = controller.state.zoom;
    controller.execute(
        &ui,
        nano_music_player::app::action::Action::PresentationMode,
    );
    native.acknowledge_resize();
    settle(&mut controller, &ui, native);
    assert!(controller.state.presentation_mode);
    assert!((size().width / ui.get_content_height() - 3.0).abs() < 0.002);
    let display_size = size();
    let display_zoom = controller.state.zoom;
    for scale_factor in [2.0, 1.0] {
        native.native_scale_factor.set(scale_factor);
        ui.window()
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
        settle(&mut controller, &ui, native);
        assert!((controller.state.zoom - display_zoom).abs() < 0.001);
        assert!((size().width - display_size.width).abs() <= 1.0);
        assert!((size().height - display_size.height).abs() <= 1.0);
    }
    controller.execute(&ui, nano_music_player::app::action::Action::Back);
    native.acknowledge_resize();
    settle(&mut controller, &ui, native);
    assert!(!controller.state.presentation_mode);
    assert!((controller.state.zoom - portrait_zoom).abs() < 0.001);
    assert!((size().width - portrait_size.width).abs() <= 1.0);
    assert!((size().height - portrait_size.height).abs() <= 1.0);
}

#[test]
fn logical_scale_override_keeps_native_requests_and_saved_zoom_stable() {
    let native = Rc::new(OnceCell::new());
    slint::platform::set_platform(Box::new(DpiPlatform {
        start: Instant::now(),
        native: native.clone(),
    }))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let mut controller = Controller::new(directory.path().to_owned());
    controller.initialize(&ui);
    ui.show().unwrap();
    let native = native.get().unwrap();
    native.acknowledge_resize();
    settle(&mut controller, &ui, native);
    let original = ui.window().size().to_logical(ui.window().scale_factor());
    let original_zoom = controller.state.zoom;

    // SLINT_SCALE_FACTOR changes the UI's logical units, while winit's native
    // monitor scale stays at 1. Logical requests must not use that native scale
    // and feed a smaller acknowledgement back into another zoom correction.
    for scale_factor in [1.25, 1.5, 2.0, 1.0] {
        ui.window()
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
        for _ in 0..4 {
            settle(&mut controller, &ui, native);
        }
        let actual = ui.window().size().to_logical(scale_factor);
        assert!(
            (controller.state.zoom - original_zoom).abs() < 0.001,
            "scale {scale_factor} changed zoom from {original_zoom} to {}",
            controller.state.zoom
        );
        assert!((actual.width - original.width).abs() <= 1.0);
        assert!((actual.height - original.height).abs() <= 1.0);
    }
    controller.save_settings(&ui);
    let saved =
        nano_music_player::app::config::Settings::load(&directory.path().join("settings.json"))
            .unwrap();
    assert!((saved.zoom - original_zoom).abs() < 0.001);
    assert!((saved.window_width.unwrap() - original.width).abs() <= 1.0);
    assert!((saved.window_height.unwrap() - original.height).abs() <= 1.0);
}
