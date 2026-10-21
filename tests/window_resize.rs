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
    physical_size: Cell<slint::PhysicalSize>,
    pending: Cell<Option<slint::PhysicalSize>>,
}

impl NativeWindow {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|weak| Self {
            window: slint::Window::new(weak.clone()),
            renderer: SoftwareRenderer::new_with_repaint_buffer_type(RepaintBufferType::NewBuffer),
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
            .set(Some(size.to_physical(self.window.scale_factor())));
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

    // A genuine drag after the transition must still update zoom and preserve
    // the device proportions, rather than leaving the window size locked.
    let dragged_width = original_size.width + 60.0;
    ui.window()
        .set_size(slint::LogicalSize::new(dragged_width, original_size.height));
    native.acknowledge_resize();
    settle(&mut controller, &ui, native);
    assert!((size().width - dragged_width).abs() <= 1.0);
    assert!(controller.state.zoom > original_zoom);
    assert!((size().width / ui.get_content_height() - 39.6 / 76.5).abs() < 0.002);
}
