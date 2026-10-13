use nano_music_player::{
    app::{
        action::Action,
        controller::Controller,
        navigation::{Group, Screen},
        state::{State, Target},
    },
    library::{Track, database::Database},
    ui::AppWindow,
};
use slint::{ComponentHandle, Model};
use std::time::{Duration, Instant};

fn platform() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
}

fn disable_file_monitor(directory: &std::path::Path) {
    std::fs::write(
        directory.join("settings.json"),
        r#"{"monitor_deleted_files":false}"#,
    )
    .unwrap();
}

fn load_fixture(tracks: &[Track]) -> (tempfile::TempDir, Controller, AppWindow) {
    let directory = tempfile::tempdir().unwrap();
    disable_file_monitor(directory.path());
    let db = Database::open(&directory.path().join("library.sqlite3")).unwrap();
    for track in tracks {
        db.save(track, 0).unwrap();
    }
    drop(db);
    let ui = AppWindow::new().unwrap();
    let mut controller = Controller::new(directory.path().to_path_buf());
    controller.initialize(&ui);
    let deadline = Instant::now() + Duration::from_secs(30);
    while controller.state.tracks.len() != tracks.len() {
        assert!(Instant::now() < deadline, "library preparation timed out");
        std::thread::sleep(Duration::from_millis(5));
        controller.poll(&ui);
    }
    (directory, controller, ui)
}

#[test]
fn library_publishes_immediate_covers_and_sharpening_preserves_model_and_selection() {
    platform();
    let tracks: Vec<_> = (0..20)
        .map(|id| {
            let mut artwork = std::io::Cursor::new(Vec::new());
            image::RgbImage::from_pixel(512, 256, image::Rgb([id as u8 * 10, 100, 180]))
                .write_to(&mut artwork, image::ImageFormat::Png)
                .unwrap();
            Track {
                path: format!("/nonexistent/nano-album-{id}.mp3").into(),
                title: format!("Song {id:02}"),
                artist: format!("Artist {id:02}"),
                album: format!("Album {id:02}"),
                genre: "Test".into(),
                number: 1,
                duration: 60.0,
                artwork: artwork.into_inner(),
                lyrics: Default::default(),
            }
        })
        .collect();
    let (directory, mut controller, ui) = load_fixture(&tracks);
    controller
        .state
        .navigation
        .open(Screen::Groups(Group::Albums));
    controller.execute(&ui, Action::FinishSearchEditing);
    assert_eq!(ui.get_rows().row_count(), tracks.len() + 1);
    assert!(
        ui.get_rows()
            .iter()
            .filter(|row| row.show_artwork)
            .all(|row| row.artwork.size().width == 96)
    );
    // Changing DPI/zoom requests sharper visible covers without rebuilding rows.
    controller.state.zoom = 3.0;
    controller.execute(&ui, Action::ZoomIn);
    let model = ui.get_rows();
    controller.state.navigation.select(15);
    controller.execute(&ui, Action::FinishSearchEditing);
    let scroll = ui.get_list_scroll_y();
    let deadline = Instant::now() + Duration::from_secs(5);
    while ui.get_rows().row_data(15).unwrap().artwork.size().width <= 96 {
        assert!(Instant::now() < deadline, "visible cover did not sharpen");
        std::thread::sleep(Duration::from_millis(5));
        controller.poll(&ui);
    }
    assert!(
        ui.get_rows() == model,
        "decode completion must update existing rows"
    );
    assert_eq!(controller.state.navigation.selected(), 15);
    assert_eq!(ui.get_list_scroll_y(), scroll);
    // Artists have ready covers too, regardless of which album thumbnails were visited.
    controller
        .state
        .navigation
        .open(Screen::Groups(Group::Artists));
    controller.execute(&ui, Action::FinishSearchEditing);
    assert!(
        ui.get_rows()
            .iter()
            .filter(|row| row.show_artwork)
            .all(|row| row.artwork.size().width > 0)
    );
    controller.state.start(0); // Discard audio effect; don't touch the live audio device.
    controller.execute(&ui, Action::NowPlaying);
    assert!(
        ui.get_artwork().size().width > 0,
        "first song reuses the prepared preview"
    );
    assert_eq!(
        std::fs::read_dir(directory.path().join("cover-previews-v1"))
            .unwrap()
            .count(),
        20
    );
}

#[test]
fn compilation_groups_keep_subtitles_artist_filters_and_representative_order() {
    let mut state = State {
        tracks: [
            ("Zulu", "B", "/z", true),
            ("Alpha", "A", "/a", false),
            ("Beta", "A", "/b", true),
            ("Beta", "A", "/aa", true),
        ]
        .into_iter()
        .map(|(title, artist, path, has_art)| Track {
            path: path.into(),
            title: title.into(),
            artist: artist.into(),
            album: "Compilation".into(),
            genre: "Test".into(),
            number: 1,
            duration: 60.0,
            artwork: if has_art { vec![1] } else { Vec::new() },
            lyrics: Default::default(),
        })
        .collect(),
        ..State::default()
    };
    state.navigation.open(Screen::Groups(Group::Albums));
    let rows = state.rows();
    assert_eq!(rows.len(), state.selection_count());
    assert_eq!(rows[1].subtitle, "A, B");
    assert_eq!(rows[1].artwork_id, Some(3));
    state.navigation.open(Screen::Artist("B".into()));
    let rows = state.rows();
    assert_eq!(rows[1].subtitle, "B");
    assert_eq!(rows[1].artwork_id, Some(0));
    assert!(matches!(&rows[1].target, Target::Screen(Screen::Songs(
        nano_music_player::app::navigation::Filter::Album { artist: Some(artist), .. }
    )) if artist == "B"));
}

#[test]
#[ignore = "manual large-library grouping profile"]
fn profile_large_group_construction() {
    let mut state = State {
        tracks: (0..10_000)
            .map(|id| Track {
                path: format!("/track-{id}.mp3").into(),
                title: format!("Song {id}"),
                artist: format!("Artist {}", id % 100),
                album: format!("Album {id}"),
                genre: "Test".into(),
                number: 1,
                duration: 60.0,
                artwork: vec![1],
                lyrics: Default::default(),
            })
            .collect(),
        ..State::default()
    };
    for screen in [
        Screen::Groups(Group::Albums),
        Screen::Groups(Group::Artists),
        Screen::Artist("Artist 0".into()),
    ] {
        state.navigation.open(screen.clone());
        let started = Instant::now();
        let rows = state.rows();
        println!(
            "{screen:?}: {} rows from 10000 tracks in {:.2} ms",
            rows.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        assert_eq!(rows.len(), state.selection_count());
    }
}

#[test]
#[ignore = "read-only profile using NANO_PROFILE_LIBRARY"]
fn profile_album_entry() {
    platform();
    let db = rusqlite::Connection::open_with_flags(
        std::env::var_os("NANO_PROFILE_LIBRARY").expect("set NANO_PROFILE_LIBRARY"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut statement = db
        .prepare("SELECT metadata FROM tracks ORDER BY path")
        .unwrap();
    let tracks: Vec<Track> = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|row| serde_json::from_slice(&row.unwrap()).unwrap())
        .collect();
    let started = Instant::now();
    let (_directory, mut controller, ui) = load_fixture(&tracks);
    println!(
        "startup fixture preparation: {:.2} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    let preview_bytes: u64 = std::fs::read_dir(_directory.path().join("cover-previews-v1"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    println!(
        "all unique preview files: {:.1} KiB",
        preview_bytes as f64 / 1024.0
    );
    for label in ["cold", "warm"] {
        controller
            .state
            .navigation
            .open(Screen::Groups(Group::Albums));
        let started = Instant::now();
        let rows = controller.state.rows();
        let grouping = started.elapsed();
        let started = Instant::now();
        controller.execute(&ui, Action::FinishSearchEditing);
        let entry = started.elapsed();
        let started = Instant::now();
        let _snapshot = ui.window().take_snapshot().unwrap();
        let render = started.elapsed();
        println!(
            "{label}: {} albums; grouping={:.2} ms entry={:.2} ms first render={:.2} ms",
            rows.len(),
            grouping.as_secs_f64() * 1000.0,
            entry.as_secs_f64() * 1000.0,
            render.as_secs_f64() * 1000.0
        );
        assert_eq!(ui.get_rows().row_count(), rows.len());
        assert!(
            ui.get_rows()
                .iter()
                .filter(|row| row.show_artwork)
                .all(|row| row.artwork.size().width > 0),
            "all first-frame album covers must be ready"
        );
        let deadline = Instant::now() + std::time::Duration::from_secs(15);
        while ui
            .get_rows()
            .row_data(1)
            .is_some_and(|row| row.artwork.size().width == 0)
            && Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
            controller.poll(&ui);
        }
        controller.execute(&ui, Action::Home);
    }
}
