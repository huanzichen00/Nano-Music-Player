#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        i18n::{Language, has_translation},
        navigation::{Filter, Screen, StatisticsPage},
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
fn click(ui: &AppWindow, x: f32, y: f32) {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let content = ui.get_content_height();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    let top = size.height - content + (content - body * scale) / 2.0;
    let position = slint::LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        top + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    );
    let button = slint::platform::PointerEventButton::Left;
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::PointerPressed { position, button });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::PointerReleased { position, button });
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
        matches!(&state.navigation.current().screen, Screen::Songs(Filter::Statistics {paths, ..}) if paths.len() == 2)
    );
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
    assert_eq!(ui.get_rows().row_count(), 8);
    assert_eq!(
        c.borrow()
            .state
            .statistics
            .snapshot
            .as_ref()
            .unwrap()
            .total_plays,
        0
    );
    snapshot(&ui, "empty-home");
    click(&ui, 80.0, 152.0); // Banner shifts pointer hit testing down by 64px.
    assert_eq!(
        c.borrow().state.navigation.current().screen,
        Screen::Statistics(StatisticsPage::Overview)
    );
    c.borrow_mut().execute(&ui, Action::Back);
    open(&c, &ui, StatisticsPage::Tracks);
    assert_eq!(ui.get_rows().row_data(0).unwrap().title, "No plays yet");
    snapshot(&ui, "empty");
    open(&c, &ui, StatisticsPage::Daily);
    assert_eq!(ui.get_rows().row_count(), 30);
    assert!(ui.get_rows().iter().all(|r| r.subtitle == "0 plays"));

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
    for (i, t) in tracks.iter().enumerate() {
        for _ in 0..=i {
            store.record_play(&t.path, now, 30).unwrap();
        }
    }
    let removed = tmp.path().join("removed.wav");
    store.record_play(&removed, now, 30).unwrap();
    {
        let mut c = c.borrow_mut();
        c.state.merge_library(tracks);
        c.state.notice.clear();
        c.state.current = Some(0);
        c.state.status = PlaybackStatus::Playing;
        c.state.paused = false;
        c.state.position = 42.0;
        c.state.queue.set(vec![0, 1], 0, false);
    }
    open(&c, &ui, StatisticsPage::Home);
    ui.invoke_item_clicked(7); // Explicit refresh sees plays made via another connection.
    open(&c, &ui, StatisticsPage::Overview); // Refresh must survive immediate subpage navigation.
    ready(&c, &ui);
    assert_eq!(
        c.borrow()
            .state
            .statistics
            .snapshot
            .as_ref()
            .unwrap()
            .total_plays,
        37
    );
    assert_eq!(ui.get_rows().row_data(0).unwrap().subtitle, "37 plays");
    snapshot(&ui, "overview");
    assert_eq!(c.borrow().state.status, PlaybackStatus::Playing);
    assert_eq!(c.borrow().state.position, 42.0);
    assert_eq!(c.borrow().state.queue.order, vec![0, 1]);
    c.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(
        c.borrow().state.navigation.current().screen,
        Screen::Statistics(StatisticsPage::Home)
    );
    assert_eq!(ui.get_selected(), 7);
    snapshot(&ui, "home");

    ui.invoke_item_clicked(6); // Range.
    ui.invoke_item_clicked(1); // Last 7 Days.
    ready(&c, &ui);
    assert_eq!(
        c.borrow().state.statistics.range,
        StatisticsRange::Last7Days
    );
    assert!(ui.get_statistics_note().contains("1,000"));
    open(&c, &ui, StatisticsPage::Recent);
    let unavailable = ui
        .get_rows()
        .iter()
        .position(|r| r.subtitle.contains("Unavailable"))
        .unwrap();
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
                    let mut c = c.borrow_mut();
                    c.state.zoom = zoom;
                    c.execute(
                        &ui,
                        if device {
                            Action::DeviceMode
                        } else {
                            Action::ScreenMode
                        },
                    );
                }
                open(&c, &ui, StatisticsPage::Tracks);
                assert!(
                    ui.get_rows().row_data(0).unwrap().subtitle.starts_with('8'),
                    "play count remains visible before long artist metadata"
                );
                assert!(ui.get_list_viewport_height() > 280.0 * ui.get_display_scale());
                snapshot(&ui, &format!("tracks-{}-{device}-{zoom}", language.code()));
                open(&c, &ui, StatisticsPage::Daily);
                for _ in 0..30 {
                    c.borrow_mut().execute(&ui, Action::SelectNext);
                }
                assert_eq!(ui.get_selected(), 29);
                assert!(ui.get_list_scroll_y() < 0.0);
                assert!(
                    (30.0 * ui.get_list_row_height() + ui.get_list_scroll_y())
                        <= ui.get_list_viewport_height() + 1.0,
                    "last row must remain visible to keyboard users"
                );
                snapshot(&ui, &format!("daily-{}-{device}-{zoom}", language.code()));
                c.borrow_mut().execute(&ui, Action::Back);
            }
        }
    }
    // Database failure reaches the UI, does not stop playback, and can recover.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("ALTER TABLE listening_history RENAME TO saved_history")
        .unwrap();
    open(&c, &ui, StatisticsPage::Home);
    ui.invoke_item_clicked(7);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !c.borrow().state.statistics.failed {
        c.borrow_mut().poll(&ui);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!c.borrow().state.statistics.loading);
    assert_eq!(c.borrow().state.position, 42.0);
    snapshot(&ui, "query-error");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("ALTER TABLE saved_history RENAME TO listening_history")
        .unwrap();
    ui.invoke_item_clicked(7);
    ready(&c, &ui);
    assert!(!c.borrow().state.statistics.failed);
    assert_eq!(
        c.borrow()
            .state
            .statistics
            .snapshot
            .as_ref()
            .unwrap()
            .total_plays,
        37
    );
    // Rankings shrinking after a refresh must clamp keyboard selection.
    open(&c, &ui, StatisticsPage::Tracks);
    for _ in 0..8 {
        c.borrow_mut().execute(&ui, Action::SelectNext);
    }
    assert_eq!(ui.get_selected(), 7);
    {
        let mut c = c.borrow_mut();
        let paths: Vec<_> = c.state.tracks.iter().map(|t| t.path.clone()).collect();
        for path in paths {
            c.state.mark_track_available(&path, false);
        }
    }
    open(&c, &ui, StatisticsPage::Home);
    ui.invoke_item_clicked(7);
    open(&c, &ui, StatisticsPage::Tracks);
    ready(&c, &ui);
    assert_eq!(ui.get_rows().row_count(), 2);
    assert!(ui.get_selected() < 2);
    c.borrow_mut().execute(&ui, Action::Home);
    assert!(!ui.get_statistics_page());
    assert_eq!(c.borrow().state.queue.order, vec![0, 1]);
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
    ui.hide().unwrap();
}
