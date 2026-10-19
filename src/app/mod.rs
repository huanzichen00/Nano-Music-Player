pub use crate::shared::{action, config, i18n, navigation, typography};
mod artwork;
pub mod controller;
pub mod geometry;
mod iphone_sync;
mod settings;
pub mod state;
mod podcast_state;
mod thumbnails;
mod video_thumbnails;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    use slint::{ComponentHandle, Timer, TimerMode};
    use std::{cell::RefCell, rc::Rc, time::Duration};
    crate::platform::select_ui_backend()?;
    let directory = config::config_dir()?;
    if std::env::args_os().any(|arg| arg == "--iphone-sync") {
        let language = config::Settings::load(&directory.join("settings.json"))?.language;
        let panel = iphone_sync::SyncPanel::new(directory, language)?;
        panel.run(language)?;
        return Ok(());
    }
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
