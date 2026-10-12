use super::artwork::decode_thumbnail;
use crate::library::{Track, scanner};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::{Hash, Hasher},
    io::{Cursor, Read},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver},
    },
    thread,
};

const CACHE_ENTRIES: usize = 64;
const CACHE_BYTES: u64 = 32 * 1024 * 1024;
const PREVIEW_RESOLUTION: u32 = 96;
const PREVIEW_FILE_BYTES: u64 = 64 * 1024;

pub(super) struct Previews {
    pixels: Vec<image::RgbaImage>,
    tracks: Vec<(PathBuf, usize)>,
}

pub(super) struct Prepared<T> {
    pub value: T,
    pub previews: Previews,
}

pub(super) enum ImportEvent {
    SelectionClosed,
    Completed(Result<Option<Prepared<scanner::ScanResult>>, String>),
}

// Publish library metadata together with its tiny covers, never decode on entry.
pub(super) fn load(database_path: PathBuf) -> Receiver<Result<Prepared<Vec<Track>>, String>> {
    let cache = database_path.with_file_name("cover-previews-v1");
    let loading = scanner::load(database_path);
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        if let Ok(result) = loading.recv() {
            let _ = sender.send(result.map(|tracks| Prepared {
                previews: prepare(&tracks, &cache),
                value: tracks,
            }));
        }
    });
    receiver
}

pub(super) fn import(database_path: PathBuf, title: String) -> Receiver<ImportEvent> {
    let cache = database_path.with_file_name("cover-previews-v1");
    let importing = scanner::import(database_path, title);
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        while let Ok(event) = importing.recv() {
            let event = match event {
                scanner::ImportEvent::SelectionClosed => ImportEvent::SelectionClosed,
                scanner::ImportEvent::Completed(result) => {
                    ImportEvent::Completed(result.map(|result| {
                        result.map(|result| Prepared {
                            previews: prepare(&result.tracks, &cache),
                            value: result,
                        })
                    }))
                }
            };
            if sender.send(event).is_err() {
                break;
            }
        }
    });
    receiver
}

fn prepare(tracks: &[Track], cache: &std::path::Path) -> Previews {
    let _ = std::fs::create_dir_all(cache);
    let mut previews = Previews {
        pixels: Vec::new(),
        tracks: Vec::new(),
    };
    // Most tracks in an album have identical embedded art. Decode/store it once.
    let mut known: HashMap<&[u8], Option<usize>> = HashMap::new();
    for track in tracks.iter().filter(|track| !track.artwork.is_empty()) {
        let preview = *known.entry(&track.artwork).or_insert_with(|| {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            track.artwork.hash(&mut hash);
            let path = cache.join(format!("{:016x}.png", hash.finish()));
            let pixels = read_preview(&path).or_else(|| {
                let pixels = decode_thumbnail(&track.artwork, PREVIEW_RESOLUTION)?;
                let mut encoded = Cursor::new(Vec::new());
                if pixels
                    .write_to(&mut encoded, image::ImageFormat::Png)
                    .is_ok()
                    && encoded.get_ref().len() as u64 <= PREVIEW_FILE_BYTES
                {
                    // A partial file after a crash is harmless: read_preview validates it.
                    let _ = std::fs::write(&path, encoded.into_inner());
                }
                Some(pixels)
            })?;
            let id = previews.pixels.len();
            previews.pixels.push(pixels);
            Some(id)
        });
        if let Some(preview) = preview {
            previews.tracks.push((track.path.clone(), preview));
        }
    }
    previews
}

fn read_preview(path: &std::path::Path) -> Option<image::RgbaImage> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(PREVIEW_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > PREVIEW_FILE_BYTES {
        return None;
    }
    let (width, height) =
        image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Png)
            .into_dimensions()
            .ok()?;
    if width == 0 || height == 0 || width > PREVIEW_RESOLUTION || height > PREVIEW_RESOLUTION {
        return None;
    }
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .ok()
        .map(|image| image.to_rgba8())
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    generation: u64,
    track: usize,
    resolution: u32,
}

struct Request {
    key: Key,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Jobs {
    queued: VecDeque<Request>,
    active: HashSet<Key>,
    stopped: bool,
}

struct Decoded {
    key: Key,
    pixels: Option<image::RgbaImage>,
}

// Only visible covers are queued. A new viewport replaces obsolete waiting work.
pub(super) struct Thumbnails {
    jobs: Arc<(Mutex<Jobs>, Condvar)>,
    decoded: Receiver<Decoded>,
    cache: VecDeque<(Key, slint::Image)>,
    previews: HashMap<usize, slint::Image>,
    visible: Vec<Key>,
    generation: u64,
}

impl Default for Thumbnails {
    fn default() -> Self {
        let jobs = Arc::new((Mutex::new(Jobs::default()), Condvar::new()));
        let (sender, decoded) = mpsc::channel();
        for _ in 0..2 {
            let jobs = jobs.clone();
            let sender = sender.clone();
            thread::spawn(move || {
                loop {
                    let request = {
                        let (lock, wake) = &*jobs;
                        let mut pending = lock.lock().unwrap();
                        while pending.queued.is_empty() && !pending.stopped {
                            pending = wake.wait(pending).unwrap();
                        }
                        if pending.stopped {
                            break;
                        }
                        let request = pending.queued.pop_front().unwrap();
                        pending.active.insert(request.key);
                        request
                    };
                    let pixels = decode_thumbnail(&request.bytes, request.key.resolution);
                    let sent = sender.send(Decoded {
                        key: request.key,
                        pixels,
                    });
                    jobs.0.lock().unwrap().active.remove(&request.key);
                    if sent.is_err() {
                        break;
                    }
                }
            });
        }
        Self {
            jobs,
            decoded,
            cache: VecDeque::new(),
            previews: HashMap::new(),
            visible: Vec::new(),
            generation: 0,
        }
    }
}

impl Thumbnails {
    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.visible.clear();
        self.cache.clear();
        self.previews.clear();
        self.jobs.0.lock().unwrap().queued.clear();
    }

    pub fn seed(&mut self, tracks: &[Track], previews: Previews) {
        let images: Vec<_> = previews.pixels.into_iter().map(to_image).collect();
        let indices: HashMap<_, _> = tracks
            .iter()
            .enumerate()
            .map(|(id, track)| (&track.path, id))
            .collect();
        self.previews = previews
            .tracks
            .into_iter()
            .filter_map(|(path, preview)| {
                indices.get(&path).map(|id| (*id, images[preview].clone()))
            })
            .collect();
    }

    pub fn cached(&self, track: usize) -> Option<slint::Image> {
        self.cache
            .iter()
            .filter(|(key, image)| key.track == track && image.size().width > 0)
            .max_by_key(|(key, _)| key.resolution)
            .map(|(_, image)| image.clone())
            .or_else(|| self.previews.get(&track).cloned())
    }

    pub fn cached_images(&self) -> impl Iterator<Item = (usize, &slint::Image)> {
        self.cache
            .iter()
            .map(|(key, image)| (key.track, image))
            .chain(self.previews.iter().map(|(track, image)| (*track, image)))
    }

    pub fn set_visible(&mut self, tracks: &[Track], ids: &[usize], resolution: u32) {
        let keys: Vec<_> = ids
            .iter()
            .filter(|id| {
                tracks
                    .get(**id)
                    .is_some_and(|track| !track.artwork.is_empty())
            })
            .map(|track| Key {
                generation: self.generation,
                track: *track,
                resolution: resolution.max(1),
            })
            .collect();
        if keys == self.visible {
            return;
        }
        // Revisited cached rows remain recent even when no decode is necessary.
        for key in &keys {
            if let Some(index) = self.cache.iter().position(|(cached, _)| cached == key) {
                let entry = self.cache.remove(index).unwrap();
                self.cache.push_back(entry);
            }
        }
        let (lock, wake) = &*self.jobs;
        let mut jobs = lock.lock().unwrap();
        let mut previous = std::mem::take(&mut jobs.queued);
        for key in &keys {
            if self.cache.iter().any(|(cached, _)| cached == key)
                || (key.resolution <= PREVIEW_RESOLUTION && self.previews.contains_key(&key.track))
                || jobs.active.contains(key)
            {
                continue;
            }
            let request = previous
                .iter()
                .position(|request| request.key == *key)
                .and_then(|index| previous.remove(index))
                .unwrap_or_else(|| Request {
                    key: *key,
                    bytes: tracks[key.track].artwork.clone(),
                });
            jobs.queued.push_back(request);
        }
        self.visible = keys;
        wake.notify_all();
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(decoded) = self.decoded.try_recv() {
            if decoded.key.generation != self.generation {
                continue;
            }
            let image = decoded.pixels.map(to_image).unwrap_or_default();
            if let Some(index) = self.cache.iter().position(|(key, _)| *key == decoded.key) {
                self.cache.remove(index);
            }
            self.cache.push_back((decoded.key, image));
            while self.cache.len() > CACHE_ENTRIES || self.cache_bytes() > CACHE_BYTES {
                self.cache.pop_front();
            }
            changed = true;
        }
        changed
    }

    fn cache_bytes(&self) -> u64 {
        self.cache
            .iter()
            .map(|(_, image)| {
                let size = image.size();
                u64::from(size.width) * u64::from(size.height) * 4
            })
            .sum()
    }
}

fn to_image(pixels: image::RgbaImage) -> slint::Image {
    slint::Image::from_rgba8(
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
        ),
    )
}

impl Drop for Thumbnails {
    fn drop(&mut self) {
        let (lock, wake) = &*self.jobs;
        let mut jobs = lock.lock().unwrap();
        jobs.stopped = true;
        jobs.queued.clear();
        wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn track(id: usize, artwork: Vec<u8>) -> Track {
        Track {
            path: format!("{id}.mp3").into(),
            title: id.to_string(),
            artist: "Artist".into(),
            album: id.to_string(),
            genre: String::new(),
            number: 1,
            duration: 60.0,
            artwork,
            lyrics: Default::default(),
        }
    }

    fn controlled() -> (Thumbnails, mpsc::Sender<Decoded>) {
        let (sender, decoded) = mpsc::channel();
        (
            Thumbnails {
                jobs: Arc::new((Mutex::new(Jobs::default()), Condvar::new())),
                decoded,
                cache: VecDeque::new(),
                previews: HashMap::new(),
                visible: Vec::new(),
                generation: 0,
            },
            sender,
        )
    }

    #[test]
    fn viewport_changes_drop_obsolete_waiting_jobs_and_keep_active_work_deduplicated() {
        let (mut thumbnails, _) = controlled();
        let tracks: Vec<_> = (0..1000).map(|id| track(id, vec![1])).collect();
        thumbnails.set_visible(&tracks, &[0, 1], 96);
        let active = {
            let mut jobs = thumbnails.jobs.0.lock().unwrap();
            assert_eq!(
                jobs.queued.len(),
                2,
                "only requested visible rows are queued"
            );
            let active = jobs.queued.pop_front().unwrap().key;
            jobs.active.insert(active);
            active
        };
        thumbnails.set_visible(&tracks, &[0, 2], 96);
        let jobs = thumbnails.jobs.0.lock().unwrap();
        assert!(jobs.active.contains(&active));
        assert_eq!(jobs.queued.len(), 1);
        assert_eq!(
            jobs.queued[0].key.track, 2,
            "old page must not delay new page"
        );
    }

    #[test]
    fn an_import_invalidates_old_results_and_failed_images_are_not_retried() {
        let (mut thumbnails, sender) = controlled();
        let tracks = vec![track(0, vec![1])];
        thumbnails.set_visible(&tracks, &[0], 96);
        let old = thumbnails
            .jobs
            .0
            .lock()
            .unwrap()
            .queued
            .pop_front()
            .unwrap()
            .key;
        thumbnails.clear();
        sender
            .send(Decoded {
                key: old,
                pixels: Some(image::RgbaImage::new(96, 96)),
            })
            .unwrap();
        assert!(!thumbnails.poll());
        assert!(thumbnails.cached(0).is_none());
        thumbnails.set_visible(&tracks, &[0], 96);
        let current = thumbnails
            .jobs
            .0
            .lock()
            .unwrap()
            .queued
            .pop_front()
            .unwrap()
            .key;
        assert_ne!(old.generation, current.generation);
        sender
            .send(Decoded {
                key: current,
                pixels: None,
            })
            .unwrap();
        assert!(thumbnails.poll());
        thumbnails.set_visible(&tracks, &[], 96);
        thumbnails.set_visible(&tracks, &[0], 96);
        assert!(thumbnails.jobs.0.lock().unwrap().queued.is_empty());
    }

    #[test]
    fn cache_stays_bounded_and_cached_viewports_require_no_decode() {
        let (mut thumbnails, sender) = controlled();
        for track in 0..4 {
            sender
                .send(Decoded {
                    key: Key {
                        generation: 0,
                        track,
                        resolution: 1536,
                    },
                    pixels: Some(image::RgbaImage::new(1536, 1536)),
                })
                .unwrap();
            thumbnails.poll();
        }
        assert_eq!(thumbnails.cache.len(), 3);
        assert!(thumbnails.cache_bytes() <= CACHE_BYTES);
        let tracks: Vec<_> = (0..4).map(|id| track(id, vec![1])).collect();
        thumbnails.set_visible(&tracks, &[1, 2, 3], 1536);
        assert!(thumbnails.jobs.0.lock().unwrap().queued.is_empty());
    }

    #[test]
    fn workers_decode_thumbnail_pixels_off_thread() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(128, 64, image::Rgb([210, 50, 90]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let tracks = vec![track(0, bytes.into_inner())];
        let mut thumbnails = Thumbnails::default();
        thumbnails.set_visible(&tracks, &[0], 32);
        assert!(thumbnails.cached(0).is_none());
        let deadline = Instant::now() + Duration::from_secs(5);
        let image = loop {
            thumbnails.poll();
            if let Some(image) = thumbnails.cached(0) {
                break image;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        };
        assert_eq!((image.size().width, image.size().height), (32, 16));
    }

    #[test]
    fn prepared_previews_are_small_shared_and_corrupt_cache_files_are_repaired() {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbImage::from_pixel(512, 256, image::Rgb([30, 180, 210]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let tracks = vec![
            track(0, bytes.get_ref().clone()),
            track(1, bytes.into_inner()),
        ];
        let directory = tempfile::tempdir().unwrap();
        let previews = prepare(&tracks, directory.path());
        assert_eq!(previews.pixels.len(), 1, "one preview per unique cover");
        assert_eq!(previews.tracks.len(), 2);
        assert_eq!(previews.pixels[0].dimensions(), (96, 48));
        let path = std::fs::read_dir(directory.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(path.metadata().unwrap().len() <= PREVIEW_FILE_BYTES);
        assert!(read_preview(&path).is_some());
        std::fs::write(&path, b"interrupted cache write").unwrap();
        assert!(read_preview(&path).is_none());
        assert_eq!(prepare(&tracks, directory.path()).pixels.len(), 1);
        assert!(read_preview(&path).is_some());
    }

    #[test]
    fn ready_previews_follow_paths_after_library_merge_and_need_no_worker() {
        let (mut thumbnails, _) = controlled();
        let tracks = [track(0, vec![1]), track(1, vec![2])];
        let previews = Previews {
            pixels: vec![image::RgbaImage::new(96, 48), image::RgbaImage::new(48, 96)],
            tracks: vec![(tracks[0].path.clone(), 0), (tracks[1].path.clone(), 1)],
        };
        let reordered = vec![tracks[1].clone(), tracks[0].clone()];
        thumbnails.seed(&reordered, previews);
        assert_eq!(thumbnails.cached(0).unwrap().size().height, 96);
        assert_eq!(thumbnails.cached(1).unwrap().size().height, 48);
        thumbnails.set_visible(&reordered, &[0, 1], 96);
        assert!(thumbnails.jobs.0.lock().unwrap().queued.is_empty());
        thumbnails.clear();
        assert!(thumbnails.cached(0).is_none());
    }
}
