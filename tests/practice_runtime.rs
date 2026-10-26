//! Native practice transport smoke; opt-in because it opens the real audio output.
#![cfg(not(any(target_os = "ios", feature = "mobile")))]
use nano_music_player::{
    app::{
        action::Action,
        controller::{Controller, connect},
    },
    audio::PlaybackStatus,
    library::scanner,
    ui::AppWindow,
};
use slint::Model;
use std::{
    cell::RefCell,
    rc::Rc,
    thread,
    time::{Duration, Instant},
};

fn wait_for(
    c: &Rc<RefCell<Controller>>,
    ui: &AppWindow,
    seconds: u64,
    predicate: impl Fn(&Controller) -> bool,
) {
    let until = Instant::now() + Duration::from_secs(seconds);
    loop {
        c.borrow_mut().poll(ui);
        if predicate(&c.borrow()) {
            return;
        }
        assert!(
            Instant::now() < until,
            "practice audio timeout: status={:?}, position={}, error={}",
            c.borrow().state.status,
            c.borrow().state.position,
            ui.get_practice_error()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "opens native audio output; run explicitly for release smoke verification"]
fn native_practice_load_seek_countdown_cancel_and_finish() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let fixture = include_bytes!("fixtures/quiet.wav");
    let len = (fixture.len() - 44) * 16;
    let mut wav = fixture[..44].to_vec();
    wav[4..8].copy_from_slice(&((len + 36) as u32).to_le_bytes());
    wav[40..44].copy_from_slice(&(len as u32).to_le_bytes());
    for _ in 0..16 {
        wav.extend_from_slice(&fixture[44..]);
    }
    for name in ["one.wav", "two.wav"] {
        std::fs::write(music.join(name), &wav).unwrap();
    }
    scanner::scan_folders(&directory.path().join("library.sqlite3"), &[music]).unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    connect(&ui, &controller);
    wait_for(&controller, &ui, 5, |c| c.state.tracks.len() == 2);
    {
        let mut c = controller.borrow_mut();
        c.state.volume = 0.0;
        c.state.current = Some(0);
        c.state.status = PlaybackStatus::Stopped;
        c.state.queue.set(vec![0, 1], 0, false);
        c.state.repeat = nano_music_player::audio::Repeat::All;
        c.practice_command(&ui, "open", "", "");
        c.practice_command(&ui, "delay", "", "1");
        c.practice_command(&ui, "toggle", "", "");
        c.practice_command(&ui, "save", "", "00:02.500\nLate");
        c.practice_command(&ui, "save", "", "00:01.500\nEarly");
        c.execute(&ui, Action::TogglePlay);
        assert!(c.state.paused);
    }
    wait_for(&controller, &ui, 5, |_| ui.get_practice_countdown() == 1);
    assert!(controller.borrow().state.paused);
    assert!(controller.borrow().state.position < 0.05);
    wait_for(&controller, &ui, 5, |c| {
        c.state.status == PlaybackStatus::Playing && c.state.position > 0.05
    });
    let early = ui.get_practice_markers().row_data(0).unwrap().id;
    let late = ui.get_practice_markers().row_data(1).unwrap().id;
    {
        let mut c = controller.borrow_mut();
        c.practice_command(&ui, "jump", &late, "");
        c.practice_command(&ui, "jump", &early, "");
    }
    wait_for(&controller, &ui, 5, |c| {
        c.state.paused && ui.get_practice_countdown() == 1
    });
    assert!((controller.borrow().state.position - 1.5).abs() < 0.05);
    controller.borrow_mut().execute(&ui, Action::TogglePlay);
    let until = Instant::now() + Duration::from_millis(1300);
    while Instant::now() < until {
        controller.borrow_mut().poll(&ui);
        assert!(
            controller.borrow().state.paused,
            "cancelled countdown resumed"
        );
        thread::sleep(Duration::from_millis(20));
    }
    {
        let mut c = controller.borrow_mut();
        c.practice_command(&ui, "lead_in", "", "1");
        c.practice_command(&ui, "jump", &late, "");
    }
    wait_for(&controller, &ui, 5, |c| {
        c.state.paused && ui.get_practice_countdown() == 1
    });
    assert!((controller.borrow().state.position - 1.5).abs() < 0.05);
    wait_for(&controller, &ui, 8, |c| {
        c.state.status == PlaybackStatus::Stopped
    });
    assert!(controller.borrow().state.paused);
    assert_eq!(
        controller.borrow().state.current,
        Some(0),
        "practice must not auto-advance"
    );
    controller
        .borrow_mut()
        .practice_command(&ui, "restart", "", "");
    wait_for(&controller, &ui, 5, |c| {
        c.state.paused && ui.get_practice_countdown() == 1
    });
    assert!((controller.borrow().state.position - 1.5).abs() < 0.05);
    controller
        .borrow_mut()
        .practice_command(&ui, "cancel", "", "");
}
