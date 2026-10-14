use crate::video::library::Video;
use std::{
    collections::{HashSet, VecDeque},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const SIZE: u32 = 96;
const CACHE_ENTRIES: usize = 64;

/// At most one extraction runs at a time. UI images remain on the UI thread.
#[derive(Default)]
pub(super) struct VideoThumbnails {
    generation: u64,
    pending: Option<(u64, PathBuf, Receiver<Option<image::RgbaImage>>)>,
    cache: VecDeque<(PathBuf, slint::Image)>,
    failed: HashSet<PathBuf>,
}

impl VideoThumbnails {
    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cache.clear();
        self.failed.clear();
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
        let duration = video.duration;
        let aspect = video.display_aspect;
        let (sender, receiver) = mpsc::channel();
        let worker_path = path.clone();
        thread::spawn(move || {
            let _ = sender.send(extract(&worker_path, duration, aspect));
        });
        self.pending = Some((self.generation, path, receiver));
        false
    }
    pub fn poll(&mut self) -> bool {
        let Some((_, _, receiver)) = self.pending.as_ref() else {
            return false;
        };
        let pixels = match receiver.try_recv() {
            Ok(pixels) => pixels,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        let (generation, path, _) = self.pending.take().unwrap();
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

fn extract(path: &Path, duration: f64, aspect: f64) -> Option<image::RgbaImage> {
    if !duration.is_finite() || duration <= 0.0 || !aspect.is_finite() || aspect <= 0.0 {
        return None;
    }
    let path = std::fs::canonicalize(path).ok()?;
    let (width, height) = if aspect >= 1.0 {
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
    };
    let mut child = Command::new(ffmpeg())
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
        .arg((duration * 0.1).min(1.0).to_string())
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
        stdout.take(65_537).read_to_end(&mut bytes).ok()?;
        (bytes.len() <= 65_536).then_some(bytes)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < Duration::from_secs(5) => {
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
    #[test]
    fn cleared_generation_discards_old_pixels_and_failures_do_not_repeat() {
        let path = PathBuf::from("fixture.mp4");
        let mut thumbnails = VideoThumbnails::default();
        let (sender, receiver) = mpsc::channel();
        thumbnails.pending = Some((0, path.clone(), receiver));
        thumbnails.clear();
        sender.send(Some(image::RgbaImage::new(96, 54))).unwrap();
        assert!(!thumbnails.poll());
        assert!(thumbnails.cached(&path).is_none());
        let (sender, receiver) = mpsc::channel();
        thumbnails.pending = Some((thumbnails.generation, path.clone(), receiver));
        sender.send(None).unwrap();
        assert!(thumbnails.poll());
        assert!(thumbnails.failed.contains(&path));
        let (sender, receiver) = mpsc::channel();
        thumbnails.pending = Some((thumbnails.generation, "working.mp4".into(), receiver));
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
    }
}
