use nano_music_player::ui::{AppWindow, LibraryRow};
use slint::{ComponentHandle, Model, ModelNotify, ModelRc, ModelTracker, VecModel};
use std::{cell::Cell, rc::Rc, time::Duration};

struct CountedRows {
    count: usize,
    artwork: bool,
    reads: Rc<Cell<usize>>,
    notify: ModelNotify,
}

impl Model for CountedRows {
    type Data = LibraryRow;

    fn row_count(&self) -> usize {
        self.count
    }

    fn row_data(&self, row: usize) -> Option<LibraryRow> {
        self.reads.set(self.reads.get() + 1);
        (row < self.count).then(|| LibraryRow {
            title: format!("Library item {row:05}").into(),
            subtitle: "Artist".into(),
            show_artwork: self.artwork,
            chevron: true,
            ..Default::default()
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

fn frame(ui: &AppWindow) {
    ui.window().take_snapshot().unwrap();
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
}

#[test]
fn returning_from_a_large_list_only_prepares_visible_rows() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some(
                std::env::var("NANO_TEST_RENDERER")
                    .unwrap_or_else(|_| "software".into())
                    .into(),
            ),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    verify_back("Albums", true, false);
    verify_back("Songs", false, true);
}

fn verify_back(title: &str, artwork: bool, shuffle: bool) {
    let ui = AppWindow::new().unwrap();
    ui.set_device_mode(false);
    ui.set_screen(1);
    ui.set_screen_title(title.into());
    ui.set_shuffle_list(shuffle);
    let reads = Rc::new(Cell::new(0));
    let rows = ModelRc::new(CountedRows {
        count: 5_000,
        artwork,
        reads: reads.clone(),
        notify: ModelNotify::default(),
    });
    ui.set_rows(rows.clone());
    ui.show().unwrap();
    ui.window().set_size(slint::LogicalSize::new(240.0, 432.0));
    frame(&ui);
    let entry_reads = reads.replace(0);

    // Use an interior position: Back must preserve the outgoing list's scroll,
    // rather than building every row between its start and the visible page.
    ui.set_list_scroll_y(-2_500.0 * ui.get_list_row_height());
    frame(&ui);
    let scroll_reads = reads.replace(0);
    ui.set_previous_screen(1);
    ui.set_previous_screen_title(title.into());
    ui.set_previous_shuffle_list(shuffle);
    ui.set_previous_list_scroll_y(ui.get_list_scroll_y());
    ui.set_previous_rows(rows);
    ui.set_music_list(true);
    ui.set_shuffle_list(false);
    ui.set_screen_title("Music".into());
    ui.set_rows(ModelRc::new(VecModel::from(vec![LibraryRow {
        title: title.into(),
        ..Default::default()
    }])));
    ui.set_transition_direction(-1);
    ui.set_screen_transition_generation(1);
    frame(&ui);
    let back_reads = reads.get();
    eprintln!("{title} row reads: entry={entry_reads}, scroll={scroll_reads}, back={back_reads}");
    for (operation, count) in [
        ("entry", entry_reads),
        ("scroll", scroll_reads),
        ("Back", back_reads),
    ] {
        assert!(
            count < 500,
            "{operation} read {count} rows for one viewport"
        );
    }
    assert!(ui.get_transition_offset() < 0.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert!(
        ui.get_transition_offset() < 0.0,
        "the slide must span multiple display frames"
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    frame(&ui);
    assert_eq!(ui.get_previous_rows().row_count(), 0);
    assert_eq!(ui.get_transition_offset(), 0.0);
    ui.hide().unwrap();
}
