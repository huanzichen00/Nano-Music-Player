use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        controller::{Controller, connect},
        navigation::{Filter, Screen},
    },
    audio::PlaybackStatus,
    library::{Track, lyrics::Lyrics},
    ui::{AppWindow, SubtitleChoice},
    video::subtitles::SubtitlePreference,
    video::{Status, library::Video, orientation},
};
use slint::{
    ComponentHandle, Model,
    platform::{PointerEventButton, WindowEvent},
};
use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

fn setup(directory: &Path) -> (AppWindow, Rc<RefCell<Controller>>) {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let controller = Rc::new(RefCell::new(Controller::new(directory.to_path_buf())));
    connect(&ui, &controller);
    controller.borrow_mut().initialize(&ui);
    ui.show().unwrap();
    ui.invoke_focus_screen();
    (ui, controller)
}

fn assert_size(ui: &AppWindow, device: bool, landscape: bool, zoom: f32) {
    let (width, height) = orientation::window_base(device, landscape);
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    assert!((size.width - width * zoom).abs() < 1.0, "{size:?}");
    assert!(
        (ui.get_content_height() - height * zoom).abs() < 1.0,
        "{size:?}"
    );
    assert_eq!(ui.get_video_landscape(), landscape);
}

// Position in the full rotated body, with the native menu outside the content.
fn click_body(ui: &AppWindow, x: f32, y: f32) {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let scale = ui.get_display_scale();
    let position =
        slint::LogicalPosition::new(x * scale, size.height - ui.get_content_height() + y * scale);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn snapshot(ui: &AppWindow, name: &str) {
    let pixels = ui.window().take_snapshot().unwrap();
    let directory = std::env::temp_dir().join("nano-qa");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        pixels.as_bytes(),
        pixels.width(),
        pixels.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

fn open_video(ui: &AppWindow, controller: &RefCell<Controller>, row: usize) {
    let mut controller = controller.borrow_mut();
    controller.state.navigation.open(Screen::Videos);
    controller.state.navigation.select(row);
    controller.execute(ui, Action::Activate);
}

#[test]
fn video_list_import_row_uses_artwork_stride_and_contains_landscape_and_portrait_frames() {
    let directory = tempfile::tempdir().unwrap();
    let (ui, controller) = setup(directory.path());
    {
        let mut c = controller.borrow_mut();
        c.state.smooth_slides = false;
        c.state.merge_videos(vec![
            Video {
                path: directory.path().join("landscape.mp4"),
                title: "Landscape frame".into(),
                duration: 10.0,
                width: 320,
                height: 180,
                display_aspect: 16.0 / 9.0,
                video_codec: "h264".into(),
                audio_codec: None,
            },
            Video {
                path: directory.path().join("portrait.mp4"),
                title: "Portrait frame".into(),
                duration: 10.0,
                width: 180,
                height: 320,
                display_aspect: 9.0 / 16.0,
                video_codec: "h264".into(),
                audio_codec: None,
            },
        ]);
        c.state.navigation.open(Screen::Videos);
        c.execute(&ui, Action::FinishSearchEditing);
    }
    let activated = Rc::new(RefCell::new(Vec::new()));
    let clicks = activated.clone();
    // Observe row hit targets without opening a native playback session.
    ui.on_item_clicked(move |row| clicks.borrow_mut().push(row));
    let colors = [[239, 17, 33, 255], [19, 221, 47, 255]];
    for zoom in [0.75, 1.25, 2.0] {
        for mode in [Action::ScreenMode, Action::DeviceMode] {
            controller.borrow_mut().state.zoom = zoom;
            controller.borrow_mut().execute(&ui, mode);
            ui.set_list_scroll_y(0.0);
            assert!(!ui.get_rows().row_data(0).unwrap().show_artwork);
            for (index, (width, height)) in [(96, 54), (54, 96)].into_iter().enumerate() {
                let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
                pixels
                    .make_mut_bytes()
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .for_each(|pixel| pixel.copy_from_slice(&colors[index]));
                let mut row = ui.get_rows().row_data(index + 1).unwrap();
                row.artwork = slint::Image::from_rgba8(pixels);
                ui.get_rows().set_row_data(index + 1, row);
            }
            let scale = ui.get_display_scale();
            assert!((ui.get_list_row_height() - 58.0 * scale).abs() < 0.01);
            snapshot(
                &ui,
                &format!("video-list-geometry-{zoom}-{}", ui.get_device_mode()),
            );
            let rendered = ui.window().take_snapshot().unwrap();
            let factor = ui.window().scale_factor();
            let pixel_scale = scale * factor;
            let mut bounds = [(u32::MAX, u32::MAX, 0, 0); 2];
            for (index, pixel) in rendered.as_bytes().as_chunks::<4>().0.iter().enumerate() {
                for (color, bound) in colors.iter().zip(&mut bounds) {
                    if pixel == color {
                        let x = index as u32 % rendered.width();
                        let y = index as u32 / rendered.width();
                        bound.0 = bound.0.min(x);
                        bound.1 = bound.1.min(y);
                        bound.2 = bound.2.max(x);
                        bound.3 = bound.3.max(y);
                    }
                }
            }
            for (index, &(left, top, right, bottom)) in bounds.iter().enumerate() {
                assert!(
                    left != u32::MAX,
                    "missing frame at zoom {zoom}, device {}",
                    ui.get_device_mode()
                );
                let width = (right - left + 1) as f32;
                let height = (bottom - top + 1) as f32;
                let expected_width = if index == 0 { 80.0 } else { 45.0 * 9.0 / 16.0 };
                assert!(
                    (width - expected_width * pixel_scale).abs() <= 2.0,
                    "frame width {width}, zoom {zoom}"
                );
                assert!(
                    (height - 45.0 * pixel_scale).abs() <= 2.0,
                    "frame height {height}, zoom {zoom}"
                );
                let position = slint::LogicalPosition::new(
                    (left + right) as f32 / 2.0 / factor,
                    (top + bottom) as f32 / 2.0 / factor,
                );
                for event in [
                    WindowEvent::PointerPressed {
                        position,
                        button: PointerEventButton::Left,
                    },
                    WindowEvent::PointerReleased {
                        position,
                        button: PointerEventButton::Left,
                    },
                ] {
                    ui.window().dispatch_event(event);
                }
            }
            assert!(((bounds[1].1 - bounds[0].1) as f32 - 58.0 * pixel_scale).abs() <= 1.0);
        }
    }
    assert_eq!(*activated.borrow(), [1, 2].repeat(6));
}

#[test]
fn whole_device_rotation_controls_modes_zoom_and_portrait_restoration() {
    let directory = tempfile::tempdir().unwrap();
    let (ui, controller) = setup(directory.path());
    controller.borrow_mut().state.merge_videos(vec![
        Video {
            path: directory.path().join("landscape.mp4"),
            title: "Landscape fixture".into(),
            duration: 12.0,
            width: 320,
            height: 180,
            display_aspect: 16.0 / 9.0,
            video_codec: "h264".into(),
            audio_codec: None,
        },
        Video {
            path: directory.path().join("portrait.mp4"),
            title: "Portrait fixture".into(),
            duration: 12.0,
            width: 180,
            height: 320,
            display_aspect: 9.0 / 16.0,
            video_codec: "h264".into(),
            audio_codec: None,
        },
    ]);
    open_video(&ui, &controller, 1);
    assert_size(&ui, true, true, 1.25);
    // A deterministic RGB fixture lets the full app render without native codecs.
    let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(320, 180);
    for (index, pixel) in pixels.make_mut_slice().iter_mut().enumerate() {
        *pixel = slint::Rgba8Pixel {
            r: ((index % 320) * 255 / 320) as u8,
            g: ((index / 320) * 255 / 180) as u8,
            b: 100,
            a: 255,
        };
    }
    ui.set_video_frame(slint::Image::from_rgba8(pixels));
    snapshot(&ui, "video-integrated-landscape");
    click_body(&ui, 36.0 + 216.0, 28.0 + 180.0);
    assert!(
        controller.borrow().state.video_paused,
        "landscape play/pause must be clickable"
    );
    let generation = controller.borrow().state.video_generation;
    click_body(&ui, 36.0 + 60.0, 28.0 + 180.0);
    assert!(
        !controller.borrow().state.video_overlay,
        "the video beside the compact transport must remain tappable"
    );
    assert_eq!(controller.borrow().state.video_generation, generation);
    click_body(&ui, 36.0 + 216.0, 28.0 + 100.0);
    assert!(controller.borrow().state.video_overlay);
    click_body(&ui, 36.0 + 216.0, 28.0 + 17.0);
    assert!(
        (controller.borrow().state.video_position - 4.8).abs() < 0.2,
        "inline landscape seeking must work"
    );
    click_body(&ui, 36.0 + 216.0, 28.0 + 225.0);
    assert!(
        controller.borrow().state.volume > 0.5,
        "compact desktop volume must work"
    );
    controller.borrow_mut().save_settings(&ui);
    let settings = Settings::load(&directory.path().join("settings.json")).unwrap();
    assert!((settings.window_width.unwrap() - 370.0).abs() < 1.0);
    assert!(settings.window_height.unwrap() > 700.0);
    controller.borrow_mut().execute(&ui, Action::ScreenMode);
    assert_size(&ui, false, true, 1.25);
    snapshot(&ui, "video-integrated-screen-landscape");
    controller.borrow_mut().execute(&ui, Action::DeviceMode);
    controller.borrow_mut().execute(&ui, Action::ZoomIn);
    let zoom = controller.borrow().state.zoom;
    assert_size(&ui, true, true, zoom);
    // Done uses the LCD coordinates of the sideways device.
    click_body(&ui, 36.0 + 30.0, 28.0 + 17.0);
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::Videos
    );
    assert_size(&ui, true, false, zoom);
    open_video(&ui, &controller, 2);
    assert_size(&ui, true, false, zoom);
    snapshot(&ui, "video-integrated-portrait");
    open_video(&ui, &controller, 1);
    click_body(&ui, orientation::BODY_HEIGHT - 58.0, 148.0);
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::Home
    );
    assert_size(&ui, true, false, zoom);
    snapshot(&ui, "video-integrated-home-restored");
    open_video(&ui, &controller, 1);
    controller.borrow_mut().execute(&ui, Action::NewPlaylist);
    assert!(ui.get_playlist_editor_open());
    assert_size(&ui, true, false, zoom);
    assert!(controller.borrow().state.video_paused);
    assert_eq!(
        controller.borrow().state.navigation.current().screen,
        Screen::Videos
    );
    ui.invoke_playlist_action("cancel".into(), "".into());
    assert!(!ui.get_playlist_editor_open());
    open_video(&ui, &controller, 1);
    ui.invoke_command("show_shortcuts".into());
    assert!(ui.get_reference_open());
    assert!(controller.borrow().state.video_paused);
    assert_size(&ui, true, false, zoom);
    snapshot(&ui, "video-reference-restored");
    controller.borrow_mut().execute(&ui, Action::CloseShortcuts);
    open_video(&ui, &controller, 1);
    controller.borrow_mut().execute(&ui, Action::Shortcuts);
    assert_size(&ui, true, false, zoom);
    assert!(controller.borrow().state.video_paused);
    controller.borrow_mut().execute(&ui, Action::Activate);
    assert!(ui.get_shortcut_editor_open());
    snapshot(&ui, "video-shortcut-editor-restored");
    ui.invoke_shortcut_command("cancel".into());
    open_video(&ui, &controller, 1);
    ui.invoke_open_genius_requested();
    assert!(ui.get_genius_preview_open());
    assert!(controller.borrow().state.video_paused);
    assert_size(&ui, true, false, zoom);
    snapshot(&ui, "video-genius-restored");
    ui.invoke_genius_action("close".into());
    open_video(&ui, &controller, 1);
    controller
        .borrow_mut()
        .execute(&ui, Action::ConfigureDeepSeek);
    assert!(ui.get_deepseek_settings_open());
    assert_size(&ui, true, false, zoom);
}

#[test]
fn subtitle_menu_pointer_selection_persists_per_video() {
    let directory = tempfile::tempdir().unwrap();
    let (ui, controller) = setup(directory.path());
    let path = directory.path().join("local-generated.mp4");
    controller.borrow_mut().state.merge_videos(vec![Video {
        path: path.clone(),
        title: "Subtitle fixture".into(),
        duration: 4.0,
        width: 320,
        height: 180,
        display_aspect: 16.0 / 9.0,
        video_codec: "h264".into(),
        audio_codec: None,
    }]);
    open_video(&ui, &controller, 1);
    ui.invoke_subtitle_command("open".into());
    assert!(ui.get_subtitle_menu_open());
    // The second row is Off. Click the rendered row rather than invoking the
    // selection callback so the hit target and controller wiring are covered.
    let factor = ui.window().scale_factor();
    let position = slint::LogicalPosition::new(300.0 / factor, 194.0 / factor);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    assert!(!ui.get_subtitle_menu_open());
    assert_eq!(
        controller.borrow().state.video_subtitles.get(&path),
        SubtitlePreference::Off
    );
    ui.invoke_subtitle_command("open".into());
    let keys: [slint::SharedString; 2] = [
        slint::platform::Key::J.into(),
        slint::platform::Key::Return.into(),
    ];
    for key in keys {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key });
    }
    assert!(!ui.get_subtitle_menu_open());
    assert_eq!(
        controller.borrow().state.video_subtitles.get(&path),
        SubtitlePreference::Automatic
    );
    ui.invoke_subtitle_command("open".into());
    let key: slint::SharedString = slint::platform::Key::Escape.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key });
    assert!(!ui.get_subtitle_menu_open());
    assert_eq!(
        ui.get_screen(),
        9,
        "Back closes the subtitle menu before leaving video"
    );
    let sidecar = directory.path().join("chosen.ass");
    std::fs::write(&sidecar, "generated subtitle fixture").unwrap();
    controller
        .borrow_mut()
        .set_video_subtitle(SubtitlePreference::External(sidecar.clone()));
    let canonical_sidecar = std::fs::canonicalize(&sidecar).unwrap();
    assert_eq!(
        controller.borrow().state.video_subtitles.get(&path),
        SubtitlePreference::External(canonical_sidecar.clone())
    );
    controller
        .borrow_mut()
        .set_video_subtitle(SubtitlePreference::External(
            directory.path().join("missing.srt"),
        ));
    assert_eq!(
        controller.borrow().state.video_subtitles.get(&path),
        SubtitlePreference::External(canonical_sidecar)
    );
    let reopened = Controller::new(directory.path().to_path_buf());
    assert_eq!(
        reopened.state.video_subtitles.get(&path),
        SubtitlePreference::External(std::fs::canonicalize(&sidecar).unwrap())
    );
    ui.invoke_subtitle_command("remove".into());
    assert_eq!(
        controller.borrow().state.video_subtitles.get(&path),
        SubtitlePreference::Automatic
    );
    assert!(
        sidecar.is_file(),
        "removing an association must preserve the original subtitle file"
    );
    assert_eq!(
        nano_music_player::video::subtitles::SubtitleAssociations::load(
            &directory.path().join("video-subtitles.json"),
        )
        .unwrap()
        .get(&path),
        SubtitlePreference::Automatic
    );
}

#[test]
fn subtitle_menu_many_tracks_renders_in_both_landscape_modes() {
    let directory = tempfile::tempdir().unwrap();
    let (ui, controller) = setup(directory.path());
    controller.borrow_mut().state.merge_videos(vec![Video {
        path: directory.path().join("landscape-menu.mp4"),
        title: "Landscape menu fixture".into(),
        duration: 4.0,
        width: 320,
        height: 180,
        display_aspect: 16.0 / 9.0,
        video_codec: "h264".into(),
        audio_codec: None,
    }]);
    open_video(&ui, &controller, 1);
    let mut choices = vec![
        SubtitleChoice {
            id: -2,
            label: "Automatic".into(),
            selected: true,
        },
        SubtitleChoice {
            id: -1,
            label: "Off".into(),
            selected: false,
        },
    ];
    choices.extend((0..24).map(|id| SubtitleChoice {
        id,
        label: format!("Embedded subtitle track {id} · Japanese captions").into(),
        selected: false,
    }));
    let selected = choices.len() - 1;
    for (index, choice) in choices.iter_mut().enumerate() {
        choice.selected = index == selected;
    }
    ui.set_subtitle_menu_selected(selected as i32);
    ui.set_subtitle_choices(slint::ModelRc::new(slint::VecModel::from(choices)));
    ui.set_subtitle_menu_open(true);
    let picked = Rc::new(std::cell::Cell::new(None));
    let capture = picked.clone();
    ui.on_subtitle_select(move |id| capture.set(Some(id)));
    let verify_selected = || {
        let pixels = ui.window().take_snapshot().unwrap();
        let selected_pixels: Vec<_> = pixels
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, rgba)| rgba[..3] == [52, 120, 200])
            .map(|(index, _)| (index as u32 % pixels.width(), index as u32 / pixels.width()))
            .collect();
        assert!(
            selected_pixels.len() > 100,
            "selected subtitle row must actually be visible, not merely present in the model"
        );
        let x = (selected_pixels.iter().map(|(x, _)| x).min().unwrap()
            + selected_pixels.iter().map(|(x, _)| x).max().unwrap()) as f32
            / 2.0;
        let y = (selected_pixels.iter().map(|(_, y)| y).min().unwrap()
            + selected_pixels.iter().map(|(_, y)| y).max().unwrap()) as f32
            / 2.0;
        let factor = ui.window().scale_factor();
        let position = slint::LogicalPosition::new(x / factor, y / factor);
        for pressed in [true, false] {
            ui.window().dispatch_event(if pressed {
                WindowEvent::PointerPressed {
                    position,
                    button: PointerEventButton::Left,
                }
            } else {
                WindowEvent::PointerReleased {
                    position,
                    button: PointerEventButton::Left,
                }
            });
        }
        assert_eq!(
            picked.get(),
            Some(23),
            "the visible selected row must receive pointer activation"
        );
        picked.set(None);
    };
    verify_selected();
    controller.borrow_mut().execute(&ui, Action::ScreenMode);
    assert!(ui.get_subtitle_menu_open());
    verify_selected();
}

fn wait_for(
    ui: &AppWindow,
    controller: &RefCell<Controller>,
    condition: impl Fn(&Controller, &AppWindow) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut controller = controller.borrow_mut();
        controller.poll(ui);
        controller.poll_video_frame(ui);
        if condition(&controller, ui) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out: {}",
            controller.state.notice
        );
        drop(controller);
        std::thread::sleep(Duration::from_millis(16));
    }
}

#[test]
#[ignore = "Requires libmpv, ffmpeg and a default audio output; only uses generated media"]
fn actual_video_import_frames_rotation_and_music_handoff() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("generated.mp4");
    let ffmpeg = if Path::new("/opt/homebrew/bin/ffmpeg").is_file() {
        "/opt/homebrew/bin/ffmpeg"
    } else {
        "ffmpeg"
    };
    assert!(
        std::process::Command::new(ffmpeg)
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=24",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-t",
                "4",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-af",
                "volume=0.001"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let (ui, controller) = setup(directory.path());
    controller.borrow_mut().import_videos(vec![path]);
    wait_for(&ui, &controller, |c, _| {
        !c.state.video_importing && c.state.videos.len() == 1
    });
    assert!(directory.path().join("videos.json").is_file());
    {
        let mut c = controller.borrow_mut();
        c.state.navigation.open(Screen::Videos);
        c.execute(&ui, Action::FinishSearchEditing);
    }
    wait_for(&ui, &controller, |_, ui| {
        ui.get_rows()
            .row_data(1)
            .is_some_and(|row| row.artwork.size().width > 0)
    });
    let row = ui.get_rows().row_data(1).unwrap();
    assert!(row.show_artwork);
    assert_eq!(row.artwork.size().width, 96);
    assert_eq!(row.artwork.size().height, 54);
    snapshot(&ui, "video-native-list-thumbnail");
    open_video(&ui, &controller, 1);
    wait_for(&ui, &controller, |c, ui| {
        c.state.video_status == Status::Playing
            && c.state.video_position > 0.2
            && ui.get_video_frame().size().width > 0
    });
    assert_size(&ui, true, true, 1.25);
    snapshot(&ui, "video-native-integrated-landscape");
    controller.borrow_mut().execute(&ui, Action::Back);
    wait_for(&ui, &controller, |c, _| {
        c.state.video_status == Status::Paused
    });
    assert_size(&ui, true, false, 1.25);
    controller.borrow_mut().execute(&ui, Action::NowPlaying);
    ui.invoke_seek_changed(0.875);
    controller.borrow_mut().execute(&ui, Action::TogglePlay);
    wait_for(&ui, &controller, |c, _| {
        c.state.video_status == Status::Finished
    });
    let previous = controller.borrow().state.video_generation;
    controller.borrow_mut().execute(&ui, Action::TogglePlay);
    wait_for(&ui, &controller, |c, _| {
        c.state.video_status == Status::Playing && c.state.video_position > 0.1
    });
    assert!(controller.borrow().state.video_generation > previous);
    let music = directory.path().join("quiet.wav");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quiet.wav"),
        &music,
    )
    .unwrap();
    {
        let mut c = controller.borrow_mut();
        c.state.merge_library(vec![Track {
            path: music,
            title: "Generated music".into(),
            artist: "Test".into(),
            album: "Test".into(),
            genre: "Test".into(),
            number: 1,
            duration: 1.0,
            artwork: vec![],
            lyrics: Lyrics::default(),
        }]);
        c.state.navigation.open(Screen::Songs(Filter::All));
        c.state.navigation.select(0);
        c.execute(&ui, Action::Activate);
    }
    wait_for(&ui, &controller, |c, _| {
        !c.state.video_active && c.state.status == PlaybackStatus::Playing
    });
    assert_size(&ui, true, false, 1.25);
    assert_eq!(controller.borrow().state.queue.current(), Some(0));
}
