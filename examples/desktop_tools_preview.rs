//! Render added desktop tools with offline fixture data; no controller, audio, API or sync job.
#[cfg(any(target_os = "ios", feature = "mobile"))]
fn main() {}

#[cfg(not(any(target_os = "ios", feature = "mobile")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use nano_music_player::{
        app::i18n::{Language, translate},
        ui::{
            AppWindow, GeniusRow, I18n, IPhoneSyncWindow, PlaylistCandidate, ShortcutRow,
            SubtitleChoice, SyncDeviceRow, SyncPlaylistRow,
        },
    };
    use slint::{ComponentHandle, ModelRc, VecModel};
    use std::path::Path;

    fn click(window: &slint::Window, x: f32, y: f32) {
        use slint::platform::{PointerEventButton, WindowEvent};
        let position = slint::LogicalPosition::new(x, y);
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
    }

    fn save(
        window: &slint::Window,
        directory: &Path,
        name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let frame = window.take_snapshot()?;
        image::save_buffer(
            directory.join(format!("{name}.png")),
            frame.as_bytes(),
            frame.width(),
            frame.height(),
            image::ColorType::Rgba8,
        )?;
        Ok(())
    }
    // The software backend makes this gallery reproducible and independent of desktop sessions.
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))?;
    let directory = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target/desktop-tools-preview".into());
    std::fs::create_dir_all(&directory)?;
    let ui = AppWindow::new()?;
    ui.global::<I18n>()
        .on_translate(|language, key| translate(Language::ALL[language as usize], &key).into());
    ui.set_clock_text("1:25 PM".into());
    ui.set_smooth_slides(false);
    ui.set_has_track(true);
    ui.set_track_title("Evening Light".into());
    ui.set_track_artist("Studio Notes".into());
    ui.set_track_path("/fixture/evening.wav".into());
    ui.set_shortcuts(ModelRc::new(VecModel::from(vec![
        ShortcutRow {
            category: "Playback".into(),
            title: "Play / Pause".into(),
            keys: "Space".into(),
        },
        ShortcutRow {
            category: "Navigation".into(),
            title: "Home".into(),
            keys: "Shift + H".into(),
        },
        ShortcutRow {
            category: "Lyrics".into(),
            title: "Show / Hide Lyrics".into(),
            keys: "Y".into(),
        },
    ])));
    let candidates: Vec<_> = [
        "Evening Light",
        "After the Rain",
        "Sunday Morning",
        "A Quiet Place",
    ]
    .iter()
    .enumerate()
    .map(|(index, title)| PlaylistCandidate {
        id: index.to_string().into(),
        title: (*title).into(),
        artist: "Studio Notes".into(),
        selected: index < 2,
    })
    .collect();
    ui.set_playlist_candidates(ModelRc::new(VecModel::from(candidates.clone())));
    ui.set_playlist_draft(ModelRc::new(VecModel::from(candidates[..2].to_vec())));
    for language in [
        Language::English,
        Language::SimplifiedChinese,
        Language::Japanese,
    ] {
        ui.global::<I18n>().set_language(language.index() as i32);
        for device in [true, false] {
            ui.set_device_mode(device);
            ui.window().set_size(slint::LogicalSize::new(
                if device { 296.0 } else { 240.0 },
                if device { 572.0 } else { 432.0 },
            ));
            ui.show()?;
            let suffix = format!(
                "{}-{}",
                language.code(),
                if device { "device" } else { "screen" }
            );
            ui.set_deepseek_settings_open(true);
            ui.set_deepseek_key_configured(true);
            ui.set_deepseek_model("deepseek-flash".into());
            save(ui.window(), &directory, &format!("ai-services-{suffix}"))?;
            ui.set_deepseek_settings_open(false);
            ui.set_shortcut_editor_open(true);
            ui.set_shortcut_title(translate(language, "Play / Pause").into());
            ui.set_shortcut_keys("Space".into());
            ui.set_shortcut_message(translate(language, "Changes save automatically.").into());
            save(
                ui.window(),
                &directory,
                &format!("shortcut-editor-{suffix}"),
            )?;
            ui.set_shortcut_editor_open(false);
            ui.set_podcast_subscribe_open(true);
            ui.set_podcast_preview_ready(true);
            ui.set_podcast_url("https://example.com/feed.xml".into());
            ui.set_podcast_preview_title("Design Notes".into());
            ui.set_podcast_preview_description(
                "Conversations on design, music and everyday creativity.".into(),
            );
            save(
                ui.window(),
                &directory,
                &format!("podcast-subscription-{suffix}"),
            )?;
            ui.set_podcast_subscribe_open(false);
            ui.invoke_open_genius_preview();
            save(ui.window(), &directory, &format!("genius-create-{suffix}"))?;
            ui.set_genius_songs(ModelRc::new(VecModel::from(
                candidates
                    .iter()
                    .map(|song| GeniusRow {
                        title: song.title.clone(),
                        artist: song.artist.clone(),
                        path: song.id.clone(),
                    })
                    .collect::<Vec<_>>(),
            )));
            ui.invoke_prepare_genius_action("create".into());
            save(ui.window(), &directory, &format!("genius-ready-{suffix}"))?;
            ui.set_genius_preview_open(false);
            ui.invoke_open_playlist_editor();
            ui.set_playlist_name("Evening Mix".into());
            save(
                ui.window(),
                &directory,
                &format!("playlist-select-{suffix}"),
            )?;
            ui.invoke_open_saved_playlist_editor();
            save(
                ui.window(),
                &directory,
                &format!("playlist-review-{suffix}"),
            )?;
            ui.set_playlist_editor_open(false);
            ui.set_playlist_editing(true);
            ui.invoke_open_saved_playlist_editor();
            save(ui.window(), &directory, &format!("playlist-edit-{suffix}"))?;
            ui.set_playlist_editor_open(false);
            ui.set_playlist_editing(false);
            ui.set_subtitle_choices(ModelRc::new(VecModel::from(vec![
                SubtitleChoice {
                    id: -1,
                    label: translate(language, "Off").into(),
                    selected: false,
                },
                SubtitleChoice {
                    id: 0,
                    label: "English · Embedded".into(),
                    selected: true,
                },
                SubtitleChoice {
                    id: 1,
                    label: "简体中文 · ASS".into(),
                    selected: false,
                },
            ])));
            ui.set_subtitle_menu_open(true);
            save(ui.window(), &directory, &format!("subtitles-{suffix}"))?;
            ui.set_subtitle_menu_open(false);
            ui.set_reference_open(true);
            save(
                ui.window(),
                &directory,
                &format!("shortcut-reference-{suffix}"),
            )?;
            ui.set_reference_open(false);
        }
        let sync = IPhoneSyncWindow::new()?;
        sync.global::<I18n>()
            .on_translate(|language, key| translate(Language::ALL[language as usize], &key).into());
        sync.global::<I18n>().set_language(language.index() as i32);
        sync.set_device_name("iPhone".into());
        sync.set_device_detail("USB · Ready".into());
        sync.set_library_song_count(128);
        sync.set_library_playlist_count(6);
        sync.set_library_size("2.4 GB".into());
        sync.set_devices(ModelRc::new(VecModel::from(vec![SyncDeviceRow {
            id: "fixture".into(),
            name: "iPhone".into(),
            subtitle: "USB".into(),
            selected: true,
        }])));
        sync.set_playlists(ModelRc::new(VecModel::from(vec![SyncPlaylistRow {
            id: "fixture".into(),
            name: "Evening Mix".into(),
            detail: "24 songs".into(),
            selected: true,
        }])));
        sync.set_scope_all(false);
        sync.show()?;
        save(
            sync.window(),
            &directory,
            &format!("iphone-sync-{}", language.code()),
        )?;
        sync.window()
            .set_size(slint::LogicalSize::new(720.0, 560.0));
        save(
            sync.window(),
            &directory,
            &format!("iphone-sync-small-{}", language.code()),
        )?;
        // Exercise the real tab with fixture state; there are no sync callbacks or jobs.
        click(sync.window(), 410.0, 95.0);
        save(
            sync.window(),
            &directory,
            &format!("iphone-sync-music-{}", language.code()),
        )?;
        sync.set_busy(true);
        sync.set_can_cancel(true);
        sync.set_progress_known(true);
        sync.set_progress(0.42);
        sync.set_phase(translate(language, "Transferring over USB").into());
        sync.set_conflicts("Evening Light · rating kept on iPhone".into());
        save(
            sync.window(),
            &directory,
            &format!("iphone-sync-busy-{}", language.code()),
        )?;
        sync.hide()?;
    }
    println!("Rendered desktop tools to {}", directory.display());
    Ok(())
}
