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
use slint::Model;
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
}
