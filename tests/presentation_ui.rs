#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use nano_music_player::{
    app::i18n::{Language, translate},
    ui::{AppWindow, I18n},
};
use slint::{
    ComponentHandle, ModelRc, Rgb8Pixel, SharedPixelBuffer, VecModel,
    platform::{Key, WindowEvent},
};
use std::{cell::RefCell, rc::Rc, time::Duration};

fn tick(ms: u64) {
    for _ in 0..ms.div_ceil(16) {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
        slint::platform::update_timers_and_animations();
    }
}

fn snapshot(ui: &AppWindow, name: &str) {
    let frame = ui.window().take_snapshot().unwrap();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa/presentation");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        frame.as_bytes(),
        frame.width(),
        frame.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

fn lyrics(ui: &AppWindow, lines: &[&str]) {
    ui.set_lyric_lines(ModelRc::new(VecModel::from(
        lines.iter().map(|line| (*line).into()).collect::<Vec<_>>(),
    )));
    tick(300);
}

fn assert_centered(ui: &AppWindow) {
    // The testing backend renders only on demand; layout must resolve the
    // wrapped row heights before the follow target can be published.
    ui.window().take_snapshot().unwrap();
    tick(300);
    ui.window().take_snapshot().unwrap();
    assert!(
        (ui.get_presentation_active_center() - ui.get_presentation_viewport_height() / 2.0).abs()
            < 1.0,
        "active lyric must remain centered: center={} viewport={}",
        ui.get_presentation_active_center(),
        ui.get_presentation_viewport_height()
    );
}

#[test]
fn clean_landscape_lyrics_wrap_follow_and_preserve_keyboard_routing() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    ui.global::<I18n>()
        .on_translate(|language, key| translate(Language::ALL[language as usize], &key).into());
    ui.set_presentation_mode(true);
    // Device chrome and transport stay hidden; metadata uses a compact header.
    ui.set_device_mode(true);
    ui.set_screen(2);
    ui.set_has_track(true);
    ui.set_track_title("七里香".into());
    ui.set_track_artist("周杰伦".into());
    ui.set_overlay_visible(true);
    ui.set_lyrics_visible(false);
    let mut pixels = SharedPixelBuffer::<Rgb8Pixel>::new(160, 160);
    for (index, pixel) in pixels.make_mut_slice().iter_mut().enumerate() {
        let x = (index % 160) as u8;
        let y = (index / 160) as u8;
        *pixel = Rgb8Pixel::new(45 + x / 2, 80 + y / 3, 140 + x / 3);
    }
    ui.set_artwork(slint::Image::from_rgb8(pixels));
    ui.show().unwrap();
    let menu_height = ui
        .window()
        .size()
        .to_logical(ui.window().scale_factor())
        .height
        - ui.get_content_height();
    let received = Rc::new(RefCell::new(Vec::new()));
    let events = received.clone();
    ui.on_key_event(move |event, editing| {
        assert!(!editing);
        events.borrow_mut().push(event.text.to_string());
        true
    });
    ui.invoke_focus_screen();
    let english = [
        "A quiet introduction",
        "We follow every note along the road",
        "A longer English lyric keeps all of its words and wraps across several lines without cutting the ending away",
        "The next phrase waits below",
        "And the final note returns us home",
    ];
    for scale in [0.75, 1.0, 1.5] {
        ui.window().set_size(slint::LogicalSize::new(
            540.0 * scale,
            180.0 * scale + menu_height,
        ));
        lyrics(&ui, &english);
        for active in [0, 1, 2, 4, 3, 0] {
            ui.set_active_lyric(active);
            tick(300);
            assert_centered(&ui);
        }
        for active in [4, 1, 3, 2] {
            ui.set_active_lyric(active);
            tick(16);
        }
        assert_centered(&ui);
        snapshot(&ui, &format!("english-{scale}"));
        assert!(ui.get_presentation_lyric_content_height() > 180.0 * scale);
    }
    let chinese = [
        "晚风轻轻吹过窗前",
        "沿着旋律慢慢向前",
        "这是一句比较长的中文歌词，应该完整换行显示，唱到最后一个字也不会被省略",
        "指尖留下温柔的回声",
        "等下一次和弦响起",
    ];
    let japanese = [
        "静かな夜に響く音",
        "君の声を追いかけて",
        "長い歌詞の言葉も途中で切らずに折り返して最後まで表示できるようにする",
        "次のフレーズを待ちながら",
        "また明日へ歩き出す",
    ];
    for (language, lines) in [(1, &chinese), (2, &japanese)] {
        ui.global::<I18n>().set_language(language);
        lyrics(&ui, lines);
        ui.set_active_lyric(2);
        tick(300);
        assert_centered(&ui);
        snapshot(&ui, &format!("long-{language}"));
    }
    ui.set_track_title("A very long song title that should leave room for the artist".into());
    ui.set_track_artist("Artist with a long name".into());
    snapshot(&ui, "long-metadata");
    ui.set_track_title("七里香".into());
    ui.set_track_artist("周杰伦".into());
    // Browsing pauses following, then restores the current lyric automatically.
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(
            (size.width - 540.0 * scale) / 2.0 + 360.0 * scale,
            size.height - ui.get_content_height() + 90.0 * scale,
        ),
        delta_x: 0.0,
        delta_y: -45.0 * scale,
    });
    tick(100);
    assert!(!ui.get_presentation_following());
    ui.set_active_lyric(3);
    tick(3500);
    assert!(ui.get_presentation_following());
    assert_centered(&ui);

    ui.set_practice_pending(true);
    for countdown in [3, 2, 1, 0] {
        ui.set_practice_countdown(countdown);
        tick(50);
        snapshot(&ui, &format!("countdown-{countdown}"));
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Space.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Space.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Escape.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Escape.into(),
    });
    assert_eq!(
        &*received.borrow(),
        &[
            char::from(Key::Space).to_string(),
            char::from(Key::Escape).to_string()
        ]
    );
    assert!(
        ui.get_presentation_mode(),
        "only the Rust command handler exits this mode"
    );
    ui.set_practice_pending(false);
    ui.set_track_title("".into());
    ui.set_track_artist("".into());
    assert_centered(&ui);
    lyrics(&ui, &[]);
    snapshot(&ui, "empty");
    ui.set_artwork(slint::Image::default());
    snapshot(&ui, "empty-cover");
}
