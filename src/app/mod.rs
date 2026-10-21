pub use crate::shared::{action, config, i18n, navigation, typography};
mod artwork;
pub mod controller;
pub mod geometry;
mod iphone_sync;
mod podcast_state;
mod settings;
pub mod state;
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
    #[cfg(target_os = "macos")]
    let focus_hotkey = match crate::platform::macos_hotkey::GlobalHotkey::register_focus() {
        Ok(hotkey) => Some(hotkey),
        Err(error) => {
            eprintln!("{error}");
            controller.borrow_mut().state.notice = error;
            None
        }
    };
    #[cfg(target_os = "macos")]
    let hide_hotkey = match crate::platform::macos_hotkey::GlobalHotkey::register_hide() {
        Ok(hotkey) => Some(hotkey),
        Err(error) => {
            eprintln!("{error}");
            controller.borrow_mut().state.notice = error;
            None
        }
    };
    controller.borrow_mut().initialize(&ui);
    let timer = Timer::default();
    let weak = ui.as_weak();
    let ticking = controller.clone();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        if let Some(ui) = weak.upgrade() {
            #[cfg(target_os = "macos")]
            if focus_hotkey
                .as_ref()
                .is_some_and(|hotkey| hotkey.take_pending())
            {
                use slint::winit_030::WinitWindowAccessor;
                // Restore minimized/hidden windows before requesting native focus.
                // Keep the current screen and its text-input focus intact.
                crate::platform::macos_hotkey::set_application_hidden(false);
                let _ = ui.show();
                ui.window().with_winit_window(|window| {
                    window.set_minimized(false);
                    window.set_visible(true);
                    window.focus_window();
                });
            }
            #[cfg(target_os = "macos")]
            if hide_hotkey
                .as_ref()
                .is_some_and(|hotkey| hotkey.take_pending())
            {
                crate::platform::macos_hotkey::set_application_hidden(true);
            }
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
