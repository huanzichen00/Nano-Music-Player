use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        navigation::Screen,
    },
    translation::TargetLanguage,
    ui::AppWindow,
};
use slint::{ComponentHandle, Model, platform::WindowEvent};
use std::{cell::RefCell, rc::Rc};

fn snapshot(ui: &AppWindow, name: &str) {
    slint::platform::update_timers_and_animations();
    let image = ui.window().take_snapshot().unwrap();
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa/desktop-lyrics");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        image.as_bytes(),
        image.width(),
        image.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

#[test]
fn ipod_translation_settings_share_menu_commands_and_persist_language() {
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
    controller.borrow_mut().execute(&ui, Action::Settings);
    assert_eq!(
        ui.get_rows().row_data(6).unwrap().title,
        "Translate Current Song"
    );
    ui.invoke_item_clicked(6);
    assert_eq!(ui.get_screen_title(), "Song Translation");
    controller.borrow_mut().execute(&ui, Action::Back);
    ui.invoke_item_clicked(2);
    assert_eq!(ui.get_screen_title(), "Lyrics");
    assert_eq!(ui.get_rows().row_data(1).unwrap().value, "English");
    assert!(ui.get_rows().row_data(0).unwrap().toggle);
    assert!(!ui.get_rows().row_data(0).unwrap().checked);
    assert!(!ui.get_rows().row_data(0).unwrap().chevron);
    ui.invoke_item_clicked(1);
    assert_eq!(ui.get_screen_title(), "Translate To");
    ui.invoke_item_clicked(1);
    assert_eq!(
        Settings::load(&directory.path().join("settings.json"))
            .unwrap()
            .translation_target,
        TargetLanguage::SimplifiedChinese
    );
    assert!(ui.get_rows().row_data(1).unwrap().checked);
    assert_eq!(
        controller.borrow().state.language,
        nano_music_player::app::i18n::Language::English
    );
    controller.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(ui.get_rows().row_data(1).unwrap().value, "简体中文");
    ui.invoke_item_clicked(0);
    assert!(ui.get_rows().row_data(0).unwrap().checked);
    assert_eq!(
        ui.get_notice(),
        "Import a music folder to begin",
        "the Settings switch only changes display, without requesting translation"
    );
    ui.invoke_command("toggle_translation".into());
    assert!(!ui.get_rows().row_data(0).unwrap().checked);
    ui.invoke_item_clicked(3);
    assert_eq!(ui.get_screen_title(), "Song Translation");
    ui.invoke_item_clicked(0);
    assert!(ui.get_notice().contains("Choose a song first"));
    controller.borrow_mut().execute(&ui, Action::Settings);
    ui.invoke_item_clicked(5);
    assert!(ui.get_deepseek_settings_open());
    assert_eq!(ui.get_deepseek_api_key(), "");
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::Settings
    );
    ui.set_deepseek_settings_open(false);
    {
        use nano_music_player::library::{Track, lyrics::Lyrics};
        let mut c = controller.borrow_mut();
        c.state.tracks = ["Chosen Song", "Playing Song"]
            .into_iter()
            .enumerate()
            .map(|(id, title)| {
                let path = directory.path().join(format!("{id}.mp3"));
                std::fs::copy(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quiet.mp3"),
                    &path,
                )
                .unwrap();
                Track {
                    path,
                    title: title.into(),
                    artist: "Artist".into(),
                    album: "Album".into(),
                    genre: "Rock".into(),
                    number: 1,
                    duration: 60.0,
                    artwork: vec![],
                    lyrics: Lyrics::default(),
                }
            })
            .collect();
        c.state.current = Some(1);
        c.state.queue.set(vec![1], 1, false);
        c.execute(&ui, Action::Settings);
    }
    let queue = controller.borrow().state.queue.order.clone();
    ui.invoke_item_clicked(2);
    ui.invoke_item_clicked(3);
    assert_eq!(ui.get_rows().row_data(0).unwrap().subtitle, "Playing Song");
    {
        let mut c = controller.borrow_mut();
        c.state.current = Some(0);
        c.execute(&ui, Action::FinishSearchEditing);
    }
    assert_eq!(
        ui.get_rows().row_data(0).unwrap().subtitle,
        "Chosen Song",
        "the translation action always names the song it will affect"
    );
    {
        let mut c = controller.borrow_mut();
        c.state.current = Some(1);
        c.execute(&ui, Action::Settings);
    }
    ui.invoke_item_clicked(2);
    ui.invoke_item_clicked(4);
    assert_eq!(ui.get_screen_title(), "Choose Song");
    assert_eq!(ui.get_rows().row_count(), 2);
    ui.invoke_item_clicked(0);
    assert_eq!(ui.get_screen_title(), "Find Lyrics");
    assert_eq!(ui.get_rows().row_data(0).unwrap().title, "Chosen Song");
    assert_eq!(
        ui.get_rows().row_data(1).unwrap().title,
        "Find Lyrics Online"
    );
    assert_eq!(
        ui.get_rows().row_data(1).unwrap().subtitle,
        "Search online sources"
    );
    assert_eq!(controller.borrow().state.current, Some(1));
    assert_eq!(controller.borrow().state.queue.order, queue);
    // Reject invalid test credentials before any socket is opened, even if the
    // developer has an environment credential. Selecting/searching never plays.
    std::fs::write(
        directory.path().join("deepseek.json"),
        r#"{"api_key":"invalid\n","model":"deepseek-flash"}"#,
    )
    .unwrap();
    ui.invoke_item_clicked(1);
    assert!(ui.get_notice().contains("Invalid DeepSeek API key"));
    assert_eq!(controller.borrow().state.current, Some(1));
    ui.invoke_item_clicked(3);
    assert_eq!(ui.get_screen_title(), "Lyrics for Track");
    assert_eq!(
        ui.get_rows().row_data(2).unwrap().subtitle,
        "From audio tags and nearby files"
    );
    controller.borrow_mut().execute(&ui, Action::NowPlaying);
    controller.borrow_mut().execute(&ui, Action::ToggleLyrics);
    ui.invoke_focus_screen();
    let playing_path = controller.borrow().state.tracks[1].path.clone();
    let translation_visible = ui.get_translation_visible();
    assert!(
        ui.get_empty_lyrics_message()
            .contains("Press F to find lyrics")
    );
    snapshot(&ui, "missing-lyrics");
    controller.borrow_mut().execute(&ui, Action::ToggleOverlay);
    assert!(!ui.get_overlay_visible());
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: "f".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: "f".into() });
    assert!(ui.get_notice().contains("Invalid DeepSeek API key"));
    assert!(ui.get_overlay_visible());
    assert!(ui.get_lyrics_visible());
    assert!(
        ui.get_empty_lyrics_message()
            .contains("Invalid DeepSeek API key")
    );
    snapshot(&ui, "lookup-error");
    assert!(
        controller
            .borrow()
            .state
            .lyrics_lookup_status
            .contains_key(&playing_path)
    );
    assert_eq!(
        ui.get_translation_visible(),
        translation_visible,
        "F only finds source lyrics"
    );
    assert_eq!(controller.borrow().state.current, Some(1));
    assert_eq!(controller.borrow().state.queue.order, queue);
    {
        use nano_music_player::library::lyrics::Lyrics;
        let mut c = controller.borrow_mut();
        c.state.tracks[1].lyrics = Lyrics::parse("[00:01]Keep these lyrics");
    }
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: "f".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: "f".into() });
    assert_eq!(ui.get_notice(), "Lyrics are already available");
    assert_eq!(
        controller.borrow().state.tracks[1].lyrics.lines()[0],
        "Keep these lyrics"
    );
    controller.borrow_mut().state.tracks[1].lyrics = Default::default();
    // Display is already On from the earlier Translate action, but this song
    // has no translation. Its first T requests generation; its second hides it.
    for shown in [true, false] {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: "t".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: "t".into() });
        assert_eq!(ui.get_translation_visible(), shown);
        assert_eq!(
            Settings::load(&directory.path().join("settings.json"))
                .unwrap()
                .translation_visible,
            shown
        );
        assert_eq!(ui.get_romaji_displayed(), !shown);
        if shown {
            assert!(ui.get_notice().contains("Invalid DeepSeek API key"));
            let c = controller.borrow();
            assert!(
                c.state
                    .lyrics_lookup_status
                    .contains_key(&c.state.tracks[1].path)
            );
        }
    }
}
