use nano_music_player::{
    app::controller::{Controller, connect},
    ui::{AppWindow, GeniusRow},
};
use slint::{Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

#[test]
fn closing_genius_clears_busy_rows_and_reopening_captures_a_fresh_seed() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    connect(&ui, &controller);
    ui.set_has_track(true);
    ui.set_track_path("/fixture/first.wav".into());
    ui.set_track_title("First song".into());
    ui.invoke_open_genius_preview();
    ui.set_genius_busy(true);
    ui.set_genius_message("Generating…".into());
    ui.set_genius_songs(ModelRc::new(VecModel::from(vec![GeniusRow {
        title: "Previous result".into(),
        artist: "Fixture".into(),
        path: "/fixture/result.wav".into(),
    }])));

    // Playback can advance while the preview is open; opening it again must
    // retain the original seed until that preview is explicitly closed.
    ui.set_track_path("/fixture/second.wav".into());
    ui.set_track_title("Second song".into());
    ui.invoke_open_genius_preview();
    assert_eq!(ui.get_genius_seed_path(), "/fixture/first.wav");

    ui.invoke_genius_action("close".into());
    assert!(!ui.get_genius_preview_open());
    assert!(!ui.get_genius_busy());
    assert_eq!(ui.get_genius_songs().row_count(), 0);
    assert!(ui.get_genius_message().is_empty());
    ui.invoke_open_genius_preview();
    assert_eq!(ui.get_genius_seed_path(), "/fixture/second.wav");
    assert_eq!(ui.get_genius_preview_stage(), 0);
    assert!(!ui.get_genius_busy());
    assert_eq!(ui.get_genius_songs().row_count(), 0);
    assert!(controller.borrow().state.queue.order.is_empty());
}
