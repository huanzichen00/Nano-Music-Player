use super::{connect_translations, navigation};
use crate::ui::{LibraryRow, MobileWindow};
use slint::{
    ComponentHandle, ModelRc, VecModel,
    platform::{PointerEventButton, WindowEvent},
};
use std::{cell::RefCell, rc::Rc};

fn click(ui: &MobileWindow, x: f32, y: f32) {
    let position = slint::LogicalPosition::new(x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn lcd_click(ui: &MobileWindow, x: f32, y: f32) {
    click(
        ui,
        ui.get_display_left() + x * ui.get_scale(),
        ui.get_display_top() + y * ui.get_scale(),
    );
}

fn snapshot(ui: &MobileWindow, name: &str) {
    let pixels = ui.window().take_snapshot().unwrap();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ios-preview");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(name),
        pixels.as_bytes(),
        pixels.width(),
        pixels.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

#[test]
fn shared_nano_views_preserve_device_geometry_and_touch_controls() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = MobileWindow::new().unwrap();
    connect_translations(&ui);
    ui.global::<crate::ui::I18n>().set_language(1);
    ui.set_clock_text("12:59 PM".into());
    ui.set_selected(0);
    ui.set_safe_top(44.0);
    ui.set_safe_bottom(34.0);
    ui.show().unwrap();
    ui.window().set_size(slint::LogicalSize::new(390.0, 844.0));

    let commands = Rc::new(RefCell::new(Vec::new()));
    let received = commands.clone();
    ui.on_command(move |command| received.borrow_mut().push(command.to_string()));
    let homes = Rc::new(RefCell::new(Vec::new()));
    let received = homes.clone();
    ui.on_home_activated(move |page, index| received.borrow_mut().push((page, index)));
    let selected = Rc::new(RefCell::new(Vec::new()));
    let received = selected.clone();
    ui.on_item_clicked(move |index| received.borrow_mut().push(index));
    let seeks = Rc::new(RefCell::new(Vec::new()));
    let received = seeks.clone();
    ui.on_seek_changed(move |position| received.borrow_mut().push(position));
    let volumes = Rc::new(RefCell::new(Vec::new()));
    let received = volumes.clone();
    ui.on_volume_changed(move |volume| received.borrow_mut().push(volume));
    let copied = Rc::new(RefCell::new(Vec::new()));
    let received = copied.clone();
    let copy_ui = ui.as_weak();
    ui.on_copy_text(move |text| {
        crate::platform::ios::copy_text(copy_ui.upgrade().unwrap().window(), &text).unwrap();
        received.borrow_mut().push(text.to_string());
    });

    snapshot(&ui, "home.png");
    lcd_click(&ui, 64.0, 102.0);
    assert_eq!(homes.borrow().last(), Some(&(0, 0)));
    ui.set_home_page(1);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::ZERO);
    ui.window().take_snapshot().unwrap();
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(220));
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(0.0, 0.0),
    });
    snapshot(&ui, "home-settings.png");
    lcd_click(&ui, 176.0, 102.0);
    assert_eq!(homes.borrow().last(), Some(&(1, 1)));

    let mut navigation = navigation::Navigation::default();
    navigation.open(navigation::Page::Music);
    ui.set_screen(7);
    ui.set_screen_title(navigation.title().into());
    ui.set_rows(ModelRc::new(VecModel::from(
        navigation
            .rows(&[], "", &[])
            .into_iter()
            .map(|row| LibraryRow {
                title: row.title.into(),
                subtitle: row.subtitle.into(),
                icon: row.icon,
                chevron: row.chevron,
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));
    snapshot(&ui, "music.png");
    lcd_click(&ui, 120.0, 64.0 + 4.5 * 48.0);
    assert_eq!(selected.borrow().last(), Some(&4));

    ui.set_screen(1);
    ui.set_screen_title("歌曲".into());
    ui.set_rows(ModelRc::new(VecModel::from(vec![
        LibraryRow {
            title: "随机播放".into(),
            ..Default::default()
        },
        LibraryRow {
            title: "晴天".into(),
            subtitle: "周杰伦".into(),
            icon: 8,
            ..Default::default()
        },
        LibraryRow {
            title: "One Summer's Day".into(),
            subtitle: "久石譲".into(),
            ..Default::default()
        },
        LibraryRow {
            title: "Dreams".into(),
            subtitle: "Fleetwood Mac".into(),
            ..Default::default()
        },
    ])));
    ui.set_selected(1);
    snapshot(&ui, "library.png");
    lcd_click(&ui, 120.0, 136.0);
    assert_eq!(selected.borrow().last(), Some(&1));

    ui.set_screen(3);
    ui.set_screen_title("搜索".into());
    let queries = Rc::new(RefCell::new(Vec::new()));
    let received = queries.clone();
    ui.on_query_changed(move |query| received.borrow_mut().push(query.to_string()));
    ui.window().take_snapshot().unwrap();
    lcd_click(&ui, 90.0, 85.0);
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: "夏".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: "夏".into() });
    assert_eq!(queries.borrow().last().map(String::as_str), Some("夏"));
    snapshot(&ui, "search.png");

    ui.set_screen(2);
    ui.set_has_track(true);
    ui.set_playing(true);
    ui.set_track_title("晴天".into());
    ui.set_track_artist("周杰伦".into());
    ui.set_track_album("叶惠美".into());
    ui.set_track_count("1 / 3".into());
    ui.set_elapsed("1:24".into());
    ui.set_duration("4:29".into());
    ui.set_progress(0.31);
    ui.set_volume(0.6);
    ui.set_lyrics_visible(false);
    ui.set_overlay_visible(true);
    snapshot(&ui, "player.png");
    lcd_click(&ui, 120.0, 120.0);
    assert!(!seeks.borrow().is_empty());
    assert!((seeks.borrow().last().unwrap() - 0.5).abs() < 0.01);
    lcd_click(&ui, 120.0, 361.0);
    assert_eq!(commands.borrow().last().map(String::as_str), Some("toggle"));
    lcd_click(&ui, 106.0, 413.0);
    assert!(!volumes.borrow().is_empty());
    assert!((volumes.borrow().last().unwrap() - 0.5).abs() < 0.01);
    lcd_click(&ui, 22.0, 13.0);
    assert_eq!(commands.borrow().last().map(String::as_str), Some("back"));
    ui.set_lyric_lines(ModelRc::new(VecModel::from(vec![
        "故事的小黄花".into(),
        "从出生那年就飘着".into(),
        "童年的荡秋千".into(),
    ])));
    ui.set_active_lyric(1);
    ui.set_lyrics_visible(true);
    snapshot(&ui, "lyrics.png");
    lcd_click(&ui, 120.0, 270.0);
    assert!(!ui.get_overlay_visible());
    ui.set_overlay_visible(true);

    ui.set_screen(6);
    ui.set_screen_title("电脑传歌".into());
    ui.set_notice("".into());
    ui.window().take_snapshot().unwrap();
    click(
        &ui,
        ui.get_display_left() + 120.0 * ui.get_scale(),
        ui.get_transfer_start_y(),
    );
    assert_eq!(
        commands.borrow().last().map(String::as_str),
        Some("transfer-start")
    );
    ui.set_transfer_running(true);
    ui.set_transfer_urls(ModelRc::new(VecModel::from(vec![
        "http://192.168.1.8:49152/#0123456789abcdef0123456789abcdef".into(),
    ])));
    snapshot(&ui, "transfer.png");
    click(
        &ui,
        ui.get_display_left() + 120.0 * ui.get_scale(),
        ui.get_transfer_copy_y(),
    );
    let expected = "http://192.168.1.8:49152/#0123456789abcdef0123456789abcdef";
    assert_eq!(copied.borrow().last().map(String::as_str), Some(expected));
    let context = slint::private_unstable_api::re_exports::WindowInner::from_pub(ui.window())
        .try_context()
        .unwrap();
    assert_eq!(
        context
            .platform()
            .clipboard_text(slint::platform::Clipboard::DefaultClipboard)
            .as_deref(),
        Some(expected)
    );
    click(
        &ui,
        ui.get_display_left() + 120.0 * ui.get_scale(),
        ui.get_transfer_start_y(),
    );
    assert_eq!(
        commands.borrow().last().map(String::as_str),
        Some("transfer-stop")
    );
    click(&ui, ui.get_home_button_x(), ui.get_home_button_y());
    assert_eq!(commands.borrow().last().map(String::as_str), Some("home"));

    ui.set_screen(6);
    ui.global::<crate::ui::I18n>().set_language(0);
    ui.set_screen_title("Computer Transfer".into());
    snapshot(&ui, "transfer-en.png");
    click(
        &ui,
        ui.get_display_left() + 120.0 * ui.get_scale(),
        ui.get_transfer_copy_y(),
    );
    assert_eq!(copied.borrow().len(), 2);

    for (width, height, top, right, bottom, left) in [
        (320.0, 568.0, 20.0, 0.0, 0.0, 0.0),
        (390.0, 844.0, 44.0, 0.0, 34.0, 0.0),
        (844.0, 390.0, 0.0, 44.0, 21.0, 44.0),
    ] {
        ui.window().set_size(slint::LogicalSize::new(width, height));
        ui.set_safe_top(top);
        ui.set_safe_right(right);
        ui.set_safe_bottom(bottom);
        ui.set_safe_left(left);
        for device in [true, false] {
            ui.set_device_mode(device);
            for screen in [0, 1, 2, 6] {
                ui.set_screen(screen);
                ui.window().take_snapshot().unwrap();
                assert!(
                    (ui.get_content_left() + ui.get_content_width() + right - width).abs() < 0.1
                );
                assert!(
                    (ui.get_content_top() + ui.get_content_height() + bottom - height).abs() < 0.1
                );
                assert!(ui.get_device_left() >= left && ui.get_device_top() >= top);
                assert!(ui.get_device_left() + ui.get_device_width() <= width - right + 0.1);
                assert!(ui.get_device_top() + ui.get_device_height() <= height - bottom + 0.1);
                assert!(
                    (ui.get_display_width() / ui.get_display_height() - 240.0 / 432.0).abs()
                        < 0.001
                );
                if width == 320.0 && device && screen == 2 {
                    snapshot(&ui, "small-player.png");
                }
            }
        }
    }
    ui.set_device_mode(true);
    ui.set_screen(2);
    snapshot(&ui, "landscape.png");
}

#[test]
fn shared_settings_switch_language_and_restore_mobile_preferences_after_restart() {
    use super::Controller;
    use crate::{
        shared::{action::Action, config::Settings, i18n::Language, settings::Target},
        ui::I18n,
    };
    use slint::Model;

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
    let storage = super::Storage::new(directory.path().into()).unwrap();
    let mut controller = Controller::new(storage.clone()).unwrap();
    controller.sync_preferences(&ui);
    assert_eq!(
        ui.global::<I18n>().get_language(),
        Language::English.index() as i32
    );
    controller.navigate(&ui, navigation::Page::Settings);
    let language_row = controller
        .entries
        .iter()
        .position(|entry| {
            entry.action
                == navigation::Selection::Setting(Target::Screen(navigation::Page::Languages))
        })
        .unwrap();
    assert_eq!(controller.entries[language_row].title, "Language");
    assert_eq!(
        ui.get_rows().row_data(language_row).unwrap().value.as_str(),
        "English"
    );
    snapshot(&ui, "settings-en.png");
    controller.activate(&ui, language_row);
    controller.activate(&ui, Language::Japanese.index());
    assert_eq!(ui.global::<I18n>().get_language(), 2);
    assert_eq!(ui.get_screen_title().as_str(), "言語");
    assert!(ui.get_rows().row_data(2).unwrap().checked);
    assert_eq!(
        Settings::load(&storage.settings_path()).unwrap().language,
        Language::Japanese
    );
    controller.command(&ui, "back");
    assert_eq!(ui.get_screen_title().as_str(), "設定");
    assert_eq!(controller.navigation.selected(), language_row);
    controller.apply_setting(&ui, Target::Language(Language::SimplifiedChinese));
    controller.apply_setting(&ui, Target::DisplayMode(false));
    controller.apply_setting(&ui, Target::Action(Action::ToggleSmoothSlides));
    controller.apply_setting(&ui, Target::Action(Action::ToggleLyrics));
    assert!(!ui.get_device_mode());
    assert!(!ui.get_smooth_slides());
    assert!(!ui.get_lyrics_visible());
    drop(controller);

    let mut controller = Controller::new(storage).unwrap();
    controller.sync_preferences(&ui);
    controller.apply_setting(&ui, Target::Screen(navigation::Page::Languages));
    assert_eq!(ui.global::<I18n>().get_language(), 1);
    assert!(ui.get_rows().row_data(1).unwrap().checked);
    assert!(!ui.get_device_mode());
    assert!(!ui.get_smooth_slides());
    assert!(!ui.get_lyrics_visible());
    ui.window().set_size(slint::LogicalSize::new(390.0, 844.0));
    snapshot(&ui, "settings-language.png");
}
