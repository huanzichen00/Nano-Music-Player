use nano_music_player::{
    app::{action::Action, controller::Controller},
    library::Track,
    ui::AppWindow,
};
use slint::{ComponentHandle, Model};
use std::time::{Duration, Instant};

fn initialize_platform() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
}

fn wait_for_cover(controller: &mut Controller, ui: &AppWindow) {
    let resolution = (240.0 * ui.get_display_scale() * ui.window().scale_factor()).ceil() as u32;
    let deadline = Instant::now() + Duration::from_secs(5);
    while ui
        .get_artwork()
        .size()
        .width
        .max(ui.get_artwork().size().height)
        < resolution
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
        controller.poll(ui);
    }
    assert_eq!(
        ui.get_artwork()
            .size()
            .width
            .max(ui.get_artwork().size().height),
        resolution
    );
}

#[test]
fn song_metadata_is_immediate_and_background_covers_follow_the_current_track() {
    initialize_platform();
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let mut controller = Controller::new(directory.path().to_path_buf());
    controller.initialize(&ui);
    controller.state.merge_library(
        [(128, 64), (64, 128)]
            .into_iter()
            .enumerate()
            .map(|(id, (width, height))| {
                let mut artwork = std::io::Cursor::new(Vec::new());
                image::RgbaImage::new(width, height)
                    .write_to(&mut artwork, image::ImageFormat::Png)
                    .unwrap();
                Track {
                    path: directory.path().join(format!("{id}.mp3")),
                    title: format!("Song {id}"),
                    artist: "Artist".into(),
                    album: "Album".into(),
                    genre: "Test".into(),
                    number: id as u32,
                    duration: 60.0,
                    artwork: artwork.into_inner(),
                    lyrics: nano_music_player::library::lyrics::Lyrics::parse("[00:00]Lyrics"),
                }
            })
            .collect(),
    );
    // Discard audio effects to leave the shared audio device untouched.
    controller.state.start(0);
    controller.execute(&ui, Action::NowPlaying);
    assert_eq!(ui.get_track_title(), "Song 0");
    assert!(ui.get_lyrics_visible());
    assert_eq!(ui.get_artwork().size().width, 0);
    controller.state.start(1);
    controller.execute(&ui, Action::NowPlaying);
    assert_eq!(ui.get_track_title(), "Song 1");
    wait_for_cover(&mut controller, &ui);
    let second_cover = ui.get_artwork().size();
    assert_eq!(second_cover.height, second_cover.width * 2);

    controller.state.start(0);
    controller.execute(&ui, Action::NowPlaying);
    wait_for_cover(&mut controller, &ui);
    let first_cover = ui.get_artwork().size();
    assert_eq!(first_cover.width, first_cover.height * 2);
    controller.state.start(1);
    controller.execute(&ui, Action::NowPlaying);
    assert_eq!(
        ui.get_artwork().size(),
        second_cover,
        "cached covers must be available immediately"
    );

    // Clearing the current track removes its cover without discarding the cache.
    controller.state.current = None;
    controller.execute(&ui, Action::NowPlaying);
    assert_eq!(ui.get_artwork().size().width, 0);
    controller.state.start(1);
    controller.execute(&ui, Action::NowPlaying);
    assert_eq!(ui.get_artwork().size(), second_cover);

    // Album-list artwork is immediately reusable for another song with the same cover.
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(512, 512, image::Rgb([180, 40, 80]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let tracks = ["Zebra", "Aardvark"]
        .into_iter()
        .enumerate()
        .map(|(id, title)| {
            let mut track = controller.state.tracks[0].clone();
            track.path = directory.path().join(format!("preview-{id}.mp3"));
            track.title = title.into();
            track.album = "Preview Album".into();
            track.artwork = bytes.get_ref().clone();
            track
        })
        .collect();
    controller.state.merge_library(tracks);
    controller
        .state
        .navigation
        .open(nano_music_player::app::navigation::Screen::Groups(
            nano_music_player::app::navigation::Group::Albums,
        ));
    controller.execute(&ui, Action::SelectPrevious);
    // This fixture injects metadata directly, bypassing startup preview preparation.
    let deadline = Instant::now() + Duration::from_secs(5);
    while ui
        .get_rows()
        .iter()
        .any(|row| row.artwork.size().width == 0)
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
        controller.poll(&ui);
    }
    controller.state.start(2);
    controller.execute(&ui, Action::NowPlaying);
    let preview_size = ui.get_artwork().size().width;
    assert!(
        preview_size > 0,
        "list thumbnail should appear before polling"
    );
    assert!(
        preview_size < second_cover.height,
        "first cover should be a smaller preview"
    );
    wait_for_cover(&mut controller, &ui);
    assert!(
        ui.get_artwork().size().width > preview_size,
        "preview is upgraded in place"
    );
}

// Reads metadata only; no playback, scanning, or writes to the user's library.
// NANO_PROFILE_LIBRARY=/path/to/library.sqlite3 cargo test --test song_entry_latency -- --ignored --nocapture
#[test]
#[ignore = "manual latency profile using an existing library"]
fn profile_song_entry_with_library_covers() {
    initialize_platform();
    let path = std::env::var_os("NANO_PROFILE_LIBRARY").expect("set NANO_PROFILE_LIBRARY");
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut statement = db.prepare("SELECT metadata FROM tracks").unwrap();
    let mut tracks: Vec<Track> = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|row| serde_json::from_slice(&row.unwrap()).unwrap())
        .collect();
    tracks.sort_by_key(|track| std::cmp::Reverse(track.artwork.len()));
    tracks.truncate(8);
    assert!(!tracks.is_empty());
    let directory = tempfile::tempdir().unwrap();
    let ui = AppWindow::new().unwrap();
    let mut controller = Controller::new(directory.path().to_path_buf());
    controller.initialize(&ui);
    controller.state.merge_library(tracks);
    let mut first = Vec::new();
    let mut repeat = Vec::new();
    let mut previews = Vec::new();
    let mut sharpened = Vec::new();
    for (pass, timings) in [&mut first, &mut repeat].into_iter().enumerate() {
        for id in 0..controller.state.tracks.len() {
            let started = Instant::now();
            // Discard the audio effect so profiling does not play music.
            controller.state.start(id);
            controller.execute(&ui, Action::NowPlaying);
            timings.push(started.elapsed().as_secs_f64() * 1_000.0);
            let deadline = Instant::now() + Duration::from_secs(10);
            while ui.get_artwork().size().width == 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
                controller.poll(&ui);
            }
            assert!(ui.get_artwork().size().width > 0);
            if pass == 0 {
                previews.push(started.elapsed().as_secs_f64() * 1_000.0);
            }
            wait_for_cover(&mut controller, &ui);
            if pass == 0 {
                sharpened.push(started.elapsed().as_secs_f64() * 1_000.0);
            }
        }
    }
    for (label, mut samples) in [
        ("first entry", first),
        ("repeat entry", repeat),
        ("first preview", previews),
        ("finished cover", sharpened),
    ] {
        samples.sort_by(f64::total_cmp);
        println!(
            "{label}: min={:.2} ms median={:.2} ms max={:.2} ms ({} covers)",
            samples[0],
            samples[samples.len() / 2],
            samples[samples.len() - 1],
            samples.len()
        );
    }
}
