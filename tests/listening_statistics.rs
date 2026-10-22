#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        i18n::{Language, has_translation},
        navigation::{Filter, Screen, StatisticsDetailPage, StatisticsPage},
        state::{State, Target},
    },
    audio::PlaybackStatus,
    library::{
        Track,
        statistics::{StatisticsRange, StatisticsStore, StatisticsTrack},
    },
    ui::AppWindow,
};
use slint::{ComponentHandle, Model};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

fn track(path: PathBuf, title: &str, artist: &str, number: u32) -> Track {
    Track {
        path,
        title: title.into(),
        artist: artist.into(),
        album: "Same Album".into(),
        genre: String::new(),
        number,
        duration: 120.0,
        artwork: Vec::new(),
        lyrics: Default::default(),
    }
}
fn metadata(tracks: &[Track]) -> Vec<StatisticsTrack> {
    tracks
        .iter()
        .map(|t| StatisticsTrack {
            path: t.path.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
        })
        .collect()
}
fn ready(c: &RefCell<Controller>, ui: &AppWindow) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        c.borrow_mut().poll(ui);
        if c.borrow().state.statistics.snapshot.is_some() && !c.borrow().state.statistics.loading {
            return;
        }
        assert!(Instant::now() < deadline, "statistics query timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn open(c: &RefCell<Controller>, ui: &AppWindow, page: StatisticsPage) {
    c.borrow_mut()
        .state
        .navigation
        .open(Screen::Statistics(page));
    c.borrow_mut().poll(ui);
}
fn activate_title(state: &mut State, title: &str) {
    let selected = state
        .rows()
        .iter()
        .position(|row| row.title == title)
        .unwrap();
    state.navigation.select(selected);
    state.apply(Action::Activate);
}
fn open_detail(c: &RefCell<Controller>, ui: &AppWindow, page: StatisticsPage) {
    open(c, ui, page);
    ready(c, ui);
    let index = c
        .borrow()
        .state
        .rows()
        .iter()
        .position(|row| {
            matches!(
                row.target,
                Target::Screen(Screen::StatisticsDetail {
                    page: StatisticsDetailPage::Summary,
                    ..
                })
            )
        })
        .unwrap();
    ui.invoke_item_clicked(index as i32);
    assert!(matches!(
        c.borrow().state.navigation.current().screen,
        Screen::StatisticsDetail {
            page: StatisticsDetailPage::Summary,
            ..
        }
    ));
}
fn activate_icon(ui: &AppWindow, icon: i32) {
    let index = ui
        .get_statistics_items()
        .iter()
        .position(|item| item.icon == icon)
        .unwrap();
    ui.invoke_item_clicked(index as i32);
}
fn nano_position(ui: &AppWindow, x: f32, y: f32) -> slint::LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let content = ui.get_content_height();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    let top = size.height - content + (content - body * scale) / 2.0;
    slint::LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        top + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    )
}
fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = nano_position(ui, x, y);
    let button = slint::platform::PointerEventButton::Left;
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::PointerPressed { position, button });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::PointerReleased { position, button });
}

fn assert_range_checkmark(ui: &AppWindow, expected_row: usize) {
    ui.set_keyboard_navigation(false);
    let shot = ui.window().take_snapshot().unwrap();
    let factor = ui.window().scale_factor();
    for row in 0..3 {
        let top = nano_position(ui, 205.0, 64.0 + row as f32 * 42.0 + 3.0);
        let bottom = nano_position(ui, 234.0, 64.0 + row as f32 * 42.0 + 39.0);
        let mut blue_pixels = 0;
        for y in ((top.y * factor) as u32).min(shot.height())
            ..((bottom.y * factor) as u32).min(shot.height())
        {
            for x in ((top.x * factor) as u32).min(shot.width())
                ..((bottom.x * factor) as u32).min(shot.width())
            {
                let offset = ((y * shot.width() + x) * 4) as usize;
                let pixel = &shot.as_bytes()[offset..offset + 4];
                if pixel[0] < 80 && pixel[1] > 70 && pixel[2] > 160 {
                    blue_pixels += 1;
                }
            }
        }
        assert_eq!(
            blue_pixels > 1,
            row == expected_row,
            "range checkmark row {row}"
        );
    }
}

fn snapshot(ui: &AppWindow, name: &str) {
    slint::platform::update_timers_and_animations();
    let shot = ui.window().take_snapshot().unwrap();
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/qa/statistics");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        shot.as_bytes(),
        shot.width(),
        shot.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
    // Catch a native-layout regression where a header's implicit centered y
    // puts it underneath the opaque list. These fixed fixtures use the blue
    // artwork fallback, and populated daily charts must contain blue bars.
    let region = if ui.get_statistics_page() {
        match ui.get_statistics_kind() {
            8 => Some((12.0, 70.0, 228.0, 180.0, 0.1)),
            9 => Some((12.0, 74.0, 84.0, 146.0, 0.1)),
            6 if ui.get_statistics_days().iter().any(|day| day.ratio > 0.0) => {
                Some((24.0, 102.0, 230.0, 198.0, 0.005))
            }
            _ => None,
        }
    } else {
        None
    };
    if let Some((left, top, right, bottom, minimum)) = region {
        let first = nano_position(ui, left, top);
        let last = nano_position(ui, right, bottom);
        let factor = ui.window().scale_factor();
        let mut blue = 0;
        let mut pixels = 0;
        for y in ((first.y * factor) as u32).min(shot.height())
            ..((last.y * factor) as u32).min(shot.height())
        {
            for x in ((first.x * factor) as u32).min(shot.width())
                ..((last.x * factor) as u32).min(shot.width())
            {
                let offset = ((y * shot.width() + x) * 4) as usize;
                let pixel = &shot.as_bytes()[offset..offset + 4];
                pixels += 1;
                if pixel[2] > pixel[0].saturating_add(30) && pixel[2] > 130 && pixel[1] > 60 {
                    blue += 1;
                }
            }
        }
        assert!(
            pixels > 0 && blue as f32 / pixels as f32 > minimum,
            "{name}: statistics header/chart is obscured ({blue}/{pixels} blue pixels)"
        );
    }
}

#[test]
fn statistics_drilldown_preserves_identity_album_order_and_existing_playback_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let store = StatisticsStore::open(&tmp.path().join("db")).unwrap();
    let tracks = vec![
        track("/a.wav".into(), "Z first", " Artist ", 1),
        track("/b.wav".into(), "A second", "artist", 2),
        track("/c.wav".into(), "Other", "Other Artist", 1),
    ];
    for (id, count) in [(0, 2), (1, 1), (2, 1)] {
        for _ in 0..count {
            store
                .record_play(&tracks[id].path, chrono::Utc::now().timestamp(), 30)
                .unwrap();
        }
    }
    let mut state = State {
        tracks: tracks.clone(),
        ..State::default()
    };
    state.statistics.snapshot = Some(
        store
            .listening_statistics(
                &metadata(&tracks),
                StatisticsRange::AllTime,
                chrono::Local::now(),
            )
            .unwrap(),
    );
    state
        .navigation
        .open(Screen::Statistics(StatisticsPage::Artists));
    state.apply(Action::Activate);
    assert!(
        matches!(&state.navigation.current().screen, Screen::StatisticsDetail {group, page: StatisticsDetailPage::Summary} if group.artist.is_none() && group.paths.len() == 2)
    );
    assert!(state.queue.order.is_empty(), "opening details only browses");
    activate_title(&mut state, "See All Tracks");
    assert_eq!(
        state
            .rows()
            .iter()
            .filter(|r| matches!(r.target, Target::Play(_)))
            .count(),
        2
    );
    state
        .navigation
        .open(Screen::Statistics(StatisticsPage::Albums));
    assert_eq!(
        state.rows().len(),
        2,
        "same-name albums from different artists stay separate"
    );
    state.apply(Action::Activate);
    assert!(
        matches!(&state.navigation.current().screen, Screen::StatisticsDetail { group, page: StatisticsDetailPage::Summary } if group.artist.is_some() && group.paths.len() == 2)
    );
    activate_title(&mut state, "See All Tracks");
    assert!(
        !state.supports_list_shuffle(),
        "album keeps the existing album browsing behavior"
    );
    assert_eq!(
        state
            .rows()
            .iter()
            .map(|r| r.title.as_str())
            .collect::<Vec<_>>(),
        ["Z first", "A second"]
    );

    state
        .navigation
        .open(Screen::Statistics(StatisticsPage::Tracks));
    state.navigation.select(1);
    let selected = match state.rows()[1].target {
        Target::Play(id) => id,
        _ => panic!("playable track"),
    };
    let effects = state.apply(Action::Activate);
    assert!(!effects.is_empty());
    assert_eq!(state.navigation.current().screen, Screen::NowPlaying);
    assert_eq!(state.current, Some(selected));
    assert_eq!(state.queue.order.len(), 3);
    let queue = state.queue.order.clone();
    state
        .navigation
        .open(Screen::Statistics(StatisticsPage::Recent));
    assert_eq!(
        state.rows().len(),
        4,
        "history retains repeated qualified events"
    );
    state.apply(Action::Activate);
    assert!(
        matches!(&state.navigation.current().screen, Screen::Songs(Filter::Statistics {paths, ..}) if paths.len() == 1)
    );
    assert_eq!(
        state.queue.order, queue,
        "browsing history does not replace the playback queue"
    );

    state
        .navigation
        .open(Screen::Statistics(StatisticsPage::Tracks));
    state.mark_track_available(&tracks[0].path, false);
    assert!(
        matches!(state.rows()[0].target, Target::None),
        "even an old snapshot cannot play an unavailable path"
    );
}

#[test]
fn home_statistics_entry_and_copy_follow_existing_navigation_and_localization() {
    let mut state = State::default();
    state.apply(Action::Right);
    state.apply(Action::Right);
    assert_eq!(state.home_page, 1);
    assert_eq!(state.selection_count(), 3);
    state.apply(Action::SelectNext);
    assert_eq!(state.navigation.selected(), 2);
    state.apply(Action::Activate);
    assert_eq!(
        state.navigation.current().screen,
        Screen::Statistics(StatisticsPage::Home)
    );
    state.apply(Action::Back);
    assert_eq!(state.navigation.current().screen, Screen::Home);
    assert_eq!(state.home_page, 1);
    assert_eq!(state.navigation.selected(), 2);
    for page in [
        StatisticsPage::Home,
        StatisticsPage::Overview,
        StatisticsPage::Artists,
        StatisticsPage::Albums,
        StatisticsPage::Tracks,
        StatisticsPage::Recent,
        StatisticsPage::Daily,
        StatisticsPage::Range,
    ] {
        assert!(has_translation(page.title()));
    }
    for range in StatisticsRange::ALL {
        assert!(has_translation(range.label()));
    }
    for text in [
        "{0} plays",
        "Local calendar days",
        "History: latest 1,000 plays.\nDate totals may be incomplete.",
        "Unable to load. Refresh to retry.",
        "No plays yet",
        "7 Days",
        "30 Days",
        "Summary and charts",
        "Artist Details",
        "Album Details",
        "Play Artist",
        "Play Album",
        "See All Tracks",
        "Latest 1,000 plays; date totals may be partial.",
        "{0} plays · {1} tracks",
        "Load failed. Tap to retry.",
    ] {
        assert!(has_translation(text), "missing {text}");
    }
}

#[test]
fn desktop_statistics_load_refresh_navigate_and_render_in_both_modes() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    Settings {
        smooth_slides: false,
        device_mode: false,
        ..Default::default()
    }
    .save(&tmp.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let c = Rc::new(RefCell::new(Controller::new(tmp.path().to_owned())));
    connect(&ui, &c);
    c.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    open(&c, &ui, StatisticsPage::Home);
    ready(&c, &ui);
    assert_eq!(ui.get_statistics_items().row_count(), 9);
    assert_eq!(ui.get_statistics_summary().total, "0");
    snapshot(&ui, "empty-home");
    click(&ui, 80.0, 120.0); // The illustrated total-play banner opens Overview.
    assert_eq!(
        c.borrow().state.navigation.current().screen,
        Screen::Statistics(StatisticsPage::Overview)
    );
    assert_eq!(ui.get_statistics_summary().artists, "0");
    snapshot(&ui, "empty-overview");
    c.borrow_mut().execute(&ui, Action::Back);
    open(&c, &ui, StatisticsPage::Tracks);
    ready(&c, &ui);
    assert_eq!(
        ui.get_statistics_items().row_data(0).unwrap().title,
        "No plays yet"
    );
    snapshot(&ui, "empty-tracks");
    open(&c, &ui, StatisticsPage::Daily);
    ready(&c, &ui);
    assert_eq!(ui.get_statistics_items().row_count(), 30);
    assert!(ui.get_statistics_items().iter().all(|row| row.value == "0"));
    assert_eq!(ui.get_statistics_days().row_count(), 30);
    assert!(ui.get_statistics_days().iter().all(|day| day.ratio == 0.0));
    snapshot(&ui, "empty-daily");

    let db = tmp.path().join("library.sqlite3");
    let store = StatisticsStore::open(&db).unwrap();
    let tracks: Vec<_> = (0..8)
        .map(|i| {
            let path = tmp.path().join(format!("{i}.wav"));
            std::fs::write(&path, include_bytes!("fixtures/quiet.wav")).unwrap();
            track(
                path,
                if i == 0 {
                    "Music"
                } else {
                    "A long song title for the narrow nano display"
                },
                if i == 0 {
                    "Artist"
                } else {
                    "An unusually long artist credit that should not hide the count"
                },
                i + 1,
            )
        })
        .collect();
    let now = chrono::Utc::now().timestamp();
    for (i, track) in tracks.iter().enumerate() {
        // Eight real daily buckets across the complete 30-day chart, making
        // 7-day/30-day selection visibly and numerically different.
        for _ in 0..=i {
            store
                .record_play(&track.path, now - i as i64 * 4 * 86_400, 30)
                .unwrap();
        }
    }
    let removed = tmp.path().join("removed.wav");
    store.record_play(&removed, now, 30).unwrap();
    {
        let mut controller = c.borrow_mut();
        controller.state.merge_library(tracks);
        controller.state.notice.clear();
        controller.state.current = Some(0);
        controller.state.status = PlaybackStatus::Playing;
        controller.state.paused = false;
        controller.state.position = 42.0;
        controller.state.queue.set(vec![0, 1], 0, false);
    }
    open(&c, &ui, StatisticsPage::Home);
    ui.invoke_statistics_refresh();
    open(&c, &ui, StatisticsPage::Overview); // Refresh survives immediate subpage navigation.
    ready(&c, &ui);
    let summary = ui.get_statistics_summary();
    assert_eq!(summary.total, "37");
    assert_eq!(summary.artists, "2");
    assert_eq!(summary.albums, "2");
    assert_eq!(summary.tracks, "8");
    assert_eq!(ui.get_statistics_days().row_count(), 30);
    assert_eq!(
        ui.get_statistics_days()
            .iter()
            .filter(|day| day.ratio > 0.0)
            .count(),
        8
    );
    snapshot(&ui, "overview");
    assert_eq!(c.borrow().state.status, PlaybackStatus::Playing);
    assert_eq!(c.borrow().state.position, 42.0);
    assert_eq!(c.borrow().state.queue.order, [0, 1]);

    // Exercise the visible segmented control by pointer, not only its callback.
    for (x, index, range, total) in [
        (45.0, 0, StatisticsRange::Last7Days, "4"),
        (120.0, 1, StatisticsRange::Last30Days, "37"),
        (195.0, 2, StatisticsRange::AllTime, "37"),
    ] {
        click(&ui, x, 81.0);
        ready(&c, &ui);
        assert_eq!(c.borrow().state.statistics.range, range);
        assert_eq!(ui.get_statistics_range_index(), index);
        assert_eq!(ui.get_statistics_summary().total, total);
        assert_eq!(
            c.borrow().state.navigation.current().screen,
            Screen::Statistics(StatisticsPage::Overview)
        );
        assert_eq!(c.borrow().state.position, 42.0);
        assert_eq!(c.borrow().state.queue.order, [0, 1]);
        assert!(ui.get_statistics_summary().footer.contains("1,000"));
        snapshot(&ui, &format!("overview-range-{index}"));
    }
    ui.invoke_statistics_range_selected(-1);
    ui.invoke_statistics_range_selected(3);
    assert_eq!(c.borrow().state.statistics.range, StatisticsRange::AllTime);

    // The legacy list uses All/7/30 order while the tabs use 7/30/All.
    // Check both semantic selection and the actual rendered checkmark.
    for (row, range, tab_index) in [
        (1, StatisticsRange::Last7Days, 0),
        (2, StatisticsRange::Last30Days, 1),
        (0, StatisticsRange::AllTime, 2),
    ] {
        open(&c, &ui, StatisticsPage::Range);
        ui.invoke_item_clicked(row);
        ready(&c, &ui);
        assert_eq!(c.borrow().state.statistics.range, range);
        open(&c, &ui, StatisticsPage::Range);
        assert_eq!(ui.get_statistics_range_index(), tab_index);
        assert_eq!(
            ui.get_statistics_items()
                .row_data(row as usize)
                .unwrap()
                .subtitle,
            "Selected"
        );
        assert_range_checkmark(&ui, row as usize);
        snapshot(&ui, &format!("range-list-{row}"));
    }
    open(&c, &ui, StatisticsPage::Overview);

    // Summary cards and the first artist row use the production pointer path.
    click(&ui, 170.0, 126.0);
    assert_eq!(
        c.borrow().state.navigation.current().screen,
        Screen::Statistics(StatisticsPage::Artists)
    );
    ready(&c, &ui);
    click(&ui, 100.0, 122.0);
    assert_eq!(ui.get_statistics_kind(), 8);
    assert!(
        ui.get_statistics_summary()
            .detail_subtitle
            .contains("35 plays")
    );
    snapshot(&ui, "artist-details");
    activate_icon(&ui, 11); // Artist albums.
    assert!(matches!(
        c.borrow().state.navigation.current().screen,
        Screen::StatisticsDetail {
            page: StatisticsDetailPage::Albums,
            ..
        }
    ));
    ui.invoke_item_clicked(0);
    assert_eq!(ui.get_statistics_kind(), 9);
    snapshot(&ui, "album-details");
    activate_icon(&ui, 13); // Exact-path album browsing.
    assert!(matches!(
        c.borrow().state.navigation.current().screen,
        Screen::Songs(Filter::Statistics {
            album_order: true,
            ..
        })
    ));
    assert_eq!(ui.get_rows().row_count(), 7);
    assert_eq!(c.borrow().state.queue.order, [0, 1]);
    c.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(ui.get_statistics_kind(), 9);

    open(&c, &ui, StatisticsPage::Recent);
    ready(&c, &ui);
    let unavailable = ui
        .get_statistics_items()
        .iter()
        .position(|row| row.subtitle == "Unavailable")
        .unwrap();
    assert!(
        !ui.get_statistics_items()
            .row_data(unavailable)
            .unwrap()
            .chevron
    );
    assert!(
        !ui.get_statistics_items()
            .row_data(unavailable)
            .unwrap()
            .value
            .is_empty()
    );
    ui.invoke_item_clicked(unavailable as i32);
    assert_eq!(
        c.borrow().state.navigation.current().screen,
        Screen::Statistics(StatisticsPage::Recent)
    );

    for language in Language::ALL {
        c.borrow_mut().state.language = language;
        for device in [false, true] {
            for zoom in [0.75, 1.0, 1.5] {
                {
                    let mut controller = c.borrow_mut();
                    controller.state.zoom = zoom;
                    controller.execute(
                        &ui,
                        if device {
                            Action::DeviceMode
                        } else {
                            Action::ScreenMode
                        },
                    );
                }
                for (page, name) in [
                    (StatisticsPage::Overview, "overview"),
                    (StatisticsPage::Artists, "artists"),
                    (StatisticsPage::Albums, "albums"),
                    (StatisticsPage::Tracks, "tracks"),
                    (StatisticsPage::Recent, "recent"),
                ] {
                    open(&c, &ui, page);
                    ready(&c, &ui);
                    assert!(ui.get_list_viewport_height() > 280.0 * ui.get_display_scale());
                    if page == StatisticsPage::Tracks {
                        assert!(
                            ui.get_statistics_items()
                                .row_data(0)
                                .unwrap()
                                .value
                                .starts_with('8'),
                            "play count has its own column beside long metadata"
                        );
                    }
                    if matches!(page, StatisticsPage::Artists | StatisticsPage::Albums) {
                        assert!(
                            ui.get_statistics_items()
                                .row_data(0)
                                .unwrap()
                                .value
                                .starts_with("35"),
                            "rank values must remain populated after projection"
                        );
                    }
                    snapshot(&ui, &format!("{name}-{}-{device}-{zoom}", language.code()));
                }
                for (page, name, kind) in [
                    (StatisticsPage::Artists, "artist-details", 8),
                    (StatisticsPage::Albums, "album-details", 9),
                ] {
                    open_detail(&c, &ui, page);
                    assert_eq!(ui.get_statistics_kind(), kind);
                    assert!(!ui.get_statistics_summary().detail_title.is_empty());
                    snapshot(&ui, &format!("{name}-{}-{device}-{zoom}", language.code()));
                }
                open(&c, &ui, StatisticsPage::Daily);
                ready(&c, &ui);
                for _ in 0..30 {
                    c.borrow_mut().execute(&ui, Action::SelectNext);
                }
                assert_eq!(ui.get_selected(), 29);
                assert!(ui.get_list_scroll_y() < 0.0);
                let viewport = ui.get_list_viewport_height() - ui.get_list_header_height();
                assert!(
                    (30.0 * ui.get_list_row_height() + ui.get_list_scroll_y()) <= viewport + 1.0,
                    "last daily row must remain visible to keyboard users"
                );
                snapshot(&ui, &format!("daily-{}-{device}-{zoom}", language.code()));
            }
        }
    }
    assert_eq!(c.borrow().state.position, 42.0);
    assert_eq!(c.borrow().state.queue.order, [0, 1]);

    // Database failure changes only statistics presentation and can be retried.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("ALTER TABLE listening_history RENAME TO saved_history")
        .unwrap();
    open(&c, &ui, StatisticsPage::Home);
    ui.invoke_statistics_refresh();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !c.borrow().state.statistics.failed {
        c.borrow_mut().poll(&ui);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!c.borrow().state.statistics.loading);
    assert!(ui.get_statistics_failed());
    assert_eq!(c.borrow().state.position, 42.0);
    snapshot(&ui, "query-error");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("ALTER TABLE saved_history RENAME TO listening_history")
        .unwrap();
    ui.invoke_statistics_refresh();
    ready(&c, &ui);
    assert!(!c.borrow().state.statistics.failed);
    assert_eq!(ui.get_statistics_summary().total, "37");

    // Rankings shrinking after refresh clamp the keyboard selection.
    open(&c, &ui, StatisticsPage::Tracks);
    for _ in 0..8 {
        c.borrow_mut().execute(&ui, Action::SelectNext);
    }
    assert_eq!(ui.get_selected(), 7);
    {
        let mut controller = c.borrow_mut();
        let paths: Vec<_> = controller
            .state
            .tracks
            .iter()
            .map(|track| track.path.clone())
            .collect();
        for path in paths {
            controller.state.mark_track_available(&path, false);
        }
    }
    ui.invoke_statistics_refresh();
    ready(&c, &ui);
    assert_eq!(ui.get_statistics_items().row_count(), 2);
    assert!(ui.get_selected() < 2);
    c.borrow_mut().execute(&ui, Action::Home);
    assert!(!ui.get_statistics_page());
    assert_eq!(c.borrow().state.queue.order, [0, 1]);
    c.borrow_mut().execute(&ui, Action::Right);
    c.borrow_mut().execute(&ui, Action::Right);
    assert_eq!(ui.get_home_page(), 1);
    snapshot(&ui, "home-entry");
    click(&ui, 60.0, 201.0);
    assert_eq!(
        c.borrow().state.navigation.current().screen,
        Screen::Statistics(StatisticsPage::Home)
    );
    c.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(ui.get_home_page(), 1);
    assert_eq!(ui.get_selected(), 2);

    // Finally exercise both detail Play actions through the controller. The
    // preceding browsing/failure checks have already verified playback isolation.
    {
        let mut controller = c.borrow_mut();
        controller.state.language = Language::English;
        let paths: Vec<_> = controller
            .state
            .tracks
            .iter()
            .map(|track| track.path.clone())
            .collect();
        for path in paths {
            controller.state.mark_track_available(&path, true);
        }
    }
    open(&c, &ui, StatisticsPage::Home);
    ui.invoke_statistics_refresh();
    ready(&c, &ui);
    for page in [StatisticsPage::Artists, StatisticsPage::Albums] {
        open_detail(&c, &ui, page);
        activate_icon(&ui, 10);
        assert_eq!(
            c.borrow().state.navigation.current().screen,
            Screen::NowPlaying
        );
        assert_eq!(c.borrow().state.current, Some(1));
        assert_eq!(c.borrow().state.queue.order, [1, 2, 3, 4, 5, 6, 7]);
        c.borrow_mut().execute(&ui, Action::Back);
        assert!(matches!(
            c.borrow().state.navigation.current().screen,
            Screen::Songs(Filter::Statistics { .. })
        ));
    }
    ui.hide().unwrap();
}
