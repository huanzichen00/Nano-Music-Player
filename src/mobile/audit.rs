//! Opt-in, debug-only native rendering fixture. Does not open the user's library
//! or read the clipboard, credentials, network, or audio output.
use super::connect_translations;
use crate::ui::{LibraryRow, MobileWindow};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::time::Duration;

pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MobileWindow::new()?;
    connect_translations(&ui);
    let insets = crate::platform::ios::safe_area();
    ui.set_safe_top(insets.top);
    ui.set_safe_right(insets.right);
    ui.set_safe_bottom(insets.bottom);
    ui.set_safe_left(insets.left);
    let scene = std::env::args()
        .skip_while(|arg| arg != "--ui-audit")
        .nth(1)
        .unwrap_or_else(|| "player".into());
    ui.set_clock_text("1:25 PM".into());
    ui.set_screen_title("Display audit".into());
    ui.set_track_title("A long song title · 中文 日本語".into());
    ui.set_track_artist("Artist".into());
    ui.set_track_album("Album".into());
    ui.set_has_track(true);
    ui.set_overlay_visible(true);
    ui.set_lyrics_visible(true);
    ui.set_timed_lyrics(true);
    ui.set_duration("5:56".into());
    ui.set_track_count("1 / 142".into());
    ui.set_lyric_lines(ModelRc::new(VecModel::from(
        (0..30)
            .map(|index| {
                format!("Line {index}: Nobody knows the way it is going to be · 中文 日本語").into()
            })
            .collect::<Vec<slint::SharedString>>(),
    )));
    ui.set_translation_visible(true);
    ui.set_translated_lines(ModelRc::new(VecModel::from(
        (0..30)
            .map(|index| {
                format!("第 {index} 行：歌词与翻译各自换行。時間に合わせて表示します。").into()
            })
            .collect::<Vec<slint::SharedString>>(),
    )));
    let mut cover = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(240, 240);
    for (id, pixel) in cover.make_mut_slice().iter_mut().enumerate() {
        *pixel = slint::Rgb8Pixel {
            r: 35 + (id % 240 / 3) as u8,
            g: 55,
            b: 95 + (id / 240 / 2) as u8,
        };
    }
    ui.set_artwork(slint::Image::from_rgb8(cover));
    ui.set_rows(ModelRc::new(VecModel::from(
        (0..30)
            .map(|index| LibraryRow {
                title: format!("Album {index} · 中文 日本語").into(),
                subtitle: "A long artist name for wrapping and clipping".into(),
                show_artwork: true,
                chevron: true,
                artwork: ui.get_artwork(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));
    match scene.as_str() {
        "home" => ui.set_screen(0),
        "list" => ui.set_screen(8),
        "api" => {
            ui.set_screen(5);
            ui.set_api_settings_open(true);
            ui.set_api_model("deepseek-chat".into());
        }
        _ => ui.set_screen(2),
    }
    let weak = ui.as_weak();
    ui.on_command(move |command| {
        if let Some(ui) = weak.upgrade() {
            match command.as_str() {
                "back" | "home" => ui.set_screen(0),
                _ => {}
            }
        }
    });
    let weak = ui.as_weak();
    let timer = slint::Timer::default();
    let mut tick = 0;
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            if let Some(ui) = weak.upgrade() {
                tick += 1;
                ui.set_progress((tick % 3560) as f32 / 3560.0);
                ui.set_elapsed(format!("{}:{:02}", tick / 600, tick / 10 % 60).into());
                ui.set_active_lyric((tick / 40) % 30);
            }
        },
    );
    ui.run()?;
    timer.stop();
    Ok(())
}
