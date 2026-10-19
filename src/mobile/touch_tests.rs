//! Portable input/layout checks; native IME lifecycle is verified separately on iOS.
use super::connect_translations;
use crate::ui::{LibraryRow, MobileWindow, PlaylistCandidate};
use slint::{
    ComponentHandle, ModelRc, VecModel,
    platform::{Clipboard, Platform, PointerEventButton, WindowEvent},
};
use std::{cell::RefCell, rc::Rc, time::Duration};

fn lcd_event(ui: &MobileWindow, x: f32, y: f32, kind: &str) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    let position = slint::LogicalPosition::new(
        ui.get_display_left() + x * ui.get_scale(),
        ui.get_display_top() + y * ui.get_scale(),
    );
    ui.window().dispatch_event(match kind {
        "press" => WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        },
        "release" => WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        },
        _ => WindowEvent::PointerMoved { position },
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
}

#[test]
fn list_swipes_enter_the_touched_menu_return_once_and_do_not_play_or_steal_vertical_scroll() {
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
    ui.set_safe_top(44.0);
    ui.set_safe_bottom(34.0);
    ui.show().unwrap();
    ui.window().set_size(slint::LogicalSize::new(390.0, 844.0));
    ui.set_screen(8);
    ui.set_screen_title("Albums".into());
    ui.set_navigation_key("Albums".into());
    ui.set_rows(ModelRc::new(VecModel::from(
        (0..20)
            .map(|index| LibraryRow {
                title: format!("Album {index}").into(),
                chevron: index != 2,
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));
    let items = Rc::new(RefCell::new(Vec::new()));
    let captured = items.clone();
    ui.on_item_clicked(move |index| captured.borrow_mut().push(index));
    let commands = Rc::new(RefCell::new(Vec::new()));
    let captured = commands.clone();
    ui.on_command(move |command| captured.borrow_mut().push(command.to_string()));
    let rest = ui.window().take_snapshot().unwrap();
    lcd_event(&ui, 180.0, 136.0, "press");
    let pressed = ui.window().take_snapshot().unwrap();
    assert!(
        rest.as_bytes() != pressed.as_bytes(),
        "touch-down shows immediate feedback"
    );
    lcd_event(&ui, 150.0, 136.0, "move");
    lcd_event(&ui, 110.0, 137.0, "move");
    assert!(
        ui.get_list_navigation_offset() < -20.0,
        "list follows the left drag"
    );
    snapshot(&ui, "list-swipe-forward.png");
    lcd_event(&ui, 70.0, 138.0, "release");
    assert_eq!(
        &*items.borrow(),
        &[1],
        "enter the row where the swipe began"
    );
    assert!(commands.borrow().is_empty());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));

    lcd_event(&ui, 55.0, 136.0, "press");
    lcd_event(&ui, 95.0, 136.0, "move");
    lcd_event(&ui, 135.0, 137.0, "move");
    assert!(ui.get_list_navigation_offset() > 20.0);
    lcd_event(&ui, 180.0, 138.0, "release");
    assert_eq!(&*commands.borrow(), &["back"]);
    assert_eq!(&*items.borrow(), &[1], "a right swipe must not click a row");
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));

    // A song with no next-level chevron is never activated by a left swipe.
    lcd_event(&ui, 180.0, 184.0, "press");
    lcd_event(&ui, 140.0, 184.0, "move");
    lcd_event(&ui, 70.0, 184.0, "release");
    assert_eq!(&*items.borrow(), &[1]);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));

    // A slow incomplete swipe springs back without navigation.
    lcd_event(&ui, 180.0, 136.0, "press");
    lcd_event(&ui, 165.0, 136.0, "move");
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(180));
    lcd_event(&ui, 160.0, 136.0, "release");
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    assert!(ui.get_list_navigation_offset().abs() < 0.1);
    assert_eq!(&*items.borrow(), &[1]);

    lcd_event(&ui, 120.0, 300.0, "press");
    lcd_event(&ui, 118.0, 280.0, "move");
    lcd_event(&ui, 95.0, 220.0, "move");
    lcd_event(&ui, 75.0, 190.0, "release");
    assert!(
        ui.get_list_scroll_position() < -50.0,
        "vertical pan stays vertical despite horizontal drift"
    );
    assert_eq!(&*items.borrow(), &[1]);
    assert_eq!(&*commands.borrow(), &["back"]);

    // Different album lists have the same screen number. Changing the page
    // identity cancels old input and starts the new list at its top.
    ui.set_navigation_key("Another album".into());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(300));
    assert_eq!(ui.get_list_scroll_position(), 0.0);
    assert!(ui.get_list_navigation_offset().abs() < 0.1);
    for _ in 0..2 {
        lcd_event(&ui, 120.0, 184.0, "press");
        lcd_event(&ui, 120.0, 184.0, "release");
    }
    assert_eq!(
        &*items.borrow(),
        &[1, 2, 2],
        "ordinary song taps remain immediate, including repeated coordinates"
    );
}

const PASTED_LYRICS: &str = "[00:01.00]First line\n[00:02.00]第二行\n";

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

fn type_text(ui: &MobileWindow, text: &str) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: text.into() });
}

fn snapshot(ui: &MobileWindow, filename: &str) {
    let pixels = ui.window().take_snapshot().unwrap();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ios-preview");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(filename),
        pixels.as_bytes(),
        pixels.width(),
        pixels.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

fn bottom_button(ui: &MobileWindow, right: bool) {
    click(
        ui,
        ui.get_modal_left() + ui.get_modal_width() * if right { 0.75 } else { 0.25 },
        ui.get_modal_top() + ui.get_modal_height() - 32.0,
    );
}

fn assert_keyboard_clearance(ui: &MobileWindow, window_height: f32, keyboard_height: f32) {
    assert!(ui.get_modal_height() > 150.0);
    let bottom = ui.get_modal_top() + ui.get_modal_height();
    assert!(
        bottom <= window_height - keyboard_height,
        "modal bottom {bottom} intersects keyboard"
    );
    assert!(ui.get_modal_width() >= 280.0);
}

#[test]
fn touch_editors_keep_actions_above_keyboard_and_preserve_multiline_input() {
    let backend = i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    );
    backend.set_clipboard_text(PASTED_LYRICS, Clipboard::DefaultClipboard);
    slint::platform::set_platform(Box::new(backend)).unwrap();
    let ui = MobileWindow::new().unwrap();
    connect_translations(&ui);
    ui.set_clock_text("12:59 PM".into());
    ui.set_safe_top(44.0);
    ui.set_safe_bottom(34.0);
    ui.show().unwrap();
    ui.window().set_size(slint::LogicalSize::new(390.0, 844.0));
    let api_commands = Rc::new(RefCell::new(Vec::new()));
    let captured = api_commands.clone();
    ui.on_api_command(move |command| captured.borrow_mut().push(command.to_string()));
    let lyric_commands = Rc::new(RefCell::new(Vec::new()));
    let captured = lyric_commands.clone();
    ui.on_lyric_editor_command(move |command| captured.borrow_mut().push(command.to_string()));

    ui.set_api_settings_open(true);
    ui.set_api_model("deepseek-chat".into());
    ui.window().take_snapshot().unwrap();
    ui.invoke_focus_api_editor();
    type_text(&ui, "sk-demo-test");
    assert_eq!(ui.get_api_key().as_str(), "sk-demo-test");
    assert!(ui.get_api_editor_focused());
    snapshot(&ui, "api-editor.png");
    let device_width = ui.get_device_width();
    ui.set_keyboard_height(300.0);
    ui.window().take_snapshot().unwrap();
    assert_keyboard_clearance(&ui, 844.0, 300.0);
    assert_eq!(
        ui.get_device_width(),
        device_width,
        "keyboard must not shrink the nano"
    );
    snapshot(&ui, "api-keyboard.png");
    // Done remains fixed while the form can scroll independently.
    click(
        &ui,
        ui.get_modal_left() + ui.get_modal_width() - 48.0,
        ui.get_modal_top() + 26.0,
    );
    assert!(!ui.get_api_editor_focused());
    bottom_button(&ui, true);
    assert_eq!(
        api_commands.borrow().last().map(String::as_str),
        Some("save")
    );
    bottom_button(&ui, false);
    assert_eq!(
        api_commands.borrow().last().map(String::as_str),
        Some("cancel")
    );
    ui.set_api_settings_open(false);
    ui.set_keyboard_height(0.0);

    ui.set_lyric_editor_open(true);
    ui.window().take_snapshot().unwrap();
    ui.invoke_focus_lyric_editor();
    click(
        &ui,
        ui.get_modal_left() + ui.get_modal_width() - 136.0,
        ui.get_modal_top() + 26.0,
    );
    assert_eq!(ui.get_lyric_editor_text().as_str(), PASTED_LYRICS);
    assert!(ui.get_lyric_editor_focused());
    snapshot(&ui, "lyrics-editor.png");
    ui.set_keyboard_height(300.0);
    ui.window().take_snapshot().unwrap();
    assert_keyboard_clearance(&ui, 844.0, 300.0);
    snapshot(&ui, "lyrics-keyboard.png");
    click(
        &ui,
        ui.get_modal_left() + ui.get_modal_width() - 48.0,
        ui.get_modal_top() + 26.0,
    );
    assert!(!ui.get_lyric_editor_focused());
    bottom_button(&ui, true);
    assert_eq!(
        lyric_commands.borrow().last().map(String::as_str),
        Some("save")
    );
    bottom_button(&ui, false);
    assert_eq!(
        lyric_commands.borrow().last().map(String::as_str),
        Some("cancel")
    );
    ui.set_lyric_editor_open(false);
    ui.set_keyboard_height(0.0);

    let playlist_commands = Rc::new(RefCell::new(Vec::new()));
    let captured = playlist_commands.clone();
    ui.on_playlist_command(move |command, id| {
        captured
            .borrow_mut()
            .push((command.to_string(), id.to_string()))
    });
    ui.set_playlist_name("Road Trip".into());
    ui.set_playlist_candidates(ModelRc::new(VecModel::from(vec![
        PlaylistCandidate {
            id: "one".into(),
            title: "First Song".into(),
            artist: "Artist".into(),
            selected: true,
        },
        PlaylistCandidate {
            id: "two".into(),
            title: "Second Song".into(),
            artist: "Artist".into(),
            selected: false,
        },
    ])));
    ui.set_playlist_draft(ModelRc::new(VecModel::from(vec![PlaylistCandidate {
        id: "one".into(),
        title: "First Song".into(),
        artist: "Artist".into(),
        selected: true,
    }])));
    ui.invoke_open_playlist_editor();
    assert!(ui.get_playlist_name_focused());
    snapshot(&ui, "playlist-editor.png");
    ui.window().set_size(slint::LogicalSize::new(320.0, 568.0));
    ui.set_safe_top(20.0);
    ui.set_safe_bottom(0.0);
    ui.set_keyboard_height(260.0);
    ui.window().take_snapshot().unwrap();
    assert_keyboard_clearance(&ui, 568.0, 260.0);
    snapshot(&ui, "playlist-keyboard-small.png");
    // At 320 pt, the 44 pt Review action remains above the keyboard.
    click(
        &ui,
        ui.get_modal_left() + ui.get_modal_width() * 0.75,
        ui.get_modal_top() + ui.get_modal_height() - 39.0 * 1.25 + 22.0,
    );
    assert_eq!(
        playlist_commands
            .borrow()
            .last()
            .map(|(command, _)| command.as_str()),
        Some("review")
    );
    assert!(ui.get_playlist_reviewing());
    assert!(!ui.get_playlist_name_focused());
    ui.set_keyboard_height(0.0);
    assert!(ui.get_modal_height() > 500.0);
    snapshot(&ui, "playlist-review-small.png");
    ui.set_playlist_editor_open(false);
}

#[test]
fn playback_touch_targets_extend_past_small_slider_and_mute_glyphs() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = MobileWindow::new().unwrap();
    connect_translations(&ui);
    ui.set_safe_top(44.0);
    ui.set_safe_bottom(34.0);
    ui.set_screen(2);
    ui.set_has_track(true);
    ui.set_overlay_visible(true);
    ui.set_lyrics_visible(false);
    ui.show().unwrap();
    ui.window().set_size(slint::LogicalSize::new(390.0, 844.0));
    let seeks = Rc::new(RefCell::new(Vec::new()));
    let captured = seeks.clone();
    ui.on_seek_changed(move |value| captured.borrow_mut().push(value));
    let volumes = Rc::new(RefCell::new(Vec::new()));
    let captured = volumes.clone();
    ui.on_volume_changed(move |value| captured.borrow_mut().push(value));
    let commands = Rc::new(RefCell::new(Vec::new()));
    let captured = commands.clone();
    ui.on_command(move |command| captured.borrow_mut().push(command.to_string()));
    ui.window().take_snapshot().unwrap();
    let x = ui.get_display_left();
    let y = ui.get_display_top();
    let s = ui.get_scale();
    click(&ui, x + 120.0 * s, y + 121.0 * s + 20.0);
    assert!(
        !seeks.borrow().is_empty(),
        "seek hit region should be at least 44 pt tall"
    );
    click(&ui, x + 106.0 * s, y + 413.0 * s + 20.0);
    assert!(
        !volumes.borrow().is_empty(),
        "volume hit region should be at least 44 pt tall"
    );
    click(&ui, x + 220.0 * s - 18.0, y + 414.0 * s);
    assert_eq!(commands.borrow().last().map(String::as_str), Some("mute"));
    click(&ui, x + 30.0 * s - 18.0, y + 163.0 * s + 18.0);
    assert_eq!(commands.borrow().last().map(String::as_str), Some("repeat"));
    click(&ui, x + 150.0 * s + 18.0, y + 163.0 * s + 18.0);
    assert_eq!(
        commands.borrow().last().map(String::as_str),
        Some("shuffle")
    );
}

#[test]
fn playlist_name_field_accepts_a_real_tap_across_its_full_mobile_height() {
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
    ui.set_safe_top(44.0);
    ui.set_safe_bottom(34.0);
    ui.show().unwrap();
    ui.window().set_size(slint::LogicalSize::new(390.0, 844.0));
    ui.invoke_open_playlist_editor();
    ui.window().take_snapshot().unwrap();
    ui.invoke_dismiss_keyboard();
    assert!(!ui.get_playlist_name_focused());
    let s = ui.get_modal_scale().min(1.25);
    let field_top = ui.get_modal_top() + (32.0 * s).max(44.0) + 33.0 * s;
    let field_height = (32.0 * s).max(44.0);
    for (x, fraction) in [
        (ui.get_modal_left() + ui.get_modal_width() / 2.0, 0.5),
        (ui.get_modal_left() + ui.get_modal_width() / 2.0, 0.9),
        (ui.get_modal_left() + 12.0 * s, 0.5),
    ] {
        ui.invoke_dismiss_keyboard();
        click(&ui, x, field_top + field_height * fraction);
        assert!(
            ui.get_playlist_name_focused(),
            "tapping the name field at {fraction} of its height must focus it"
        );
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: "Dinner mix".into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: "Dinner mix".into(),
    });
    assert_eq!(ui.get_playlist_name(), "Dinner mix");
    ui.window().set_size(slint::LogicalSize::new(320.0, 568.0));
    ui.set_safe_top(20.0);
    ui.set_safe_bottom(0.0);
    ui.set_keyboard_height(260.0);
    ui.window().take_snapshot().unwrap();
    assert_keyboard_clearance(&ui, 568.0, 260.0);
    assert!(ui.get_playlist_name_focused());
    assert_eq!(ui.get_playlist_name(), "Dinner mix");
    snapshot(&ui, "playlist-name-tap-keyboard.png");
}

#[test]
fn timed_lyrics_follow_the_active_line_resume_after_manual_scroll_and_reset_for_a_new_song() {
    use crate::library::lyrics::Lyrics;

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
    ui.set_safe_top(44.0);
    ui.set_safe_bottom(34.0);
    ui.set_screen(2);
    ui.set_has_track(true);
    ui.set_playing(true);
    ui.set_overlay_visible(true);
    ui.set_lyrics_visible(true);
    let source = (0..45)
        .map(|index| {
            let second = 31 + index * 3;
            format!(
                "[{:02}:{:02}.310]Lyric {index} 中文",
                second / 60,
                second % 60
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let lyrics = Lyrics::parse(&source);
    ui.set_lyric_lines(ModelRc::new(VecModel::from(
        lyrics
            .lines()
            .into_iter()
            .map(slint::SharedString::from)
            .collect::<Vec<_>>(),
    )));
    ui.set_timed_lyrics(true);
    // The source's first timestamp is 31.310s: 15s must not invent a highlight.
    assert_eq!(lyrics.active(15.0), None);
    ui.set_active_lyric(-1);
    ui.show().unwrap();
    ui.window().set_size(slint::LogicalSize::new(390.0, 844.0));

    let advance = |milliseconds| {
        for _ in 0..milliseconds / 20 {
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
        }
        ui.window().take_snapshot().unwrap();
    };
    // Inspect rendered geometry without requiring Slint compiler debug info.
    let marker_y = || {
        let pixels = ui.window().take_snapshot().unwrap();
        let x = (ui.get_display_left() + 1.5 * ui.get_scale()).round() as usize;
        let first_y = (ui.get_display_top() + 184.0 * ui.get_scale()).ceil() as usize;
        let last_y = (ui.get_display_top() + 322.0 * ui.get_scale()).floor() as usize;
        (first_y..last_y).find_map(|y| {
            let pixel = (y * pixels.width() as usize + x) * 4;
            (pixels.as_bytes()[pixel..pixel + 3] == [184, 234, 255]).then_some(y as f32)
        })
    };
    advance(300);
    let viewport_top = ui.get_display_top() + 184.0 * ui.get_scale();
    assert!(
        marker_y().is_none(),
        "no highlight before the first timestamp"
    );
    snapshot(&ui, "lyrics-before-first-timestamp.png");

    // Feed the same parsed source/position projection used by the controller.
    let active = lyrics.active(134.0).unwrap();
    assert!(active > 30);
    ui.set_active_lyric(active as i32);
    advance(400);
    let follow_y = viewport_top + 32.0 * ui.get_scale();
    assert!(
        (marker_y().expect("active lyric marker must be visible") - follow_y).abs() < 2.0,
        "the timestamp-selected lyric must scroll into the follow position"
    );
    snapshot(&ui, "lyrics-follow-active.png");

    // Advancing the timestamp while a finger owns scrolling must preserve the
    // manual viewport. It still updates the highlight and the eventual target.
    lcd_event(&ui, 120.0, 225.0, "press");
    lcd_event(&ui, 120.0, 250.0, "move");
    lcd_event(&ui, 120.0, 282.0, "move");
    ui.window().take_snapshot().unwrap();
    let manual_y = marker_y().expect("manually moved lyric remains visible");
    assert!(manual_y > follow_y + 30.0 * ui.get_scale());
    ui.set_active_lyric((active + 1) as i32);
    advance(200);
    assert!(
        marker_y().expect("next lyric remains in the manual viewport")
            > manual_y + 20.0 * ui.get_scale(),
        "a new timestamp must not pull the page away from the user's finger"
    );
    lcd_event(&ui, 120.0, 282.0, "release");
    // Allow inertia to settle, then the explicit three-second idle period.
    advance(10000);
    assert!(
        (marker_y().expect("resumed lyric marker must be visible") - follow_y).abs() < 2.0,
        "manual scrolling must resume following even without another timestamp"
    );
    snapshot(&ui, "lyrics-follow-resumed.png");

    // A new song starts before its first timestamp, even if the preceding song
    // was far down its lyric sheet. Old follow targets must not survive it.
    let next_song = ModelRc::new(VecModel::from(
        (0..45)
            .map(|index| slint::SharedString::from(format!("New song {index}")))
            .collect::<Vec<_>>(),
    ));
    ui.set_lyric_lines(next_song.clone());
    ui.set_active_lyric(-1);
    advance(400);
    assert!(marker_y().is_none());
    advance(4000);
    // Compare the lyric viewport with a fresh window at this song's top.
    let fresh = MobileWindow::new().unwrap();
    connect_translations(&fresh);
    fresh.set_safe_top(44.0);
    fresh.set_safe_bottom(34.0);
    fresh.set_screen(2);
    fresh.set_has_track(true);
    fresh.set_playing(true);
    fresh.set_overlay_visible(true);
    fresh.set_lyrics_visible(true);
    fresh.set_lyric_lines(next_song);
    fresh.set_active_lyric(-1);
    fresh.show().unwrap();
    fresh
        .window()
        .set_size(slint::LogicalSize::new(390.0, 844.0));
    advance(400);
    let actual = ui.window().take_snapshot().unwrap();
    let expected = fresh.window().take_snapshot().unwrap();
    let left = ui.get_display_left().ceil() as usize;
    let right = (ui.get_display_left() + 240.0 * ui.get_scale()).floor() as usize;
    let bottom = (ui.get_display_top() + 322.0 * ui.get_scale()).floor() as usize;
    for y in viewport_top.ceil() as usize..bottom {
        let start = (y * actual.width() as usize + left) * 4;
        let end = (y * actual.width() as usize + right) * 4;
        assert_eq!(
            &actual.as_bytes()[start..end],
            &expected.as_bytes()[start..end],
            "a new song must show its first lyrics rather than the preceding scroll position"
        );
    }
}
