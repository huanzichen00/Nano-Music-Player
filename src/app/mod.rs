pub mod action;
mod artwork;
pub mod config;
pub mod controller;
pub mod geometry;
pub mod i18n;
pub mod navigation;
pub mod state;
mod thumbnails;
pub mod typography;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    use slint::{ComponentHandle, Timer, TimerMode};
    use std::{cell::RefCell, rc::Rc, time::Duration};
    // The OpenGL glyph atlas tears small text at some fractional window scales
    // on macOS. Skia rendered the same native fixtures correctly without a font
    // substitution. Keep explicit backend overrides available for diagnostics.
    #[cfg(target_os = "macos")]
    if std::env::var_os("SLINT_BACKEND").is_none() {
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("skia".into())
            .select()?;
    }
    let directory = config::config_dir()?;
    let ui = crate::ui::AppWindow::new()?;
    let controller = Rc::new(RefCell::new(controller::Controller::new(directory)));
    controller::connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    let timer = Timer::default();
    let weak = ui.as_weak();
    let ticking = controller.clone();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        if let Some(ui) = weak.upgrade() {
            ticking.borrow_mut().poll(&ui);
        }
    });
    let video_timer = Timer::default();
    let video_ui = ui.as_weak();
    let video_controller = controller.clone();
    video_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        if let Some(ui) = video_ui.upgrade() {
            video_controller.borrow_mut().poll_video_frame(&ui);
        }
    });
    ui.invoke_focus_screen();
    ui.run()?;
    controller.borrow_mut().save_settings(&ui);
    Ok(())
}
