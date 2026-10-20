use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        i18n::Language,
        navigation::{Screen, SettingsSection},
        state::Target,
    },
    audio::gain::GainMode,
    ui::AppWindow,
};
use slint::{
    ComponentHandle, Model,
    platform::{PointerEventButton, WindowEvent},
};
use std::{cell::RefCell, rc::Rc};

fn snapshot(ui: &AppWindow, name: &str) {
    slint::platform::update_timers_and_animations();
    let image = ui.window().take_snapshot().unwrap();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa/settings");
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

fn click(ui: &AppWindow, x: f32, y: f32) {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let content_height = ui.get_content_height();
    let body_height = if ui.get_device_mode() {
        296.0 * 76.5 / 39.6
    } else {
        432.0
    };
    let top = size.height - content_height + (content_height - body_height * scale) / 2.0;
    let position = slint::LogicalPosition::new(
        (size.width - 240.0 * scale) / 2.0 + x * scale,
        top + (y + if ui.get_device_mode() { 36.0 } else { 0.0 }) * scale,
    );
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

#[test]
fn settings_controls_are_explicit_persist_and_keep_the_playback_context() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    Settings {
        smooth_slides: false,
        device_mode: false,
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.path().to_owned())));
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    controller.borrow_mut().state.notice.clear();
    controller.borrow_mut().execute(&ui, Action::Settings);
    assert_eq!(ui.get_rows().row_count(), 8);
    snapshot(&ui, "settings-en");
    assert!(ui.get_list_row_height() * 7.0 <= ui.get_list_viewport_height() + 0.01);
    // Real pointer events use the same row geometry as keyboard navigation.
    click(&ui, 90.0, 64.0 + 14.0);
    assert_eq!(ui.get_screen_title(), "Appearance");
    assert_eq!(ui.get_rows().row_data(0).unwrap().value, "Screen Only");
    assert!(ui.get_rows().row_data(4).unwrap().toggle);
    assert!(!ui.get_rows().row_data(4).unwrap().chevron);
    snapshot(&ui, "appearance-en");
    click(&ui, 210.0, 64.0 + 4.0 * 56.0 + 18.0);
    assert!(!ui.get_rows().row_data(4).unwrap().checked);
    assert!(!Settings::load(&path).unwrap().always_on_top);
    ui.invoke_item_clicked(0);
    assert_eq!(ui.get_selected(), 1, "open with the saved choice selected");
    ui.invoke_item_clicked(0);
    assert!(Settings::load(&path).unwrap().device_mode);
    controller.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(ui.get_selected(), 0);
    assert_eq!(ui.get_rows().row_data(0).unwrap().value, "iPod Body");
    ui.invoke_item_clicked(1);
    ui.invoke_item_clicked(3);
    assert_eq!(Settings::load(&path).unwrap().zoom, 1.5);
    controller.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(ui.get_rows().row_data(1).unwrap().value, "150%");
    controller.borrow_mut().execute(&ui, Action::Settings);
    ui.invoke_item_clicked(1);
    assert_eq!(
        ui.get_rows().row_count(),
        4,
        "all playback preferences share one page"
    );
    snapshot(&ui, "playback-en");
    ui.invoke_item_clicked(1);
    assert_eq!(ui.get_rows().row_count(), 3);
    ui.invoke_item_clicked(2);
    assert_eq!(
        Settings::load(&path).unwrap().replay_gain_mode,
        GainMode::Album
    );
    assert!(ui.get_rows().row_data(2).unwrap().checked);
    controller.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(ui.get_rows().row_data(1).unwrap().value, "By Album");
    ui.invoke_item_clicked(2);
    ui.invoke_item_clicked(3);
    assert_eq!(Settings::load(&path).unwrap().replay_gain_preamp_db, -3.0);
    controller.borrow_mut().execute(&ui, Action::Back);
    assert_eq!(ui.get_rows().row_data(2).unwrap().value, "-3 dB");
    for language in [
        Language::SimplifiedChinese,
        Language::Japanese,
        Language::English,
    ] {
        {
            let mut c = controller.borrow_mut();
            c.state.navigation.open(Screen::Languages);
            c.state.navigation.select(language.index());
            c.execute(&ui, Action::Activate);
            c.execute(&ui, Action::Settings);
        }
        snapshot(&ui, &format!("settings-{}", language.code()));
        ui.invoke_item_clicked(6);
        assert_eq!(
            controller.borrow().state.navigation.current().screen,
            Screen::SettingsSection(SettingsSection::Translation)
        );
        snapshot(&ui, &format!("song-translation-{}", language.code()));
        controller.borrow_mut().execute(&ui, Action::Back);
        for (index, name) in [(0, "appearance"), (1, "playback"), (2, "lyrics")] {
            ui.invoke_item_clicked(index);
            snapshot(&ui, &format!("{name}-{}", language.code()));
            controller.borrow_mut().execute(&ui, Action::Back);
        }
    }
    // Preferences never create a playing song or populate the queue.
    let c = controller.borrow();
    assert_eq!(c.state.current, None);
    assert!(c.state.queue.order.is_empty());
    assert!(
        c.state
            .rows()
            .iter()
            .any(|row| matches!(row.target, Target::Action(Action::ConfigureDeepSeek)))
    );
    assert_eq!(c.state.navigation.current().screen, Screen::Settings);
    drop(c);
    ui.hide().unwrap();
}

#[test]
fn settings_copy_has_translations() {
    use nano_music_player::app::{i18n::has_translation, state::State};
    let mut state = State::default();
    for section in [
        SettingsSection::Display,
        SettingsSection::DisplayMode,
        SettingsSection::WindowSize,
        SettingsSection::Playback,
        SettingsSection::ReplayGain,
        SettingsSection::ReplayGainPreamp,
        SettingsSection::Lyrics,
        SettingsSection::Translation,
        SettingsSection::TranslationLanguage,
    ] {
        state.navigation.open(Screen::SettingsSection(section));
        assert!(has_translation(state.navigation.current().screen.title()));
        for row in state.rows() {
            if !row.subtitle.is_empty() {
                assert!(has_translation(&row.subtitle), "{}", row.subtitle);
            }
            if !matches!(
                row.target,
                Target::Zoom(_)
                    | Target::ReplayGainPreamp(_)
                    | Target::Translation(nano_music_player::translation::Command::SetTarget(_))
            ) {
                assert!(has_translation(&row.title), "{}", row.title);
            }
        }
    }
}
