#![cfg(target_os = "windows")]

use nano_music_player::{
    app::{
        action::Action,
        controller::{Controller, connect},
    },
    ui::AppWindow,
};
use slint::{ComponentHandle, Timer, TimerMode, winit_030::WinitWindowAccessor};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

// Use the real HWND and winit resize acknowledgements: a synthetic WindowAdapter
// cannot reproduce HMENU/non-client sizing. The software renderer works on CI
// runners without an interactive GPU. Each scale is run in a separate process.
#[test]
fn native_window_does_not_grow_while_idle_or_after_mode_changes() {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();

    let failure = Rc::new(RefCell::new(None));
    let timer_failure = failure.clone();
    let weak = ui.as_weak();
    let ticking = controller.clone();
    let mut stage = 0;
    let mut started = Instant::now();
    let mut samples = Vec::new();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let ui = weak.upgrade().unwrap();
        let mut controller = ticking.borrow_mut();
        controller.poll(&ui);
        if let Some(size) = ui.window().with_winit_window(|native| {
            native
                .inner_size()
                .to_logical::<f32>(f64::from(ui.window().scale_factor()))
        }) {
            samples.push((size.width, size.height, ui.get_content_height()));
        }
        if started.elapsed() < Duration::from_secs(2) {
            return;
        }

        let (base, zoom) = match stage {
            0 => ((296.0, 296.0 * 76.5 / 39.6), 1.25),
            1 => ((240.0, 432.0), 1.25),
            2 => ((240.0, 432.0), 1.45),
            3 | 5 => ((296.0, 296.0 * 76.5 / 39.6), 1.45),
            _ => ((540.0, 180.0), 1.0),
        };
        let recent: Vec<_> = samples.iter().rev().take(5).copied().collect();
        let tolerance = (1.0 / ui.window().scale_factor()).max(1.0) + 0.1;
        let stable = recent.len() == 5
            && (controller.state.zoom - zoom).abs() < 0.01
            && recent.iter().all(|&(width, height, content)| {
                (width - base.0 * zoom).abs() <= tolerance
                    && (content - base.1 * zoom).abs() <= tolerance
                    && (height - recent[0].1).abs() <= tolerance
            });
        eprintln!(
            "stage {stage}, DPI {}, zoom {}, samples {recent:?}",
            ui.window().scale_factor(),
            controller.state.zoom
        );
        if !stable {
            *timer_failure.borrow_mut() = Some(format!(
                "stage {stage}: expected stable base {base:?} at zoom {zoom}, got zoom {} and {recent:?}",
                controller.state.zoom
            ));
            slint::quit_event_loop().unwrap();
            return;
        }

        match stage {
            0 => controller.execute(&ui, Action::ScreenMode),
            1 => {
                // An external width change must still be accepted and corrected
                // to the proper aspect instead of fixing growth by locking size.
                // Use physical pixels: SLINT_SCALE_FACTOR can override Slint's
                // logical units independently of the monitor's native DPI.
                let requested = slint::LogicalSize::new(348.0, recent[0].1)
                    .to_physical(ui.window().scale_factor());
                ui.window().with_winit_window(|native| {
                    let _ = native.request_inner_size(
                        slint::winit_030::winit::dpi::PhysicalSize::new(
                            requested.width,
                            requested.height,
                        ),
                    );
                });
            }
            2 => controller.execute(&ui, Action::DeviceMode),
            3 => controller.execute(&ui, Action::PresentationMode),
            4 => controller.execute(&ui, Action::Back),
            _ => {
                controller.save_settings(&ui);
                slint::quit_event_loop().unwrap();
            }
        }
        stage += 1;
        samples.clear();
        started = Instant::now();
    });
    slint::run_event_loop().unwrap();
    timer.stop();
    assert!(failure.borrow().is_none(), "{:?}", failure.borrow());
    let saved =
        nano_music_player::app::config::Settings::load(&directory.path().join("settings.json"))
            .unwrap();
    assert!((saved.zoom - 1.45).abs() < 0.01);
}
