use crate::video::library::Video;
use std::{
    collections::{HashSet, VecDeque, hash_map::RandomState},
    hash::BuildHasher,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

const SIZE: u32 = 96;
const CACHE_ENTRIES: usize = 64;
const OUTPUT_LIMIT: u64 = 65_536;
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(5);

struct PendingThumbnail {
    generation: u64,
    path: PathBuf,
    receiver: Receiver<Option<image::RgbaImage>>,
    cancelled: Arc<AtomicBool>,
}

/// At most one extraction runs at a time. UI images remain on the UI thread.
#[derive(Default)]
pub(super) struct VideoThumbnails {
    generation: u64,
    pending: Option<PendingThumbnail>,
    cache: VecDeque<(PathBuf, slint::Image)>,
    failed: HashSet<PathBuf>,
    // Random keys are created once, so a path keeps its sampled position even
    // after image eviction or library invalidation. Rendering never rerolls it.
    selection: RandomState,
}

impl VideoThumbnails {
    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cache.clear();
        self.failed.clear();
        if let Some(pending) = &self.pending {
            pending.cancelled.store(true, Ordering::Relaxed);
        }
        // Keep the old receiver until its worker exits; clearing cannot create
        // overlapping decoders. Its generation prevents stale publication.
    }
    pub fn cached(&self, path: &Path) -> Option<slint::Image> {
        self.cache
            .iter()
            .find(|(key, _)| key == path)
            .map(|(_, image)| image.clone())
    }
    pub fn request(&mut self, video: &Video) -> bool {
        if self.pending.is_some() {
            return false;
        }
        if self.failed.contains(&video.path) || self.cached(&video.path).is_some() {
            return true;
        }
        let path = video.path.clone();
        let position = self.position(video);
        let aspect = video.display_aspect;
        let (sender, receiver) = mpsc::channel();
        let worker_path = path.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        thread::spawn(move || {
            let _ = sender.send(extract(&worker_path, position, aspect, &worker_cancelled));
        });
        self.pending = Some(PendingThumbnail {
            generation: self.generation,
            path,
            receiver,
            cancelled,
        });
        false
    }
    fn position(&self, video: &Video) -> f64 {
        // Use 53 random bits, exactly representable as a unit-interval f64.
        let fraction = (self.selection.hash_one(&video.path) >> 11) as f64 / (1_u64 << 53) as f64;
        frame_position(video.duration, fraction)
    }
    pub fn poll(&mut self) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        let pixels = match pending.receiver.try_recv() {
            Ok(pixels) => pixels,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        let PendingThumbnail {
            generation, path, ..
        } = self.pending.take().unwrap();
        if generation != self.generation {
            return false;
        }
        if let Some(pixels) = pixels {
            let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                pixels.as_raw(),
                pixels.width(),
                pixels.height(),
            );
            self.cache
                .push_back((path, slint::Image::from_rgba8(buffer)));
            if self.cache.len() > CACHE_ENTRIES {
                self.cache.pop_front();
            }
        } else {
            self.failed.insert(path);
        }
        true
    }
}

impl Drop for VideoThumbnails {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancelled.store(true, Ordering::Relaxed);
        }
    }
}

fn frame_position(duration: f64, fraction: f64) -> f64 {
    if duration.is_finite() && duration > 0.0 {
        // Avoid opening/closing titles while retaining variety across videos.
        duration * (0.15 + 0.70 * fraction)
    } else {
        // Unknown-duration media gets a conservative, bounded one-second seek.
        1.0
    }
}

fn thumbnail_size(aspect: f64) -> Option<(u32, u32)> {
    if !aspect.is_finite() || aspect <= 0.0 {
        return None;
    }
    Some(if aspect >= 1.0 {
        (
            SIZE,
            (f64::from(SIZE) / aspect)
                .round()
                .clamp(1.0, f64::from(SIZE)) as u32,
        )
    } else {
        (
            (f64::from(SIZE) * aspect)
                .round()
                .clamp(1.0, f64::from(SIZE)) as u32,
            SIZE,
        )
    })
}

fn ffmpeg() -> PathBuf {
    if let Some(path) = std::env::var_os("NANO_FFMPEG") {
        return path.into();
    }
    for path in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"] {
        if Path::new(path).is_file() {
            return path.into();
        }
    }
    "ffmpeg".into()
}

fn extract(
    path: &Path,
    position: f64,
    aspect: f64,
    cancelled: &AtomicBool,
) -> Option<image::RgbaImage> {
    extract_using(path, position, aspect, cancelled, &ffmpeg())
}

fn extract_using(
    path: &Path,
    position: f64,
    aspect: f64,
    cancelled: &AtomicBool,
    executable: &Path,
) -> Option<image::RgbaImage> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let path = std::fs::canonicalize(path).ok()?;
    let (width, height) = thumbnail_size(aspect)?;
    let mut child = Command::new(executable)
        .args([
            "-nostdin",
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-threads",
            "1",
            "-ss",
        ])
        .arg(position.to_string())
        .arg("-i")
        .arg(path)
        .args([
            "-map",
            "0:V:0",
            "-an",
            "-sn",
            "-dn",
            "-frames:v",
            "1",
            "-filter_threads",
            "1",
            "-vf",
        ])
        .arg(format!("scale={width}:{height},setsar=1"))
        .args(["-f", "image2pipe", "-vcodec", "png", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() as u64 <= OUTPUT_LIMIT).then_some(bytes)
    });
    let started = Instant::now();
    let status = loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < EXTRACT_TIMEOUT => {
                thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader.join().ok()??;
    if !status?.success() {
        return None;
    }
    let pixels = image::load_from_memory(&bytes).ok()?.into_rgba8();
    (pixels.width() == width && pixels.height() == height).then_some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(path: &str) -> Video {
        Video {
            path: path.into(),
            title: path.into(),
            duration: 100.0,
            width: 320,
            height: 180,
            display_aspect: 16.0 / 9.0,
            video_codec: "h264".into(),
            audio_codec: None,
        }
    }

    fn pending(
        thumbnails: &mut VideoThumbnails,
        path: &Path,
    ) -> mpsc::Sender<Option<image::RgbaImage>> {
        let (sender, receiver) = mpsc::channel();
        thumbnails.pending = Some(PendingThumbnail {
            generation: thumbnails.generation,
            path: path.to_owned(),
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        sender
    }

    #[test]
    fn samples_avoid_edges_and_unknown_duration_uses_a_conservative_seek() {
        for duration in [0.01, 0.5, 1.0, 10.0, 3600.0] {
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0 - f64::EPSILON] {
                let position = frame_position(duration, fraction);
                assert!(position >= duration * 0.15 && position <= duration * 0.85);
                assert!(position > 0.0 && position < duration);
            }
        }
        for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(frame_position(duration, 0.5), 1.0);
        }
    }

    #[test]
    fn random_selection_varies_by_video_and_survives_cache_eviction_and_clear() {
        let mut thumbnails = VideoThumbnails::default();
        let first = video("0.mp4");
        let position = thumbnails.position(&first);
        for index in 0..=CACHE_ENTRIES {
            let path = PathBuf::from(format!("{index}.mp4"));
            pending(&mut thumbnails, &path)
                .send(Some(image::RgbaImage::new(96, 54)))
                .unwrap();
            assert!(thumbnails.poll());
        }
        assert_eq!(thumbnails.cache.len(), CACHE_ENTRIES);
        assert!(thumbnails.cached(Path::new("0.mp4")).is_none());
        assert_eq!(thumbnails.position(&first), position);
        thumbnails.clear();
        assert_eq!(thumbnails.position(&first), position);
        let positions: HashSet<_> = (0..16)
            .map(|index| {
                thumbnails
                    .position(&video(&format!("{index}.mp4")))
                    .to_bits()
            })
            .collect();
        assert!(
            positions.len() > 1,
            "different videos must not all use the same offset"
        );
    }

    #[test]
    fn scales_display_aspect_without_cropping_or_exceeding_pixel_bound() {
        assert_eq!(thumbnail_size(16.0 / 9.0), Some((96, 54)));
        assert_eq!(thumbnail_size(9.0 / 16.0), Some((54, 96)));
        // 320x180 with SAR 2:1 and a 90-degree display rotation.
        assert_eq!(thumbnail_size(9.0 / 32.0), Some((27, 96)));
        assert_eq!(thumbnail_size(1.0), Some((96, 96)));
        assert_eq!(thumbnail_size(1000.0), Some((96, 1)));
        for aspect in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(thumbnail_size(aspect).is_none());
        }
    }

    #[test]
    fn cleared_generation_discards_old_pixels_and_failures_do_not_repeat() {
        let path = PathBuf::from("fixture.mp4");
        let mut thumbnails = VideoThumbnails::default();
        let sender = pending(&mut thumbnails, &path);
        let cancelled = thumbnails.pending.as_ref().unwrap().cancelled.clone();
        thumbnails.clear();
        assert!(cancelled.load(Ordering::Relaxed));
        assert!(
            !thumbnails.request(&video("another.mp4")),
            "wait for the old worker to exit"
        );
        sender.send(Some(image::RgbaImage::new(96, 54))).unwrap();
        assert!(!thumbnails.poll());
        assert!(thumbnails.cached(&path).is_none());
        let sender = pending(&mut thumbnails, &path);
        sender.send(None).unwrap();
        assert!(thumbnails.poll());
        assert!(thumbnails.failed.contains(&path));
        assert!(thumbnails.request(&video("fixture.mp4")));
        assert!(thumbnails.pending.is_none());
        let sender = pending(&mut thumbnails, Path::new("working.mp4"));
        sender.send(Some(image::RgbaImage::new(96, 54))).unwrap();
        assert!(thumbnails.poll());
        assert_eq!(
            thumbnails
                .cached(Path::new("working.mp4"))
                .unwrap()
                .size()
                .width,
            96
        );
        assert!(thumbnails.request(&video("working.mp4")));
        assert!(thumbnails.pending.is_none());
    }

    #[test]
    fn dropping_cache_cancels_inflight_extraction() {
        let mut thumbnails = VideoThumbnails::default();
        let _sender = pending(&mut thumbnails, Path::new("fixture.mp4"));
        let cancelled = thumbnails.pending.as_ref().unwrap().cancelled.clone();
        drop(thumbnails);
        assert!(cancelled.load(Ordering::Relaxed));
    }

    #[cfg(unix)]
    fn mock_extractor(directory: &Path, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = directory.join(format!("{name}.sh"));
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn process_output_limit_rejects_oversized_png_and_cancellation_kills_the_worker() {
        let directory = tempfile::tempdir().unwrap();
        let media = directory.path().join("input.mp4");
        std::fs::write(&media, "mock media").unwrap();
        let oversized = mock_extractor(
            directory.path(),
            "oversized",
            "cat \"$0.png\"; exec /usr/bin/head -c 65536 /dev/zero",
        );
        image::RgbaImage::new(96, 54)
            .save(oversized.with_extension("sh.png"))
            .unwrap();
        let cancelled = AtomicBool::new(false);
        assert!(extract_using(&media, 1.0, 16.0 / 9.0, &cancelled, &oversized).is_none());

        let blocked = mock_extractor(directory.path(), "blocked", "exec sleep 30");
        let started = Instant::now();
        thread::scope(|scope| {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(100));
                cancelled.store(true, Ordering::Relaxed);
            });
            assert!(extract_using(&media, 1.0, 16.0 / 9.0, &cancelled, &blocked).is_none());
        });
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[cfg(unix)]
    fn process_deadline_stops_a_stalled_decoder() {
        let directory = tempfile::tempdir().unwrap();
        let media = directory.path().join("input.mp4");
        std::fs::write(&media, "mock media").unwrap();
        let blocked = mock_extractor(directory.path(), "blocked", "exec sleep 30");
        let started = Instant::now();
        assert!(
            extract_using(&media, 1.0, 16.0 / 9.0, &AtomicBool::new(false), &blocked).is_none()
        );
        assert!(started.elapsed() >= EXTRACT_TIMEOUT);
        assert!(started.elapsed() < EXTRACT_TIMEOUT + Duration::from_secs(2));
    }

    #[test]
    #[ignore = "Requires FFmpeg and ffprobe; generates silent media, never opens a player"]
    fn native_extraction_uses_middle_frames_and_respects_rotation_and_sar() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("middle.mp4");
        assert!(Command::new(ffmpeg()).args([
            "-v", "error", "-f", "lavfi", "-i", "color=c=green:size=320x180:rate=20:duration=6",
            "-vf", "drawbox=color=red:t=fill:enable='lt(t,0.8)',drawbox=color=blue:t=fill:enable='gte(t,5.2)'",
            "-c:v", "libx264", "-pix_fmt", "yuv420p",
        ]).arg(&path).status().unwrap().success());
        let mut source = crate::video::library::probe(&path).unwrap();
        let thumbnails = VideoThumbnails::default();
        let cancelled = AtomicBool::new(false);
        let pixels = extract(
            &path,
            thumbnails.position(&source),
            source.display_aspect,
            &cancelled,
        )
        .unwrap();
        assert_eq!(pixels.dimensions(), (96, 54));
        let middle = pixels.get_pixel(48, 27);
        assert!(
            middle[1] > 80 && middle[0] < 30 && middle[2] < 30,
            "expected green middle frame: {middle:?}"
        );
        source.duration = f64::NAN;
        assert!(
            extract(
                &path,
                thumbnails.position(&source),
                source.display_aspect,
                &cancelled
            )
            .is_some()
        );

        let anamorphic = directory.path().join("anamorphic.mp4");
        assert!(
            Command::new(ffmpeg())
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=red:size=320x180:rate=20:duration=2",
                    "-vf",
                    "drawbox=x=iw/2:y=0:w=iw/2:h=ih:color=blue:t=fill,setsar=2",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                ])
                .arg(&anamorphic)
                .status()
                .unwrap()
                .success()
        );
        let rotated = directory.path().join("rotated.mp4");
        assert!(
            Command::new(ffmpeg())
                .args(["-v", "error", "-display_rotation", "90", "-i"])
                .arg(&anamorphic)
                .args(["-c", "copy"])
                .arg(&rotated)
                .status()
                .unwrap()
                .success()
        );
        let source = crate::video::library::probe(&rotated).unwrap();
        assert!(
            (source.display_aspect - 9.0 / 32.0).abs() < 0.001,
            "{source:?}"
        );
        let pixels = extract(
            &rotated,
            thumbnails.position(&source),
            source.display_aspect,
            &cancelled,
        )
        .unwrap();
        assert_eq!(pixels.dimensions(), (27, 96));
        let top = pixels.get_pixel(13, 12);
        let bottom = pixels.get_pixel(13, 84);
        assert!(
            top[2] > 200 && top[0] < 30,
            "rotation must put blue above red: {top:?}"
        );
        assert!(
            bottom[0] > 200 && bottom[2] < 30,
            "rotation must put red below blue: {bottom:?}"
        );
    }
}
