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
    assert!(matches!(engine.events.try_recv(), Ok(Event::Finished(2))));
    engine.send(Command::Stop { handoff: 42 }).unwrap();
    assert!(matches!(
        engine.events.recv_timeout(Duration::from_secs(2)),
        Ok(Event::Stopped { handoff: 42 })
    ));
    assert!(engine.take_frame().is_none());
    assert!(engine.take_progress().is_none());
    let mut missing = video;
    missing.path = dir.path().join("missing.mp4");
    engine
        .send(Command::Load {
            generation: 3,
            video: missing,
            volume: 0.0,
            paused: false,
        })
        .unwrap();
    let start = Instant::now();
    loop {
        if let Ok(Event::Error { generation, .. }) = engine.events.try_recv() {
            assert_eq!(generation, 3);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(20));
    }
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
