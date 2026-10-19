//! Real mobile controller/storage workflows, without a network provider or audio output.
use super::{Controller, connect_translations, navigation::Page, storage::Storage};
use crate::{
    library::{lyrics::Lyrics, playlists, statistics::StatisticsStore},
    ui::MobileWindow,
};
use slint::ComponentHandle;
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn pump_until(
    controller: &mut Controller,
    ui: &MobileWindow,
    ready: impl Fn(&Controller, &MobileWindow) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready(controller, ui) {
        controller.poll(ui);
        assert!(
            Instant::now() < deadline,
            "workflow timed out: {}",
            controller.notice
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn play_and_song_selection_recover_without_an_interruption_end_and_report_real_decode_errors() {
    use crate::{audio::PlaybackStatus, platform::ios::Event};
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        Default::default(),
    )))
    .unwrap();
    let ui = MobileWindow::new().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::new(directory.path().into()).unwrap();
    let audio = storage.music_dir().join("Interrupted.wav");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quiet.wav"),
        &audio,
    )
    .unwrap();
    let mut controller = Controller::new(storage).unwrap();
    pump_until(&mut controller, &ui, |c, _| c.tracks.len() == 1);
    // Removing the scanned fixture makes decoding fail before any real device
    // opens, and lets this verify Error vs an indefinitely deferred Loading.
    std::fs::remove_file(audio).unwrap();
    controller.playback.select(0, vec![0]);
    controller
        .playback
        .progress(controller.playback.session, PlaybackStatus::Playing, 23.5);
    controller.native_event(&ui, Event::InterruptionBegan { epoch: 1 });
    assert!(controller.playback.interrupted());
    assert!(!controller.playback.should_play());
    controller.command(&ui, "toggle"); // The visible Play button is a Play request.
    assert!(!controller.playback.interrupted());
    assert!(controller.playback.wants_playing);
    assert_eq!(controller.playback.position, 23.5);
    assert!(controller.deferred_load.is_none());
    pump_until(&mut controller, &ui, |c, _| {
        c.playback.status == PlaybackStatus::Error
    });
    assert!(!controller.notice.is_empty());

    controller.native_event(&ui, Event::InterruptionBegan { epoch: 2 });
    controller.select(&ui, 0, vec![0], false);
    assert!(!controller.playback.interrupted());
    assert!(controller.deferred_load.is_none());
    pump_until(&mut controller, &ui, |c, _| {
        c.playback.status == PlaybackStatus::Error
    });
    assert!(!controller.playback.wants_playing);
}

#[test]
fn playlist_edit_lyric_save_rating_and_delete_survive_a_real_controller_restart() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        Default::default(),
    )))
    .unwrap();
    let ui = MobileWindow::new().unwrap();
    connect_translations(&ui);
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::new(directory.path().into()).unwrap();
    for name in ["First.wav", "Second.wav"] {
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quiet.wav"),
            storage.music_dir().join(name),
        )
        .unwrap();
    }
    let mut controller = Controller::new(storage.clone()).unwrap();
    controller.sync_preferences(&ui);
    pump_until(&mut controller, &ui, |controller, _| {
        controller.tracks.len() == 2 && controller.ratings_ready
    });
    let first = controller.tracks[0].path.clone();
    let second = controller.tracks[1].path.clone();
    controller.command(&ui, "new-playlist");
    assert!(ui.get_playlist_editor_open());
    ui.set_playlist_name("Dinner mix".into());
    controller.playlist_command(&ui, "toggle", first.to_str().unwrap());
    controller.playlist_command(&ui, "toggle", second.to_str().unwrap());
    controller.playlist_command(&ui, "move:0", second.to_str().unwrap());
    controller.playlist_command(&ui, "create", "");
    pump_until(&mut controller, &ui, |controller, ui| {
        !ui.get_playlist_editor_open() && controller.saved_playlists.len() == 1
    });
    let playlist = controller.saved_playlists[0].clone();
    assert_eq!(playlist.tracks, [second.clone(), first.clone()]);
    controller.open_playlist_editor(&ui, Some(playlist.id));
    ui.set_playlist_name("Renamed mix".into());
    controller.playlist_command(&ui, "create", "");
    pump_until(&mut controller, &ui, |_, ui| !ui.get_playlist_editor_open());
    assert_eq!(controller.saved_playlists[0].id, playlist.id);
    assert_eq!(controller.saved_playlists[0].name, "Renamed mix");

    controller.navigate(&ui, Page::Lyrics(0));
    controller.annotation_command(&ui, "lyric-edit");
    assert!(ui.get_lyric_editor_open());
    let source = "[00:01.250]First line\n[00:02.500]中文 日本語";
    ui.set_lyric_editor_text(source.into());
    controller.lyric_editor_command(&ui, "save");
    pump_until(&mut controller, &ui, |_, ui| !ui.get_lyric_editor_open());
    assert_eq!(controller.tracks[0].lyrics, Lyrics::parse(source));
    controller.rate_track(&ui, 5);
    drop(controller);

    let restored = storage.refresh().unwrap();
    assert_eq!(
        restored
            .tracks
            .iter()
            .find(|track| track.path == first)
            .unwrap()
            .lyrics,
        Lyrics::parse(source)
    );
    assert_eq!(
        StatisticsStore::open(storage.database_path())
            .unwrap()
            .ratings()
            .unwrap()[&first],
        5
    );
    assert_eq!(
        playlists::load(storage.database_path()).unwrap()[0].tracks,
        [second.clone(), first.clone()]
    );
    let mut controller = Controller::new(storage.clone()).unwrap();
    pump_until(&mut controller, &ui, |controller, _| {
        controller.saved_playlists.len() == 1
    });
    controller.playlist_command(&ui, "delete-request", &playlist.id.to_string());
    assert!(ui.get_playlist_delete_open());
    assert_eq!(playlists::load(storage.database_path()).unwrap().len(), 1);
    controller.playlist_command(&ui, "delete-cancel", "");
    assert_eq!(playlists::load(storage.database_path()).unwrap().len(), 1);
    controller.playlist_command(&ui, "delete-request", &playlist.id.to_string());
    controller.playlist_command(&ui, "delete-confirm", &playlist.id.to_string());
    pump_until(&mut controller, &ui, |controller, _| {
        controller.saved_playlists.is_empty()
    });
    assert!(playlists::load(storage.database_path()).unwrap().is_empty());
    assert!(first.is_file() && second.is_file());
}

#[test]
fn api_editor_keeps_a_saved_key_when_only_the_model_changes() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        Default::default(),
    )))
    .unwrap();
    let ui = MobileWindow::new().unwrap();
    connect_translations(&ui);
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::new(directory.path().into()).unwrap();
    let mut controller = Controller::new(storage.clone()).unwrap();
    controller.open_api_settings(&ui);
    crate::platform::ios::copy_text(ui.window(), "  sk-offline-fixture\n").unwrap();
    assert!(
        ui.get_api_key().is_empty(),
        "opening settings must not read the clipboard"
    );
    controller.api_command(&ui, "paste-api-key");
    assert_eq!(ui.get_api_key(), "sk-offline-fixture");
    assert!(
        !storage.support_dir().join("deepseek.json").exists(),
        "Paste fills the editor; only Save persists credentials"
    );
    ui.set_api_model("deepseek-chat".into());
    controller.api_command(&ui, "save");
    assert!(!ui.get_api_settings_open());
    assert!(ui.get_api_key().is_empty());
    controller.open_api_settings(&ui);
    assert!(ui.get_api_key_configured());
    assert!(ui.get_api_key().is_empty());
    ui.set_api_model("deepseek-reasoner".into());
    controller.api_command(&ui, "save");
    let saved =
        crate::ai::DeepSeekSettings::load(&storage.support_dir().join("deepseek.json")).unwrap();
    assert_eq!(saved.api_key, "sk-offline-fixture");
    assert_eq!(saved.model, "deepseek-reasoner");
}

#[test]
fn album_and_artist_thumbnails_decode_into_existing_rows_without_resetting_navigation() {
    use crate::mobile::navigation::Group;
    use slint::Model;
    use std::io::Cursor;
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = MobileWindow::new().unwrap();
    connect_translations(&ui);
    ui.show().unwrap();
    ui.window().set_size(slint::LogicalSize::new(390.0, 844.0));
    let directory = tempfile::tempdir().unwrap();
    let mut controller = Controller::new(Storage::new(directory.path().into()).unwrap()).unwrap();
    pump_until(&mut controller, &ui, |controller, _| {
        controller.job.is_none() && controller.ratings_ready
    });
    let mut encoded = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(32, 32, image::Rgba([220, 40, 10, 255]))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    controller.tracks.push(crate::library::Track {
        path: directory.path().join("music.wav"),
        title: "Song".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        genre: String::new(),
        number: 1,
        duration: 30.0,
        artwork: encoded.into_inner(),
        lyrics: Default::default(),
    });
    for group in [Group::Albums, Group::Artists] {
        controller.navigate(&ui, Page::Groups(group));
        assert_eq!(
            controller.entries[0].track, None,
            "menu cover must not play its representative song"
        );
        let rows = controller.row_model.clone();
        let deadline = Instant::now() + Duration::from_secs(8);
        while ui
            .get_rows()
            .row_data(0)
            .is_none_or(|row| row.artwork.size().width == 0)
        {
            controller.poll_thumbnails(&ui);
            assert!(Instant::now() < deadline, "thumbnail worker timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            std::rc::Rc::ptr_eq(&rows, &controller.row_model),
            "cover completion updates a row rather than replacing the whole list"
        );
        assert_eq!(ui.get_screen(), 8);
        assert_eq!(ui.get_rows().row_data(0).unwrap().artwork.size().width, 192);
    }
}
