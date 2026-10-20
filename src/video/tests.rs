use super::{
    Status,
    engine::{Command, Engine, Event},
    library,
};
use std::{
    path::Path,
    process::Command as Process,
    thread,
    time::{Duration, Instant},
};

fn fixture(path: &Path) {
    let ffmpeg = std::env::var_os("NANO_FFMPEG").unwrap_or_else(|| {
        if Path::new("/opt/homebrew/bin/ffmpeg").is_file() {
            "/opt/homebrew/bin/ffmpeg".into()
        } else {
            "ffmpeg".into()
        }
    });
    assert!(
        Process::new(ffmpeg)
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
                "volume=0.02",
            ])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

fn wait(
    engine: &Engine,
    condition: impl Fn(&super::engine::Progress) -> bool,
) -> super::engine::Progress {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(8) {
        if let Some(progress) = engine.take_progress()
            && condition(&progress)
        {
            return progress;
        }
        thread::sleep(Duration::from_millis(15));
    }
    panic!(
        "Timed out waiting for video progress; event: {:?}",
        engine.events.try_recv()
    );
}

fn wait_finished(engine: &Engine, generation: u64) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        while let Ok(event) = engine.events.try_recv() {
            if matches!(event, Event::Finished(found) if found == generation) {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "missing Finished event for generation {generation}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_stopped(engine: &Engine, handoff: u64) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        while let Ok(event) = engine.events.try_recv() {
            if matches!(event, Event::Stopped { handoff: found } if found == handoff) {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "missing Stopped event for handoff {handoff}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "Requires libmpv and ffmpeg; generates and decodes an original fixture"]
fn native_import_transport_generations_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original.mp4");
    let db = dir.path().join("videos.json");
    fixture(&path);
    let imported = library::import(&[dir.path().into()], &db).unwrap();
    assert_eq!(imported.imported, 1);
    assert!(imported.errors.is_empty());
    let video = library::load(&db).unwrap().remove(0);
    assert_eq!(video.video_codec, "h264");
    assert_eq!(video.audio_codec.as_deref(), Some("aac"));
    assert_eq!(video.render_size(), (640, 360));
    assert_eq!(
        library::import(std::slice::from_ref(&path), &db)
            .unwrap()
            .videos
            .len(),
        1
    );
    std::fs::write(dir.path().join("bad.mp4"), "not video").unwrap();
    let imported = library::import(&[dir.path().into()], &db).unwrap();
    assert_eq!(imported.videos.len(), 1);
    assert_eq!(imported.errors.len(), 1);

    let engine = Engine::new(true);
    engine
        .send(Command::Load {
            generation: 1,
            video: video.clone(),
            start: 0.0,
            volume: 0.2,
            paused: false,
        })
        .unwrap();
    wait(&engine, |p| p.status == Status::Playing && p.position > 0.2);
    let frame = engine.take_frame().unwrap();
    assert_eq!((frame.width, frame.height, frame.generation), (640, 360, 1));
    assert_eq!(frame.rgba.len(), 640 * 360 * 4);
    assert!(
        frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    assert!(frame.rgba.iter().any(|v| *v > 20 && *v < 200));
    engine
        .send(Command::Pause {
            generation: 1,
            paused: true,
        })
        .unwrap();
    let paused = wait(&engine, |p| p.status == Status::Paused);
    thread::sleep(Duration::from_millis(160));
    assert!((engine.take_progress().unwrap().position - paused.position).abs() < 0.05);
    engine
        .send(Command::Seek {
            generation: 1,
            seconds: 2.0,
        })
        .unwrap();
    wait(&engine, |p| {
        p.status == Status::Paused && (p.position - 2.0).abs() < 0.15
    });
    engine
        .send(Command::Volume {
            generation: 1,
            volume: 0.0,
        })
        .unwrap();
    engine
        .send(Command::Load {
            generation: 2,
            video: video.clone(),
            start: 0.0,
            volume: 0.0,
            paused: true,
        })
        .unwrap();
    wait(&engine, |p| p.generation == 2 && p.status == Status::Paused);
    // Old commands for the same item must not control a new session.
    engine
        .send(Command::Pause {
            generation: 1,
            paused: false,
        })
        .unwrap();
    engine
        .send(Command::Seek {
            generation: 1,
            seconds: 3.0,
        })
        .unwrap();
    engine
        .send(Command::Subtitle {
            generation: 1,
            preference: super::subtitles::SubtitlePreference::External(
                dir.path().join("stale-session-must-ignore.ass"),
            ),
        })
        .unwrap();
    thread::sleep(Duration::from_millis(160));
    let progress = engine.take_progress().unwrap();
    assert_eq!(progress.status, Status::Paused);
    assert!(progress.position < 0.1);
    assert_eq!(engine.take_frame().unwrap().generation, 2);
    engine
        .send(Command::Seek {
            generation: 2,
            seconds: 3.5,
        })
        .unwrap();
    engine
        .send(Command::Pause {
            generation: 2,
            paused: false,
        })
        .unwrap();
    wait(&engine, |p| p.status == Status::Finished);
    wait_finished(&engine, 2);
    // EOF unloads the native file. Replay and seeking after EOF open fresh sessions.
    engine
        .send(Command::Load {
            generation: 3,
            video: video.clone(),
            start: 0.0,
            volume: 0.0,
            paused: false,
        })
        .unwrap();
    wait(&engine, |p| {
        p.generation == 3 && p.status == Status::Playing && p.position > 0.1 && p.position < 1.0
    });
    engine
        .send(Command::Seek {
            generation: 3,
            seconds: 3.5,
        })
        .unwrap();
    wait(&engine, |p| {
        p.generation == 3 && p.status == Status::Finished
    });
    wait_finished(&engine, 3);
    engine
        .send(Command::Load {
            generation: 4,
            video: video.clone(),
            start: 1.5,
            volume: 0.0,
            paused: true,
        })
        .unwrap();
    wait(&engine, |p| {
        p.generation == 4 && p.status == Status::Paused && (p.position - 1.5).abs() < 0.15
    });
    assert_eq!(engine.take_frame().unwrap().generation, 4);
    engine.send(Command::Stop { handoff: 42 }).unwrap();
    wait_stopped(&engine, 42);
    assert!(engine.take_frame().is_none());
    assert!(engine.take_progress().is_none());
    let mut missing = video;
    missing.path = dir.path().join("missing.mp4");
    engine
        .send(Command::Load {
            generation: 5,
            video: missing,
            start: 0.0,
            volume: 0.0,
            paused: false,
        })
        .unwrap();
    let start = Instant::now();
    loop {
        if let Ok(Event::MissingFile { generation, .. }) = engine.events.try_recv() {
            assert_eq!(generation, 5);
            let progress = engine.take_progress().unwrap();
            assert_eq!((progress.generation, progress.status), (5, Status::Error));
            assert!(engine.take_frame().is_none());
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "Requires libmpv/libass and ffmpeg; verifies styled subtitles in copied render frames"]
fn ass_sidecar_is_composited_and_seek_synchronized() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("styled.mp4");
    let ass = dir.path().join("styled.ass");
    assert!(
        Process::new(std::env::var_os("NANO_FFMPEG").unwrap_or_else(|| "ffmpeg".into()))
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=320x180:r=24",
                "-t",
                "4",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(&ass, "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 180\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,42,&H0000FF00,&H000000FF,&H00000000,&H80000000,1,0,0,0,100,100,0,0,1,2,0,2,10,10,20,1\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.50,0:00:02.50,Default,,0,0,0,,STYLED TEST\n").unwrap();
    let video = library::probe(&path).unwrap();
    let engine = Engine::new(true);
    engine
        .send(Command::Load {
            generation: 91,
            video,
            start: 0.0,
            volume: 0.0,
            paused: true,
        })
        .unwrap();
    wait(&engine, |p| {
        p.generation == 91 && p.status == Status::Paused
    });
    engine
        .send(Command::Subtitle {
            generation: 91,
            preference: super::subtitles::SubtitlePreference::External(
                dir.path().join("missing.ass"),
            ),
        })
        .unwrap();
    let subtitle_error_deadline = Instant::now() + Duration::from_secs(2);
    let mut subtitle_error_seen = false;
    while Instant::now() < subtitle_error_deadline {
        while let Ok(event) = engine.events.try_recv() {
            subtitle_error_seen |= matches!(event, Event::SubtitleError { generation: 91, .. });
        }
        if subtitle_error_seen {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(subtitle_error_seen);
    assert_eq!(engine.take_progress().unwrap().status, Status::Paused);
    engine
        .send(Command::Subtitle {
            generation: 91,
            preference: super::subtitles::SubtitlePreference::External(ass),
        })
        .unwrap();
    engine
        .send(Command::Pause {
            generation: 91,
            paused: false,
        })
        .unwrap();
    wait(&engine, |p| p.generation == 91 && p.position > 1.0);
    engine
        .send(Command::Pause {
            generation: 91,
            paused: true,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(250));
    let frame = engine.take_frame().unwrap();
    let green = frame
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[1] > 170 && p[0] < 100 && p[2] < 100)
        .count();
    assert!(
        green > 100,
        "ASS style was not composited into RGB frame; green pixels={green}"
    );
    engine
        .send(Command::Seek {
            generation: 91,
            seconds: 3.0,
        })
        .unwrap();
    wait(&engine, |p| {
        p.generation == 91 && p.status == Status::Paused && p.position > 2.8
    });
    thread::sleep(Duration::from_millis(250));
    let frame = engine.take_frame().unwrap();
    let green_after_end = frame
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[1] > 170 && p[0] < 100 && p[2] < 100)
        .count();
    assert!(
        green_after_end < 100,
        "subtitle remained visible beyond its ASS end time: {green_after_end}"
    );
    let replacement = dir.path().join("manual-replacement.ass");
    let replacement_script = std::fs::read_to_string(dir.path().join("styled.ass"))
        .unwrap()
        .replace("&H0000FF00", "&H00FF0000")
        .replace("STYLED TEST", "MANUAL REPLACEMENT");
    std::fs::write(&replacement, &replacement_script).unwrap();
    let replacement_identity = std::fs::canonicalize(&replacement).unwrap();
    engine
        .send(Command::Seek {
            generation: 91,
            seconds: 1.0,
        })
        .unwrap();
    wait(&engine, |p| {
        p.generation == 91 && p.status == Status::Paused && (p.position - 1.0).abs() < 0.15
    });
    engine
        .send(Command::Subtitle {
            generation: 91,
            preference: super::subtitles::SubtitlePreference::External(replacement.clone()),
        })
        .unwrap();
    engine
        .send(Command::Pause {
            generation: 91,
            paused: false,
        })
        .unwrap();
    wait(&engine, |p| p.generation == 91 && p.position > 1.2);
    engine
        .send(Command::Pause {
            generation: 91,
            paused: true,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(250));
    let replacement_frame = engine.take_frame().unwrap();
    let blue = replacement_frame
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[2] > 170 && p[0] < 100 && p[1] < 100)
        .count();
    assert!(
        blue > 100,
        "manual replacement did not override the discovered sidecar: {blue}"
    );
    engine
        .send(Command::Subtitle {
            generation: 91,
            preference: super::subtitles::SubtitlePreference::Off,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(250));
    let off_frame = engine.take_frame().unwrap();
    let colored = off_frame
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| (p[1] > 170 || p[2] > 170) && p[0] < 100)
        .count();
    assert!(
        colored < 100,
        "subtitle pixels remained after selecting Off: {colored}"
    );
    // Selecting a loaded file repeatedly must keep one track. Successful async
    // replies must not become error notices, and edits at the same path reload.
    for _ in 0..3 {
        engine
            .send(Command::Subtitle {
                generation: 91,
                preference: super::subtitles::SubtitlePreference::External(replacement.clone()),
            })
            .unwrap();
    }
    thread::sleep(Duration::from_millis(250));
    let updated_script = replacement_script
        .replace("&H00FF0000", "&H000000FF")
        .replace("MANUAL REPLACEMENT", "UPDATED IN PLACE");
    std::fs::write(&replacement, updated_script).unwrap();
    engine
        .send(Command::Subtitle {
            generation: 91,
            preference: super::subtitles::SubtitlePreference::External(replacement.clone()),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut last_tracks = None;
    let mut red_seen = false;
    while Instant::now() < deadline {
        while let Ok(event) = engine.events.try_recv() {
            match event {
                Event::SubtitleError { message, .. } => {
                    panic!("unexpected subtitle error: {message}")
                }
                Event::SubtitleTracks { tracks, .. } => last_tracks = Some(tracks),
                _ => {}
            }
        }
        if let Some(frame) = engine.take_frame() {
            red_seen |= frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] > 170 && pixel[1] < 100 && pixel[2] < 100)
                .count()
                > 100;
        }
        if red_seen
            && last_tracks.as_ref().is_some_and(|tracks| {
                tracks
                    .iter()
                    .filter(|track| track.external_path.as_ref() == Some(&replacement_identity))
                    .count()
                    == 1
            })
        {
            break;
        }
        thread::sleep(Duration::from_millis(15));
    }
    assert!(red_seen, "same-path subtitle edit was not reloaded");
    assert_eq!(
        last_tracks
            .unwrap()
            .iter()
            .filter(|track| track.external_path.as_ref() == Some(&replacement_identity))
            .count(),
        1
    );
}

#[test]
#[ignore = "Requires libmpv/libass and ffmpeg; validates embedded subtitle track discovery and selection"]
fn embedded_ass_track_is_enumerated_and_selectable() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.mp4");
    let ass = dir.path().join("embedded.ass");
    let path = dir.path().join("embedded.mkv");
    fixture(&base);
    std::fs::write(&ass, "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 180\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,42,&H0000FF00,&H000000FF,&H00000000,&H80000000,1,0,0,0,100,100,0,0,1,2,0,2,10,10,20,1\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.50,0:00:03.50,Default,,0,0,0,,EMBEDDED STYLE\n").unwrap();
    let ffmpeg = std::env::var_os("NANO_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
    assert!(
        Process::new(ffmpeg)
            .args(["-v", "error", "-y", "-i"])
            .arg(&base)
            .arg("-i")
            .arg(&ass)
            .args(["-map", "0", "-map", "1:s:0", "-c", "copy"])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let video = library::probe(&path).unwrap();
    let engine = Engine::new(true);
    engine
        .send(Command::Load {
            generation: 92,
            video,
            start: 1.0,
            volume: 0.0,
            paused: true,
        })
        .unwrap();
    wait(&engine, |p| {
        p.generation == 92 && p.status == Status::Paused
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let embedded = loop {
        if let Ok(Event::SubtitleTracks {
            generation: 92,
            tracks,
        }) = engine.events.recv_timeout(Duration::from_millis(50))
            && let Some(track) = tracks.into_iter().find(|track| !track.external)
        {
            break track;
        }
        assert!(
            Instant::now() < deadline,
            "embedded subtitle track list did not arrive"
        );
    };
    engine
        .send(Command::Subtitle {
            generation: 92,
            preference: super::subtitles::SubtitlePreference::Embedded(embedded.id),
        })
        .unwrap();
    engine
        .send(Command::Pause {
            generation: 92,
            paused: false,
        })
        .unwrap();
    wait(&engine, |p| p.generation == 92 && p.position > 1.3);
    engine
        .send(Command::Pause {
            generation: 92,
            paused: true,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(200));
    let frame = engine.take_frame().unwrap();
    let green = frame
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[1] > 170 && p[0] < 100 && p[2] < 100)
        .count();
    assert!(green > 100, "embedded ASS was not composited: {green}");
}

#[test]
#[ignore = "Requires libmpv, ffmpeg and a real default audio output"]
fn native_default_audio_output_clock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio-clock.mp4");
    fixture(&path);
    let video = library::probe(&path).unwrap();
    let engine = Engine::default();
    engine
        .send(Command::Load {
            generation: 1,
            video,
            start: 0.0,
            volume: 0.02,
            paused: false,
        })
        .unwrap();
    let start = Instant::now();
    let progress = wait(&engine, |p| {
        p.status == Status::Playing && p.position > 0.5 && p.audio_output.is_some()
    });
    assert_ne!(progress.audio_output.as_deref(), Some("null"));
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(engine.take_frame().is_some());
}

#[test]
#[ignore = "Requires libmpv and FFmpeg encoders; checks the documented container/codec matrix"]
fn native_container_codec_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    fixture(&source);
    let ffmpeg = std::env::var_os("NANO_FFMPEG").unwrap_or_else(|| {
        if Path::new("/opt/homebrew/bin/ffmpeg").is_file() {
            "/opt/homebrew/bin/ffmpeg".into()
        } else {
            "ffmpeg".into()
        }
    });
    let engine = Engine::new(true);
    for (n, (name, options, video_codec, audio_codec)) in [
        (
            "movie.m4v",
            vec!["-c", "copy", "-f", "mp4"],
            "h264",
            Some("aac"),
        ),
        ("movie.mkv", vec!["-c", "copy"], "h264", Some("aac")),
        (
            "movie.mov",
            vec!["-c:v", "copy", "-c:a", "pcm_s16le"],
            "h264",
            Some("pcm_s16le"),
        ),
        (
            "movie.webm",
            vec![
                "-c:v",
                "libvpx-vp9",
                "-deadline",
                "realtime",
                "-cpu-used",
                "8",
                "-c:a",
                "libopus",
            ],
            "vp9",
            Some("opus"),
        ),
        (
            "movie.avi",
            vec!["-c:v", "mpeg4", "-c:a", "libmp3lame"],
            "mpeg4",
            Some("mp3"),
        ),
        ("silent.mp4", vec!["-c:v", "copy", "-an"], "h264", None),
    ]
    .into_iter()
    .enumerate()
    {
        let path = dir.path().join(name);
        assert!(
            Process::new(&ffmpeg)
                .args(["-v", "error", "-y", "-i"])
                .arg(&source)
                .args(["-t", "1"])
                .args(options)
                .arg(&path)
                .status()
                .unwrap()
                .success(),
            "{name}"
        );
        let video = library::probe(&path).unwrap();
        assert_eq!(video.video_codec, video_codec, "{name}");
        assert_eq!(video.audio_codec.as_deref(), audio_codec, "{name}");
        let generation = n as u64 + 1;
        engine
            .send(Command::Load {
                generation,
                video,
                start: 0.0,
                volume: 0.0,
                paused: false,
            })
            .unwrap();
        wait(&engine, |p| {
            p.generation == generation && p.status == Status::Playing && p.position > 0.05
        });
        assert_eq!(
            engine.take_frame().unwrap().generation,
            generation,
            "{name}"
        );
    }
}
