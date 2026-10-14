use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        navigation::{Screen, SettingsSection},
    },
    translation::TargetLanguage,
    ui::AppWindow,
};
use slint::{ComponentHandle, Model, platform::WindowEvent};
use std::{cell::RefCell, rc::Rc};

#[test]
fn ipod_translation_settings_share_menu_commands_and_persist_language() {
    i_slint_backend_testing::init_no_event_loop();
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
    controller.borrow_mut().execute(&ui, Action::Settings);
    ui.invoke_item_clicked(2);
    assert_eq!(ui.get_screen_title(), "Lyrics");
    ui.invoke_item_clicked(1);
    assert_eq!(ui.get_screen_title(), "Translation");
    assert_eq!(ui.get_rows().row_data(0).unwrap().subtitle, "English");
    assert_eq!(ui.get_rows().row_data(3).unwrap().subtitle, "Off");
    ui.invoke_item_clicked(0);
    assert_eq!(ui.get_screen_title(), "Translation Language");
    ui.invoke_item_clicked(1);
    assert_eq!(
        Settings::load(&directory.path().join("settings.json"))
            .unwrap()
            .translation_target,
        TargetLanguage::SimplifiedChinese
    );
    assert_eq!(ui.get_rows().row_data(1).unwrap().subtitle, "✓");
    assert_eq!(
        controller.borrow().state.language,
        nano_music_player::app::i18n::Language::English
    );
    controller.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(ui.get_rows().row_data(0).unwrap().subtitle, "简体中文");
    ui.invoke_item_clicked(3);
    assert_eq!(ui.get_rows().row_data(3).unwrap().subtitle, "On");
    ui.invoke_command("toggle_translation".into());
    assert_eq!(ui.get_rows().row_data(3).unwrap().subtitle, "Off");
    ui.invoke_item_clicked(1);
    assert!(ui.get_notice().contains("Choose a song first"));
    ui.invoke_item_clicked(6);
    assert!(ui.get_deepseek_settings_open());
    assert_eq!(ui.get_deepseek_api_key(), "");
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::SettingsSection(SettingsSection::Translation)
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
        "Search with DeepSeek"
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
    ui.invoke_focus_screen();
    for _ in 0..2 {
        let previous = ui.get_translation_visible();
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: "t".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: "t".into() });
        assert_eq!(ui.get_translation_visible(), !previous);
        assert_eq!(
            Settings::load(&directory.path().join("settings.json"))
                .unwrap()
                .translation_visible,
            !previous
        );
        assert_eq!(ui.get_romaji_displayed(), previous);
    }
}
