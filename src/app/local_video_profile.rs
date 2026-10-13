//! Opt-in integration profile for user-supplied local video and ASS/SSA files.
//! All playback/preferences use an isolated configuration. Normal-library import
//! happens only after verification and only with an explicit import directory.
use super::*;
use crate::video::{Status, engine::Engine as VideoEngine, library, subtitles::*};
use slint::ComponentHandle;
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn pump(controller: &mut Controller, ui: &AppWindow, duration: Duration) {
    let started = Instant::now();
    while started.elapsed() < duration {
        controller.poll(ui);
        controller.poll_video_frame(ui);
        assert_ne!(
            controller.state.video_status,
            Status::Error,
            "{}",
            controller.state.notice
        );
        assert!(
            !controller.state.notice.starts_with("Subtitle error:"),
            "{}",
            controller.state.notice
        );
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(10));
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for(controller: &mut Controller, ui: &AppWindow, predicate: impl Fn(&Controller) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !predicate(controller) {
        assert!(
            Instant::now() < deadline,
            "video profile timed out: {}",
            controller.state.notice
        );
        pump(controller, ui, Duration::from_millis(20));
    }
}

fn save_pixels(pixels: &slint::SharedPixelBuffer<slint::Rgba8Pixel>, path: &Path) {
    image::save_buffer(
        path,
        pixels.as_bytes(),
        pixels.width(),
        pixels.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

fn differences(
    a: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    b: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
) -> usize {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    a.as_bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_bytes().as_chunks::<4>().0)
        .filter(|(a, b)| (0..3).any(|channel| a[channel].abs_diff(b[channel]) > 16))
        .count()
}

#[test]
#[ignore = "Opt-in local video/ASS profile; NANO_VIDEO_PROFILE_IMPORT_DIR explicitly enables normal-library import"]
fn local_video_ass_pipeline_and_optional_library_import() {
    let required = |name| fs::canonicalize(std::env::var_os(name).expect(name)).expect(name);
    let source = required("NANO_VIDEO_PROFILE_SOURCE");
    let subtitle = required("NANO_VIDEO_PROFILE_SUBTITLE");
    assert!(
        subtitle
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ass")
                || extension.eq_ignore_ascii_case("ssa"))
    );
    let output = PathBuf::from(
        std::env::var_os("NANO_VIDEO_PROFILE_OUTPUT").expect("NANO_VIDEO_PROFILE_OUTPUT"),
    );
    fs::create_dir_all(&output).unwrap();
    let samples: Vec<(f64, bool)> = std::env::var("NANO_VIDEO_PROFILE_SAMPLES")
        .expect("seconds:on,seconds:off")
        .split(',')
        .map(|sample| {
            let (seconds, expected) = sample.split_once(':').expect("seconds:on or seconds:off");
            (
                seconds.parse::<f64>().unwrap(),
                match expected {
                    "on" => true,
                    "off" => false,
                    _ => panic!("on or off required"),
                },
            )
        })
        .collect();
    assert!(!samples.is_empty());
    assert!(
        samples.iter().any(|(_, expected)| *expected)
            && samples.iter().any(|(_, expected)| !*expected),
        "provide both a caption event and a caption-free gap"
    );
    let source_before = fs::metadata(&source).unwrap();
    let subtitle_before = fs::read(&subtitle).unwrap();
    let directory = tempfile::tempdir().unwrap();
    Settings {
        monitor_deleted_files: false,
        ..Default::default()
    }
    .save(&directory.path().join("settings.json"))
    .unwrap();

    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        },
    )))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let mut controller = Controller::new(directory.path().to_owned());
    controller.video_engine = VideoEngine::new(true);
    controller.initialize(&ui);
    controller.import_videos(vec![source.clone()]);
    wait_for(&mut controller, &ui, |c| {
        !c.state.video_importing && c.state.videos.iter().any(|v| v.path == source)
    });
    let video_id = controller
        .state
        .videos
        .iter()
        .position(|v| v.path == source)
        .unwrap();
    let video = controller.state.videos[video_id].clone();
    assert_eq!(video.render_size(), (640, 360));
    assert!(
        samples.iter().all(|(seconds, _)| seconds.is_finite()
            && *seconds >= 0.0
            && *seconds < video.duration)
    );
    controller.state.video_current = Some(video_id);
    controller.set_video_subtitle(SubtitlePreference::External(subtitle.clone()));
    controller.state.volume = 0.0;
    controller.state.navigation.open(Screen::Videos);
    let row = controller
        .state
        .rows()
        .iter()
        .position(|row| matches!(row.target, Target::PlayVideo(id) if id == video_id))
        .unwrap();
    controller.state.navigation.select(row);
    controller.execute(&ui, Action::Activate);
    wait_for(&mut controller, &ui, |c| {
        c.state.video_status == Status::Playing
            && c.subtitle_tracks
                .iter()
                .any(|track| track.external_path.as_ref() == Some(&subtitle))
    });
    controller.execute(&ui, Action::TogglePlay);
    wait_for(&mut controller, &ui, |c| {
        c.state.video_status == Status::Paused
    });
    ui.show().unwrap();
    let mut reports = Vec::new();
    for (index, (seconds, expected)) in samples.iter().copied().enumerate() {
        let generation = controller.state.video_generation;
        controller.send_video(crate::video::engine::Command::Seek {
            generation,
            seconds,
        });
        wait_for(&mut controller, &ui, |c| {
            c.state.video_status == Status::Paused
                && (c.state.video_position - seconds).abs() < 0.15
        });
        controller.set_video_subtitle(SubtitlePreference::Off);
        pump(&mut controller, &ui, Duration::from_millis(400));
        let off = ui.get_video_frame().to_rgba8().expect("decoded RGBA frame");
        assert_eq!((off.width(), off.height()), video.render_size());
        controller.set_video_subtitle(SubtitlePreference::External(subtitle.clone()));
        pump(&mut controller, &ui, Duration::from_millis(500));
        let on = ui.get_video_frame().to_rgba8().unwrap();
        let changed = differences(&off, &on);
        if expected {
            assert!(changed > 30, "no subtitle pixels at {seconds}s: {changed}");
        } else {
            assert!(
                changed < 30,
                "subtitle did not expire at {seconds}s: {changed}"
            );
        }
        save_pixels(&off, &output.join(format!("sample-{index}-off.png")));
        save_pixels(&on, &output.join(format!("sample-{index}-on.png")));
        controller.state.video_overlay = false;
        controller.initialize(&ui);
        // initialize updates layout; retain the actual decoded image explicitly.
        ui.set_video_frame(slint::Image::from_rgba8(on));
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(250));
        save_pixels(
            &ui.window().take_snapshot().unwrap(),
            &output.join(format!("sample-{index}-device.png")),
        );
        assert!((controller.state.video_position - seconds).abs() < 0.15);
        controller.set_video_subtitle(SubtitlePreference::Off);
        pump(&mut controller, &ui, Duration::from_millis(400));
        let off_again = ui.get_video_frame().to_rgba8().unwrap();
        assert!(
            differences(&off, &off_again) < 30,
            "Off did not restore the paused video frame"
        );
        reports.push(serde_json::json!({"seconds":seconds,"captions_expected":expected,"changed_pixels":changed}));
    }
    controller.set_video_subtitle(SubtitlePreference::External(subtitle.clone()));
    pump(&mut controller, &ui, Duration::from_millis(400));
    let choices = controller.subtitle_choices();
    assert_eq!(
        controller
            .subtitle_tracks
            .iter()
            .filter(|track| track.external_path.as_ref() == Some(&subtitle))
            .count(),
        1,
        "repeated selection must not duplicate the ASS track"
    );
    assert!(
        choices
            .iter()
            .any(|choice| choice.selected && choice.id >= 0)
    );
    let controller = Rc::new(RefCell::new(controller));
    connect(&ui, &controller);
    ui.invoke_subtitle_command("open".into());
    assert!(ui.get_subtitle_menu_open());
    let menu = ui.window().take_snapshot().unwrap();
    assert!(
        menu.as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|rgba| rgba[..3] == [52, 120, 200])
            .count()
            > 100,
        "the persisted subtitle selection must be visible in the real menu"
    );
    save_pixels(&menu, &output.join("subtitle-menu.png"));
    let tracks = controller.borrow().subtitle_tracks.iter().map(|track| serde_json::json!({"id":track.id,"language":track.language,"external":track.external})).collect::<Vec<_>>();
    ui.invoke_subtitle_command("close".into());
    drop(ui);
    drop(controller);
    let replay_ui = AppWindow::new().unwrap();
    let mut restarted = Controller::new(directory.path().to_owned());
    restarted.video_engine = VideoEngine::new(true);
    assert_eq!(
        restarted.state.video_subtitles.get(&source),
        SubtitlePreference::External(subtitle.clone())
    );
    restarted.initialize(&replay_ui);
    wait_for(&mut restarted, &replay_ui, |c| {
        c.state.videos.iter().any(|video| video.path == source)
    });
    restarted.state.volume = 0.0;
    restarted.state.navigation.open(Screen::Videos);
    let row = restarted.state.rows().iter().position(|row| matches!(row.target, Target::PlayVideo(id) if restarted.state.videos[id].path == source)).unwrap();
    restarted.state.navigation.select(row);
    restarted.execute(&replay_ui, Action::Activate);
    wait_for(&mut restarted, &replay_ui, |c| {
        c.state.video_status == Status::Playing
            && c.subtitle_tracks
                .iter()
                .any(|track| track.external_path.as_ref() == Some(&subtitle))
    });
    restarted.execute(&replay_ui, Action::TogglePlay);
    wait_for(&mut restarted, &replay_ui, |c| {
        c.state.video_status == Status::Paused
    });
    if let Some((seconds, _)) = samples.iter().find(|(_, expected)| *expected) {
        let generation = restarted.state.video_generation;
        restarted.send_video(crate::video::engine::Command::Seek {
            generation,
            seconds: *seconds,
        });
        wait_for(&mut restarted, &replay_ui, |c| {
            (c.state.video_position - seconds).abs() < 0.15
        });
        pump(&mut restarted, &replay_ui, Duration::from_millis(400));
        let restored = replay_ui.get_video_frame().to_rgba8().unwrap();
        restarted.set_video_subtitle(SubtitlePreference::Off);
        pump(&mut restarted, &replay_ui, Duration::from_millis(400));
        assert!(
            differences(&restored, &replay_ui.get_video_frame().to_rgba8().unwrap()) > 30,
            "persisted ASS must render after reopening the video"
        );
    }
    drop(restarted);
    assert_eq!(fs::read(&subtitle).unwrap(), subtitle_before);
    let source_after = fs::metadata(&source).unwrap();
    assert_eq!(source_after.len(), source_before.len());
    assert_eq!(
        source_after.modified().unwrap(),
        source_before.modified().unwrap()
    );

    let mut imported = false;
    if let Some(import_directory) = std::env::var_os("NANO_VIDEO_PROFILE_IMPORT_DIR") {
        let import_directory = PathBuf::from(import_directory);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let backup = import_directory
            .join("qa-backups")
            .join(format!("video-ass-{stamp}"));
        fs::create_dir_all(&backup).unwrap();
        for name in ["videos.json", "video-subtitles.json"] {
            let path = import_directory.join(name);
            if path.exists() {
                fs::copy(&path, backup.join(name)).unwrap();
            }
        }
        let database = import_directory.join("videos.json");
        let before = library::load(&database).unwrap();
        let result = library::import(std::slice::from_ref(&source), &database).unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(
            before.iter().all(|video| result.videos.contains(video)),
            "existing videos must be retained"
        );
        assert_eq!(
            result
                .videos
                .iter()
                .filter(|video| video.path == source)
                .count(),
            1
        );
        let associations_path = import_directory.join("video-subtitles.json");
        let mut associations = SubtitleAssociations::load(&associations_path).unwrap();
        associations.set(&source, SubtitlePreference::External(subtitle.clone()));
        associations.save(&associations_path).unwrap();
        assert_eq!(
            SubtitleAssociations::load(&associations_path)
                .unwrap()
                .get(&source),
            SubtitlePreference::External(subtitle.clone())
        );
        imported = true;
    }
    let report = serde_json::json!({"title":video.title,"duration":video.duration,"source_size":[video.width,video.height],"render_size":video.render_size(),"video_codec":video.video_codec,"audio_codec":video.audio_codec,"tracks":tracks,"samples":reports,"association_persisted":true,"reopened_ass_rendered":true,"source_unchanged":true,"normal_library_imported":imported,"native_window_checked":false});
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
