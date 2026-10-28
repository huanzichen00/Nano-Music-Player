use super::*;
use crate::library::{Track, lyrics::Lyrics};

fn fixture(directory: &std::path::Path) -> Controller {
    let mut controller = Controller::new(directory.to_owned());
    controller.state.merge_library(
        (0..20)
            .map(|id| Track {
                path: directory.join(format!("song-{id:02}.wav")),
                title: format!("Song {id:02}"),
                artist: "Artist".into(),
                album: format!("Album {}", id / 5),
                genre: "Genre".into(),
                number: id + 1,
                duration: 120.0,
                artwork: Vec::new(),
                lyrics: Lyrics::parse("[00:00]First line\n[00:10]Second line"),
            })
            .collect(),
    );
    // Exercise projection directly: no audio command or worker poll is needed.
    // Artist-photo downloads remain disabled for reusable controllers.
    controller.state.current = Some(0);
    controller.state.status = PlaybackStatus::Playing;
    controller.state.navigation.open(Screen::Songs(Filter::All));
    controller
}

#[test]
fn progress_sync_reuses_rows_and_does_not_restart_screen_transitions() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let mut controller = fixture(directory.path());
    controller.state.navigation.select(4);
    let ui = AppWindow::new().unwrap();
    controller.initialize(&ui);
    let rows = ui.get_rows();
    let library_rows = controller.library_rows.clone();
    let generation = ui.get_screen_transition_generation();
    let selection = ui.get_selected();

    for position in [1.0, 12.5, 31.0] {
        controller.state.position = position;
        controller.sync(&ui);

        assert!(Rc::ptr_eq(&controller.library_rows, &library_rows));
        assert!(
            ui.get_rows() == rows,
            "progress must retain the Slint model"
        );
        assert_eq!(ui.get_screen_transition_generation(), generation);
        assert_eq!(ui.get_selected(), selection);
        assert!((ui.get_progress() - (position / 120.0) as f32).abs() < f32::EPSILON);
    }
    assert_eq!(ui.get_elapsed(), "0:31");
    assert_eq!(ui.get_active_lyric(), 1);
    assert!(ui.get_playing());
}

#[test]
fn navigation_captures_source_rows_selection_and_scroll_before_destination_sync() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let mut controller = fixture(directory.path());
    controller.state.navigation.select(11);
    let ui = AppWindow::new().unwrap();
    controller.initialize(&ui);
    ui.set_list_scroll_y(-120.0);
    let source_rows = ui.get_rows();
    let source_scroll = ui.get_list_scroll_y();
    let source_selection = ui.get_selected();
    let generation = ui.get_screen_transition_generation();

    controller
        .state
        .navigation
        .open(Screen::Groups(Group::Albums));
    controller.sync(&ui);

    assert!(ui.get_previous_rows() == source_rows);
    assert_eq!(ui.get_previous_selected(), source_selection);
    assert_eq!(ui.get_previous_list_scroll_y(), source_scroll);
    assert_eq!(ui.get_previous_screen_title(), "Songs");
    assert_eq!(ui.get_screen_title(), "Albums");
    assert_eq!(ui.get_selected(), 0);
    assert_eq!(ui.get_list_scroll_y(), 0.0);
    assert_eq!(ui.get_transition_direction(), 1);
    assert_eq!(ui.get_screen_transition_generation(), generation + 1);
    assert_eq!(ui.get_rows().row_count(), 5);
    assert!(ui.get_rows() != source_rows);
    let destination_rows = ui.get_rows();

    controller.state.navigation.select(2);
    controller.state.position = 17.5;
    controller.sync(&ui);

    assert!(ui.get_rows() == destination_rows);
    assert!(ui.get_previous_rows() == source_rows);
    assert_eq!(ui.get_selected(), 2);
    assert_eq!(ui.get_previous_selected(), source_selection);
    assert_eq!(ui.get_previous_list_scroll_y(), source_scroll);
    assert_eq!(ui.get_screen_transition_generation(), generation + 1);
}

#[test]
fn silent_fifo_cache_updates_refresh_current_lyrics_without_replacing_a_newer_notice() {
    use crate::translation::{Request, TargetLanguage};
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let mut controller = fixture(directory.path());
    let first = controller.state.tracks[0].clone();
    let second = controller.state.tracks[1].clone();
    let request = Request {
        request_id: 1,
        track_id: first.path.clone(),
        target: TargetLanguage::English,
        provider_version: format!(
            "{}:{}",
            crate::translation::PROMPT_VERSION,
            crate::ai::DEFAULT_MODEL
        ),
        source: LyricsService::source(&first),
    };
    let value = |text: &str| {
        request
            .validate_response(&serde_json::json!({"translations": [text, "second"]}).to_string())
            .unwrap()
    };
    controller.lyrics.insert_cached_for_test(value("old"));
    controller.lyrics.set_translation_generation_for_test(1);
    controller.settings.translation_target = TargetLanguage::English;
    let ui = AppWindow::new().unwrap();
    controller.initialize(&ui);
    assert_eq!(ui.get_translated_lines().row_data(0).unwrap(), "old");

    // Regenerate A, remove B while browsing it, then return to A before either
    // durable completion is polled. Both notices must remain silent on A.
    controller.lyrics.apply_translation_result(
        request.clone(),
        Ok(value("new")),
        Some(&first),
        TargetLanguage::English,
    );
    controller
        .lyrics
        .remove_translation(Some(&second), TargetLanguage::English)
        .unwrap();
    controller.state.notice = "A newer operation".into();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while controller.lyrics.translation_busy() {
        controller.poll_translation(&ui);
        assert!(
            std::time::Instant::now() < deadline,
            "translation writer timed out"
        );
        std::thread::yield_now();
    }
    assert_eq!(ui.get_translated_lines().row_data(0).unwrap(), "new");
    assert_eq!(controller.state.notice, "A newer operation");
}
