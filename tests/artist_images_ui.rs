#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        i18n::{Language, has_translation, translate},
        navigation::{Screen, StatisticsPage},
        state::{Effect, State, Target},
    },
    audio::{PlaybackStatus, Repeat},
    library::artist_images::{JobSnapshot, JobState, QueueSnapshot, normalized_artist_key},
    ui::{AppWindow, StatisticsItem, StatisticsSummary},
};
use slint::{ComponentHandle, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::{
    cell::{Cell, RefCell},
    path::Path,
    rc::Rc,
};

fn job(artist: &str, state: JobState, error: Option<&str>) -> JobSnapshot {
    JobSnapshot {
        key: normalized_artist_key(artist),
        artist: artist.into(),
        albums: vec!["An album — アルバム".into()],
        state,
        attempts: 2,
        next_retry: 1_800_000_000,
        error: error.map(str::to_owned),
        credit: None,
        asset: None,
    }
}

fn queue() -> QueueSnapshot {
    QueueSnapshot {
        jobs: vec![
            job("Ready Artist", JobState::Ready, None),
            job(
                "待機中のアーティスト — Long artist credit",
                JobState::Pending,
                None,
            ),
            job(
                "Z Failed Artist",
                JobState::Failed,
                Some("Unrecognized provider response — retained for retry"),
            ),
            job("Running Artist", JobState::Running, None),
            job(
                "A Failed Artist",
                JobState::Failed,
                Some("No exact artist identity found; queued for retry"),
            ),
        ],
        pending: 1,
        running: 1,
        failed: 2,
        ready: 1,
    }
}

fn open_queue(state: &mut State) {
    state
        .navigation
        .open(Screen::Statistics(StatisticsPage::ArtistImages));
}

#[test]
fn image_queue_rows_prioritize_unfinished_jobs_without_losing_failure_context() {
    let snapshot = queue();
    let mut state = State {
        artist_image_queue: snapshot.clone(),
        ..Default::default()
    };
    open_queue(&mut state);
    let rows = state.statistics_entries();
    assert!(matches!(rows[0].row.target, Target::RetryArtistImages));
    assert_eq!(
        rows.iter()
            .skip(1)
            .map(|entry| entry.row.title.as_str())
            .collect::<Vec<_>>(),
        [
            "Running Artist",
            "A Failed Artist",
            "Z Failed Artist",
            "待機中のアーティスト — Long artist credit",
            "Ready Artist",
        ]
    );
    assert_eq!(
        rows.iter()
            .skip(1)
            .map(|entry| entry.value.as_str())
            .collect::<Vec<_>>(),
        ["Finding…", "Retry later", "Retry later", "Queued", "Ready"]
    );
    assert_eq!(
        rows[3].row.subtitle,
        "Image lookup failed; queued for retry"
    );
    assert_eq!(rows[4].row.subtitle, "An album — アルバム");
    assert!(
        rows.iter()
            .skip(1)
            .all(|entry| matches!(entry.row.target, Target::None))
    );
    assert_eq!(
        state.artist_image_queue, snapshot,
        "projection must not mutate durable job metadata"
    );
    assert!(state.statistics_note().contains("4 queued · 1 ready"));
}

#[test]
fn image_queue_empty_error_and_retry_do_not_change_audio_or_discard_jobs() {
    let mut state = State::default();
    state.queue.set(vec![1, 0, 2], 0, false);
    state.current = Some(0);
    state.position = 73.5;
    state.status = PlaybackStatus::Playing;
    state.paused = false;
    state.repeat = Repeat::All;
    state.artist_image_error = Some("Cannot read artist image cache".into());
    open_queue(&mut state);
    assert_eq!(state.rows().len(), 2);
    assert_eq!(state.rows()[1].title, "No artists queued");
    assert_eq!(
        state.statistics_note(),
        "Image queue unavailable. Retry later."
    );
    let effects = state.apply(Action::Activate);
    assert!(matches!(effects.as_slice(), [Effect::RetryArtistImages]));
    assert_eq!(state.queue.order, [1, 0, 2]);
    assert_eq!(state.queue.position, 1);
    assert_eq!(state.current, Some(0));
    assert_eq!(state.position, 73.5);
    assert_eq!(state.status, PlaybackStatus::Playing);
    assert!(!state.paused);
    assert_eq!(state.repeat, Repeat::All);
    assert_eq!(
        state.artist_image_error.as_deref(),
        Some("Cannot read artist image cache")
    );

    state.artist_image_queue = queue();
    let retained = state.artist_image_queue.clone();
    state.navigation.select(3);
    assert!(
        state.apply(Action::Activate).is_empty(),
        "failed job rows never start playback"
    );
    state.apply(Action::Back);
    open_queue(&mut state);
    assert_eq!(
        state.artist_image_queue, retained,
        "reopening preserves pending and failed work"
    );
    assert_eq!(
        state.artist_image_error.as_deref(),
        Some("Cannot read artist image cache")
    );
}

#[test]
fn artist_images_append_to_statistics_navigation_and_all_copy_is_localized() {
    let mut state = State::default();
    state
        .navigation
        .open(Screen::Statistics(StatisticsPage::Home));
    let rows = state.rows();
    assert_eq!(
        rows.iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        [
            "Overview",
            "Top Artists",
            "Top Albums",
            "Top Tracks",
            "Recently Played",
            "Daily Plays",
            "Time Range",
            "Refresh",
            "Artist Images",
        ]
    );
    state.navigation.select(8);
    assert!(state.apply(Action::Activate).is_empty());
    assert_eq!(state.navigation.current().screen.statistics_kind(), 10);
    state.apply(Action::Back);
    assert_eq!(
        state.navigation.current().screen,
        Screen::Statistics(StatisticsPage::Home)
    );
    assert_eq!(state.navigation.selected(), 8);
    for text in [
        "Artist Images",
        "Background image queue",
        "Retry Failed Images",
        "Pending tasks are saved automatically",
        "No artists queued",
        "Queued",
        "Finding…",
        "Retry later",
        "Ready",
        "Photo credits",
        "Photo: {0} · {1}",
        "Photos: {0} queued · {1} ready. Failed jobs retry.",
        "Image queue unavailable. Retry later.",
        "Image lookup failed; queued for retry",
        "No exact artist identity found; queued for retry",
    ] {
        assert!(has_translation(text), "missing translation for {text}");
    }
    state.artist_image_queue = queue();
    open_queue(&mut state);
    for language in Language::ALL {
        state.language = language;
        let entries = state.statistics_entries();
        let failure = entries
            .iter()
            .find(|entry| entry.row.title == "A Failed Artist")
            .unwrap();
        assert_eq!(
            failure.row.subtitle,
            translate(language, "No exact artist identity found; queued for retry")
        );
        if language != Language::English {
            assert_ne!(
                failure.row.subtitle,
                "No exact artist identity found; queued for retry"
            );
        }
    }
}

fn point(ui: &AppWindow, x: f32, y: f32) -> slint::LogicalPosition {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let content = ui.get_content_height();
    let body = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    slint::LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        size.height - content
            + (content - body * scale) / 2.0
            + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    )
}

fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = point(ui, x, y);
    let button = slint::platform::PointerEventButton::Left;
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::PointerPressed { position, button });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::PointerReleased { position, button });
}

fn screenshot(ui: &AppWindow, name: &str) -> SharedPixelBuffer<Rgba8Pixel> {
    slint::platform::update_timers_and_animations();
    let pixels = ui.window().take_snapshot().unwrap();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa/artist-images");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        pixels.as_bytes(),
        pixels.width(),
        pixels.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
    // The divider must stay above the two-line note. An implicit centered y
    // previously put a full-width line through its text on every page.
    let divider = |top: f32, bottom: f32| {
        let first = point(ui, 2.0, top);
        let last = point(ui, 5.0, bottom);
        let scale = ui.window().scale_factor();
        ((first.y * scale).floor() as u32..(last.y * scale).ceil() as u32).any(|y| {
            ((first.x * scale).floor() as u32..(last.x * scale).ceil() as u32).any(|x| {
                let offset = ((y * pixels.width() + x) * 4) as usize;
                let pixel = &pixels.as_bytes()[offset..offset + 4];
                pixel[0] < 235 && pixel[0] > 190 && pixel[1] > pixel[0] && pixel[2] > pixel[1]
            })
        })
    };
    assert!(
        // Device dimensions use fractional logical heights, while snapshots
        // round the window to whole physical pixels. Allow that edge rounding.
        divider(405.0, 409.0),
        "{name}: footer divider belongs at its top"
    );
    assert!(
        !divider(411.0, 425.0),
        "{name}: divider must not cross the note"
    );
    pixels
}

#[test]
fn queue_and_clickable_photo_credit_render_in_three_languages_and_both_modes_offline() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    Settings {
        device_mode: false,
        smooth_slides: false,
        ..Default::default()
    }
    .save(&directory.path().join("settings.json"))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    let credits = Rc::new(Cell::new(0));
    let received = credits.clone();
    // Count the real pointer callback without opening an external browser.
    ui.on_statistics_photo_credits(move || received.set(received.get() + 1));
    for language in Language::ALL {
        for device in [false, true] {
            {
                let mut c = controller.borrow_mut();
                c.state.navigation.open(Screen::Home);
                c.execute(&ui, Action::FinishSearchEditing);
                c.state.language = language;
                c.state.artist_image_queue = queue();
                c.state.artist_image_error = None;
                c.state.status = PlaybackStatus::Playing;
                c.state.current = Some(0);
                c.state.position = 46.0;
                c.state.paused = false;
                c.state.queue.set(vec![0, 1], 0, false);
                open_queue(&mut c.state);
                c.execute(
                    &ui,
                    if device {
                        Action::DeviceMode
                    } else {
                        Action::ScreenMode
                    },
                );
            }
            assert_eq!(ui.get_statistics_kind(), 10);
            assert_eq!(ui.get_statistics_items().row_count(), 6);
            assert_eq!(
                ui.get_list_header_height(),
                0.0,
                "queue has no range selector"
            );
            assert!((ui.get_list_row_height() - 48.0 * ui.get_display_scale()).abs() < 0.1);
            assert_eq!(
                ui.get_statistics_items()
                    .row_data(0)
                    .unwrap()
                    .title
                    .as_str(),
                translate(language, "Retry Failed Images")
            );
            assert_eq!(
                ui.get_statistics_items()
                    .row_data(1)
                    .unwrap()
                    .value
                    .as_str(),
                translate(language, "Finding…")
            );
            assert!(ui.get_statistics_items().row_data(0).unwrap().chevron);
            assert!(!ui.get_statistics_items().row_data(2).unwrap().chevron);
            screenshot(&ui, &format!("queue-{}-{device}", language.code()));
            assert_eq!(controller.borrow().state.position, 46.0);
            assert_eq!(controller.borrow().state.status, PlaybackStatus::Playing);
            assert_eq!(controller.borrow().state.queue.order, [0, 1]);

            let mut photo = SharedPixelBuffer::<Rgba8Pixel>::new(128, 128);
            for pixel in photo.make_mut_slice() {
                *pixel = Rgba8Pixel {
                    r: 43,
                    g: 104,
                    b: 164,
                    a: 255,
                };
            }
            let credit = translate(language, "Photo: {0} · {1}")
                .replace("{0}", "Long Photographer Name / 撮影者 — Wikimedia Commons")
                .replace("{1}", "CC BY-SA 4.0");
            let mut summary = StatisticsSummary {
                detail_title: "Artist — 藝人".into(),
                detail_subtitle: translate(language, "{0} plays · {1} tracks")
                    .replace("{0}", "312")
                    .replace("{1}", "28")
                    .into(),
                detail_artwork: slint::Image::from_rgba8(photo),
                photo_credit: credit.into(),
                photo_url: "https://commons.wikimedia.org/wiki/File:Artist_fixture.jpg".into(),
                footer: format!(
                    "{} · {}",
                    translate(language, "All Time"),
                    translate(language, "Latest 1,000 plays; date totals may be partial.")
                )
                .into(),
                ..ui.get_statistics_summary()
            };
            ui.set_screen_title(translate(language, "Artist Details").into());
            ui.set_statistics_kind(8);
            ui.set_statistics_items(ModelRc::new(VecModel::from(
                [
                    ("Play Artist", 10),
                    ("Albums", 11),
                    ("Top Tracks", 12),
                    ("See All Tracks", 13),
                ]
                .into_iter()
                .map(|(title, icon)| StatisticsItem {
                    title: translate(language, title).into(),
                    icon,
                    chevron: true,
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
            )));
            ui.set_statistics_summary(summary.clone());
            let credited = screenshot(&ui, &format!("credit-{}-{device}", language.code()));
            let before = credits.get();
            click(&ui, 218.0, 72.0);
            assert_eq!(
                credits.get(),
                before + 1,
                "visible credit uses the real mouse callback"
            );
            summary.photo_credit = "".into();
            ui.set_statistics_summary(summary);
            slint::platform::update_timers_and_animations();
            let plain = ui.window().take_snapshot().unwrap();
            assert_ne!(
                credited.as_bytes(),
                plain.as_bytes(),
                "attribution must be visibly rendered"
            );
            click(&ui, 218.0, 72.0);
            assert_eq!(
                credits.get(),
                before + 1,
                "empty credit has no invisible hit target"
            );
        }
    }
    {
        let mut c = controller.borrow_mut();
        c.state.navigation.open(Screen::Home);
        c.execute(&ui, Action::FinishSearchEditing);
        c.state.language = Language::English;
        c.state.artist_image_queue = QueueSnapshot::default();
        c.state.artist_image_error = Some("Cannot read artist image cache".into());
        open_queue(&mut c.state);
        c.execute(&ui, Action::ScreenMode);
    }
    assert_eq!(ui.get_statistics_kind(), 10);
    assert_eq!(ui.get_statistics_items().row_count(), 2);
    assert_eq!(
        ui.get_statistics_items().row_data(1).unwrap().title,
        "No artists queued"
    );
    assert_eq!(
        ui.get_statistics_summary().footer,
        "Image queue unavailable. Retry later."
    );
    screenshot(&ui, "empty-error-queue");
    ui.hide().unwrap();
}
