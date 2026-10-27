//! Cover rendering runs off-thread; NSWorkspace calls stay on the main thread.
use image::{Rgb, RgbImage, imageops};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
};

pub(crate) trait Desktop {
    fn apply(&mut self, path: &Path) -> Result<(), String>;
    fn restore(&mut self) -> Result<(), String>;
}

struct Request {
    revision: u64,
    artwork: Vec<u8>,
}

/// A single pending slot bounds memory while a user rapidly skips songs.
pub(crate) struct Wallpaper {
    directory: PathBuf,
    desktop: Option<Box<dyn Desktop>>,
    pending: Arc<Mutex<Option<Request>>>,
    wake: Option<mpsc::SyncSender<()>>,
    completed: Option<mpsc::Receiver<(u64, Result<PathBuf, String>)>>,
    revision: u64,
    artwork: Vec<u8>,
}

impl Wallpaper {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self {
            directory: directory.join("wallpaper"),
            desktop: None,
            pending: Default::default(),
            wake: None,
            completed: None,
            revision: 0,
            artwork: Vec::new(),
        }
    }

    /// Called only by the real app, never by headless controller tests.
    pub(crate) fn start(&mut self) -> Result<(), String> {
        self.artwork.clear();
        #[cfg(target_os = "macos")]
        {
            std::fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
            let mut desktop = native::Native::new(&self.directory.join("originals.archive"))?;
            let result = desktop.restore(); // Recover an interrupted previous session.
            self.desktop = Some(Box::new(desktop));
            result?;
        }
        Ok(())
    }

    pub(crate) fn update(&mut self, artwork: Option<&[u8]>) -> Result<(), String> {
        let artwork = artwork.unwrap_or_default();
        if artwork == self.artwork {
            return Ok(());
        }
        self.revision = self.revision.wrapping_add(1);
        self.artwork = artwork.to_vec();
        *self.pending.lock().unwrap() = None;
        if artwork.is_empty() {
            return self.restore();
        }
        if self.desktop.is_none() {
            return Ok(());
        }
        if self.wake.is_none() {
            let pending = self.pending.clone();
            let directory = self.directory.clone();
            let (wake, receiver) = mpsc::sync_channel(1);
            let (sender, completed) = mpsc::channel();
            thread::spawn(move || {
                while receiver.recv().is_ok() {
                    let Some(request) = pending.lock().unwrap().take() else {
                        continue;
                    };
                    let result = render_to_file(&directory, &request.artwork, request.revision);
                    if let Err(disconnected) = sender.send((request.revision, result)) {
                        if let Ok(path) = disconnected.0.1 {
                            let _ = std::fs::remove_file(path);
                        }
                        break;
                    }
                }
            });
            self.wake = Some(wake);
            self.completed = Some(completed);
        }
        *self.pending.lock().unwrap() = Some(Request {
            revision: self.revision,
            artwork: artwork.to_vec(),
        });
        let _ = self.wake.as_ref().unwrap().try_send(());
        Ok(())
    }

    pub(crate) fn poll(&mut self) -> Result<(), String> {
        let Some(completed) = &self.completed else {
            return Ok(());
        };
        for (revision, result) in completed.try_iter() {
            if revision != self.revision {
                if let Ok(path) = result {
                    let _ = std::fs::remove_file(path);
                }
                continue;
            }
            let result = result.and_then(|path| self.desktop.as_mut().unwrap().apply(&path));
            if let Err(error) = result {
                // A song without decodable artwork must not leave the previous cover up.
                self.restore()?;
                return Err(error);
            }
        }
        Ok(())
    }

    pub(crate) fn restore(&mut self) -> Result<(), String> {
        if let Some(desktop) = &mut self.desktop {
            desktop.restore()?;
        }
        Ok(())
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        self.revision = self.revision.wrapping_add(1);
        *self.pending.lock().unwrap() = None;
        self.artwork.clear();
        self.wake = None;
        if let Some(completed) = self.completed.take() {
            for (_, result) in completed.try_iter() {
                if let Ok(path) = result {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        self.restore()
    }
}

impl Drop for Wallpaper {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Cannot restore desktop wallpaper: {error}");
        }
    }
}

fn render_to_file(directory: &Path, bytes: &[u8], revision: u64) -> Result<PathBuf, String> {
    // Decode at cover resolution first, bounding processing of very large embedded images.
    let cover = crate::shared::artwork::decode_thumbnail(bytes, 1200)
        .ok_or_else(|| "Cannot decode song artwork".to_owned())?;
    let cover = image::DynamicImage::ImageRgba8(cover).to_rgb8();
    let image = compose(&cover, 2560, 1600);
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    // Unique URLs prevent both macOS image caching and stale work overwriting
    // the currently visible cover. Superseded files are removed by poll().
    let path = directory.join(format!("cover-{}-{revision}.png", std::process::id()));
    let temporary = path.with_extension("tmp");
    image
        .save_with_format(&temporary, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn compose(cover: &RgbImage, width: u32, height: u32) -> RgbImage {
    // Blur at a small resolution; avoid an expensive full-screen convolution.
    let small = image::DynamicImage::ImageRgb8(cover.clone())
        .resize_to_fill(160, 100, imageops::FilterType::Triangle)
        .to_rgb8();
    let mut background = imageops::blur(&small, 10.0);
    for pixel in background.pixels_mut() {
        *pixel = Rgb(pixel.0.map(|v| (u16::from(v) * 38 / 100) as u8));
    }
    let mut canvas = imageops::resize(&background, width, height, imageops::FilterType::Triangle);
    let size = height * 72 / 100;
    let foreground = image::DynamicImage::ImageRgb8(cover.clone())
        .resize(width * 80 / 100, size, imageops::FilterType::Lanczos3)
        .to_rgb8();
    let x = (width - foreground.width()) / 2;
    let y = (height - foreground.height()) / 2;
    imageops::replace(&mut canvas, &foreground, i64::from(x), i64::from(y));
    canvas
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::{
        ffi::{CStr, CString, c_char, c_void},
        os::unix::ffi::OsStrExt,
    };

    unsafe extern "C" {
        fn nm_wallpaper_create(
            journal: *const c_char,
            error: *mut c_char,
            capacity: usize,
        ) -> *mut c_void;
        fn nm_wallpaper_apply(
            handle: *mut c_void,
            path: *const c_char,
            error: *mut c_char,
            capacity: usize,
        ) -> bool;
        fn nm_wallpaper_restore(handle: *mut c_void, error: *mut c_char, capacity: usize) -> bool;
        fn nm_wallpaper_destroy(handle: *mut c_void);
    }

    pub(super) struct Native(*mut c_void);

    fn string(path: &Path) -> Result<CString, String> {
        CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())
    }

    fn message(error: &[c_char]) -> String {
        unsafe { CStr::from_ptr(error.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    impl Native {
        pub(super) fn new(path: &Path) -> Result<Self, String> {
            let mut error = [0; 1024];
            let handle = unsafe {
                nm_wallpaper_create(string(path)?.as_ptr(), error.as_mut_ptr(), error.len())
            };
            if handle.is_null() {
                Err(message(&error))
            } else {
                Ok(Self(handle))
            }
        }
    }

    impl Desktop for Native {
        fn apply(&mut self, path: &Path) -> Result<(), String> {
            let mut error = [0; 1024];
            if unsafe {
                nm_wallpaper_apply(
                    self.0,
                    string(path)?.as_ptr(),
                    error.as_mut_ptr(),
                    error.len(),
                )
            } {
                Ok(())
            } else {
                Err(message(&error))
            }
        }

        fn restore(&mut self) -> Result<(), String> {
            let mut error = [0; 1024];
            if unsafe { nm_wallpaper_restore(self.0, error.as_mut_ptr(), error.len()) } {
                Ok(())
            } else {
                Err(message(&error))
            }
        }
    }

    impl Drop for Native {
        fn drop(&mut self) {
            unsafe { nm_wallpaper_destroy(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_writes_distinct_complete_images_and_rejects_invalid_covers() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = include_bytes!("../../assets/generated/music.png");
        let first = render_to_file(directory.path(), bytes, 1).unwrap();
        let second = render_to_file(directory.path(), bytes, 2).unwrap();
        assert_ne!(first, second);
        assert_eq!(image::image_dimensions(&first).unwrap(), (2560, 1600));
        assert!(render_to_file(directory.path(), b"invalid image", 3).is_err());
        let preview = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa/wallpaper");
        std::fs::create_dir_all(&preview).unwrap();
        std::fs::copy(first, preview.join("preview.png")).unwrap();
    }

    #[test]
    fn composition_preserves_cover_aspect_and_softens_the_background() {
        let cover = RgbImage::from_pixel(120, 60, Rgb([200, 100, 50]));
        let result = compose(&cover, 320, 200);
        assert_eq!(result.dimensions(), (320, 200));
        assert_eq!(*result.get_pixel(160, 100), Rgb([200, 100, 50]));
        assert_eq!(*result.get_pixel(0, 0), Rgb([76, 38, 19]));
        // A 2:1 cover is fitted intact rather than stretched to a square.
        assert_eq!(*result.get_pixel(160, 30), Rgb([76, 38, 19]));
        assert_eq!(*result.get_pixel(40, 100), Rgb([200, 100, 50]));
    }

    #[test]
    fn worker_applies_a_complete_cover_once_and_restores_for_missing_artwork() {
        struct Fake(Arc<Mutex<Vec<&'static str>>>);
        impl Desktop for Fake {
            fn apply(&mut self, path: &Path) -> Result<(), String> {
                assert_eq!(image::image_dimensions(path).unwrap(), (2560, 1600));
                self.0.lock().unwrap().push("apply");
                Ok(())
            }
            fn restore(&mut self) -> Result<(), String> {
                self.0.lock().unwrap().push("restore");
                Ok(())
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut wallpaper = Wallpaper::new(directory.path().to_owned());
        wallpaper.desktop = Some(Box::new(Fake(calls.clone())));
        let bytes = include_bytes!("../../assets/generated/music.png");
        wallpaper.update(Some(bytes)).unwrap();
        let revision = wallpaper.revision;
        wallpaper.update(Some(bytes)).unwrap();
        assert_eq!(
            wallpaper.revision, revision,
            "same album cover does not render again"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while calls.lock().unwrap().is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "wallpaper worker timed out"
            );
            wallpaper.poll().unwrap();
            thread::sleep(std::time::Duration::from_millis(10));
        }
        wallpaper.update(Some(&[])).unwrap();
        assert_eq!(*calls.lock().unwrap(), ["apply", "restore"]);
    }

    #[test]
    fn disabled_and_superseded_work_never_applies_a_stale_cover() {
        struct Fake(Arc<Mutex<Vec<String>>>);
        impl Desktop for Fake {
            fn apply(&mut self, _: &Path) -> Result<(), String> {
                self.0.lock().unwrap().push("apply".into());
                Ok(())
            }
            fn restore(&mut self) -> Result<(), String> {
                self.0.lock().unwrap().push("restore".into());
                Ok(())
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut wallpaper = Wallpaper::new(directory.path().to_owned());
        wallpaper.desktop = Some(Box::new(Fake(calls.clone())));
        wallpaper.artwork = vec![1];
        wallpaper.revision = 2;
        let (sender, receiver) = mpsc::channel();
        wallpaper.completed = Some(receiver);
        let stale = directory.path().join("stale.png");
        std::fs::write(&stale, b"stale").unwrap();
        sender.send((1, Ok(stale.clone()))).unwrap();
        wallpaper.poll().unwrap();
        assert!(!stale.exists());
        assert!(calls.lock().unwrap().is_empty());
        wallpaper.update(None).unwrap();
        sender.send((2, Ok(stale))).unwrap();
        wallpaper.poll().unwrap();
        assert_eq!(*calls.lock().unwrap(), ["restore"]);
        wallpaper.artwork = vec![2];
        sender
            .send((wallpaper.revision, Err("bad image".into())))
            .unwrap();
        assert_eq!(wallpaper.poll().unwrap_err(), "bad image");
        assert_eq!(*calls.lock().unwrap(), ["restore", "restore"]);
    }
}
