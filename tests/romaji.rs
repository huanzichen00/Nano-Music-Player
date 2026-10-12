use nano_music_player::{
    ai::DeepSeekSettings,
    app::{
        action::Action,
        config::Settings,
        controller::Controller,
        navigation::{Filter, Screen},
        state::{Effect, State},
    },
    input::{Context, Dispatch, Dispatcher, Key, KeyPress, Keymap, Modifiers, Surface},
    library::{Track, database::Database, lyrics::Lyrics, romaji::Romaji},
    ui::AppWindow,
};
use slint::Model;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn track(path: PathBuf, title: &str) -> Track {
    Track {
        path,
        title: title.into(),
        artist: "Artist".into(),
        album: "Album".into(),
        genre: String::new(),
        number: 0,
        duration: 60.0,
        artwork: Vec::new(),
        lyrics: Lyrics::parse("[00:01]君の声\n[00:02]Hello\n[00:03]空へ"),
    }
}

#[test]
fn romaji_actions_target_the_selected_song_and_pin_the_lyrics_screen() {
    let mut state = State::default();
    state.merge_library(vec![track("a.mp3".into(), "A"), track("b.mp3".into(), "B")]);
    state.current = Some(0);
    state.navigation.open(Screen::Songs(Filter::All));
    state.navigation.select(1);
    assert!(matches!(
        state.apply(Action::GenerateRomaji).as_slice(),
        [Effect::GenerateRomaji { id: 1 }]
    ));
    state.apply(Action::ManageLyrics);
    state.current = Some(0);
    assert!(matches!(
        state.apply(Action::RemoveRomaji).as_slice(),
        [Effect::RemoveRomaji { id: 1 }]
    ));
    assert!(matches!(
        state.apply(Action::CancelRomaji).as_slice(),
        [Effect::CancelRomaji]
    ));
    assert!(matches!(
        state.apply(Action::ToggleRomaji).as_slice(),
        [Effect::RomajiVisibilityChanged]
    ));
    assert!(!state.romaji_visible);
}

#[test]
fn plain_r_toggles_romaji_only_on_the_song_interface_and_respects_guards_and_customization() {
    let mut dispatcher = Dispatcher::new(Keymap::default());
    let mut press = KeyPress {
        key: Key::Letter('r'),
        modifiers: Modifiers::default(),
        repeat: false,
    };
    let mut context = Context {
        surface: Surface::NowPlaying,
        text_input: false,
        reference_open: false,
        native_dialog: false,
    };
    for surface in [
        Surface::Home,
        Surface::Library,
        Surface::Placeholder,
        Surface::NowPlaying,
        Surface::NowPlayingVolume,
    ] {
        context.surface = surface;
        let action = if matches!(surface, Surface::NowPlaying | Surface::NowPlayingVolume) {
            Action::ToggleRomaji
        } else {
            Action::CycleRepeat
        };
        assert_eq!(
            dispatcher.dispatch(&press, context),
            Dispatch::Action(action)
        );
    }
    context.surface = Surface::NowPlaying;
    press.repeat = true;
    assert_eq!(dispatcher.dispatch(&press, context), Dispatch::Consumed);
    press.repeat = false;
    context.text_input = true;
    assert_eq!(dispatcher.dispatch(&press, context), Dispatch::Ignored);
    context.text_input = false;
    context.native_dialog = true;
    assert_eq!(dispatcher.dispatch(&press, context), Dispatch::Ignored);
    context.native_dialog = false;
    context.reference_open = true;
    assert_eq!(dispatcher.dispatch(&press, context), Dispatch::Consumed);
    context.reference_open = false;
    dispatcher
        .keymap
        .bindings
        .iter_mut()
        .find(|binding| binding.key == Key::Letter('r'))
        .unwrap()
        .action = Action::ToggleShuffle;
    assert_eq!(
        dispatcher.dispatch(&press, context),
        Dispatch::Action(Action::ToggleShuffle)
    );
}

#[test]
fn restart_loads_annotations_and_controls_preserve_credentials_and_original_lyrics() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("song.mp3");
    let original = track(path.clone(), "Japanese song");
    let romaji = Romaji::new(
        original.lyrics.clone(),
        vec!["kimi no koe".into(), "".into(), "sora e".into()],
        "test".into(),
    )
    .unwrap();
    let database_path = directory.path().join("library.sqlite3");
    {
        let database = Database::open(&database_path).unwrap();
        database.save(&original, 1).unwrap();
        database.save_romaji(&path, &romaji).unwrap();
    }
    let key_path = directory.path().join("deepseek.json");
    DeepSeekSettings {
        api_key: "fixture-secret".into(),
        ..Default::default()
    }
    .save(&key_path)
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let mut controller = Controller::new(directory.path().to_path_buf());
    controller.initialize(&ui);
    let deadline = Instant::now() + Duration::from_secs(5);
    while controller.state.tracks.is_empty() || controller.state.romaji.is_empty() {
        assert!(
            Instant::now() < deadline,
            "background library/cache load timed out"
        );
        controller.poll(&ui);
        std::thread::sleep(Duration::from_millis(5));
    }
    controller.state.current = Some(0);
    controller.poll(&ui);
    assert_eq!(ui.get_lyric_lines().row_data(0).unwrap(), "君の声");
    assert_eq!(ui.get_romaji_lines().row_count(), 3);
    assert_eq!(ui.get_romaji_lines().row_data(0).unwrap(), "kimi no koe");
    assert_eq!(ui.get_romaji_lines().row_data(1).unwrap(), "");
    assert!(ui.get_timed_lyrics());
    controller.execute(&ui, Action::ToggleRomaji);
    assert!(!ui.get_romaji_visible());
    assert!(
        !Settings::load(&directory.path().join("settings.json"))
            .unwrap()
            .romaji_visible
    );

    controller.execute(&ui, Action::ConfigureDeepSeek);
    assert!(ui.get_deepseek_settings_open());
    assert!(ui.get_deepseek_key_configured());
    assert!(ui.get_deepseek_api_key().is_empty());
    ui.set_deepseek_model("invalid/model".into());
    controller.deepseek_command(&ui, "save");
    assert!(ui.get_deepseek_settings_open());
    assert!(!ui.get_deepseek_message().is_empty());
    assert_eq!(
        DeepSeekSettings::load(&key_path).unwrap().api_key,
        "fixture-secret"
    );
    ui.set_deepseek_model("deepseek-flash".into());
    controller.deepseek_command(&ui, "save");
    assert_eq!(
        DeepSeekSettings::load(&key_path).unwrap().api_key,
        "fixture-secret"
    );
    assert!(!ui.get_deepseek_settings_open());
    controller.execute(&ui, Action::ConfigureDeepSeek);
    ui.set_deepseek_api_key("discard-this-key".into());
    controller.deepseek_command(&ui, "cancel");
    assert!(ui.get_deepseek_api_key().is_empty());
    assert_eq!(
        DeepSeekSettings::load(&key_path).unwrap().api_key,
        "fixture-secret"
    );

    controller.execute(&ui, Action::RemoveRomaji);
    assert_eq!(ui.get_romaji_lines().row_count(), 0);
    assert_eq!(controller.state.tracks[0].lyrics, original.lyrics);
    drop(controller); // Drains the queued SQLite deletion before another process opens the cache.
    assert!(
        Database::open(&database_path)
            .unwrap()
            .load_romaji()
            .unwrap()
            .is_empty()
    );
    assert!(
        !std::fs::read_to_string(directory.path().join("settings.json"))
            .unwrap()
            .contains("fixture-secret")
    );
    drop(ui);
}
