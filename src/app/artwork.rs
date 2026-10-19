use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

// Room for both a small preview and a finished cover for recent songs.
const CACHE_ENTRIES: usize = 16;
const CACHE_BYTES: u64 = 32 * 1024 * 1024;
const PREVIEW_RESOLUTION: u32 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    generation: u64,
    track: usize,
    resolution: u32,
}

struct Request {
    key: Key,
    bytes: Vec<u8>,
}

struct Decoded {
    key: Key,
    pixels: Option<image::RgbaImage>,
}

// Slint images stay on the UI thread; decoding and resizing run on this worker.
pub(super) struct ArtworkLoader {
    requests: Sender<Request>,
    decoded: Receiver<Decoded>,
    cache: VecDeque<(Key, slint::Image)>,
    wanted: Option<Key>,
    displayed_resolution: u32,
    generation: u64,
}

impl Default for ArtworkLoader {
    fn default() -> Self {
        let (requests, receiver) = mpsc::channel::<Request>();
        let (sender, decoded) = mpsc::channel();
        thread::spawn(move || {
            let mut queued = None;
            while let Some(request) = queued.take().or_else(|| receiver.recv().ok()) {
                // Rapid Next/Previous presses only need the newest waiting cover.
                let request = receiver.try_iter().last().unwrap_or(request);
                let mut original = None;
                if request.key.resolution > PREVIEW_RESOLUTION {
                    let preview =
                        jpeg_thumbnail(&request.bytes, PREVIEW_RESOLUTION).or_else(|| {
                            // PNG and other formats are decoded once, then reused for both sizes.
                            original = image::load_from_memory(&request.bytes).ok();
                            original.as_ref().map(|image| {
                                image
                                    .thumbnail(PREVIEW_RESOLUTION, PREVIEW_RESOLUTION)
                                    .to_rgba8()
                            })
                        });
                    if let Some(pixels) = preview
                        && sender
                            .send(Decoded {
                                key: Key {
                                    resolution: PREVIEW_RESOLUTION,
                                    ..request.key
                                },
                                pixels: Some(pixels),
                            })
                            .is_err()
                    {
                        break;
                    }
                    // A new song takes priority over sharpening the previous song's preview.
                    if let Some(request) = receiver.try_iter().last() {
                        queued = Some(request);
                        continue;
                    }
                }
                let thumbnail = |image: image::DynamicImage| {
                    image
                        .thumbnail(request.key.resolution, request.key.resolution)
                        .to_rgba8()
                };
                let pixels = original.map(thumbnail).or_else(|| {
                    jpeg_thumbnail(&request.bytes, request.key.resolution)
                        .or_else(|| image::load_from_memory(&request.bytes).ok().map(thumbnail))
                });
                if sender
                    .send(Decoded {
                        key: request.key,
                        pixels,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            requests,
            decoded,
            cache: VecDeque::new(),
            wanted: None,
            displayed_resolution: 0,
            generation: 0,
        }
    }
}

impl ArtworkLoader {
    pub fn cancel(&mut self) {
        self.wanted = None;
        self.displayed_resolution = 0;
    }

    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cache.clear();
        self.wanted = None;
        self.displayed_resolution = 0;
    }

    pub fn seed_preview(&mut self, track: usize, preview: Option<slint::Image>) {
        let Some(image) = preview.filter(|image| image.size().width > 0) else {
            return;
        };
        let size = image.size();
        let key = Key {
            generation: self.generation,
            track,
            resolution: size.width.max(size.height),
        };
        if !self.cache.iter().any(|(cached, _)| *cached == key) {
            self.cache_image(key, image);
        }
    }

    pub fn request(&mut self, track: usize, bytes: &[u8], resolution: u32) -> slint::Image {
        let key = Key {
            generation: self.generation,
            track,
            resolution: resolution.max(1),
        };
        let previous = self.wanted.replace(key);
        let exact_cached = self.cache.iter().any(|(cached, _)| *cached == key);
        let fallback = self
            .cache
            .iter()
            .enumerate()
            .filter(|(_, (cached, image))| {
                cached.track == key.track
                    && cached.generation == key.generation
                    && image.size().width > 0
            })
            .max_by_key(|(_, (cached, _))| cached.resolution)
            .map(|(index, _)| index);
        let mut image = slint::Image::default();
        self.displayed_resolution = 0;
        if let Some(index) = fallback {
            let entry = self.cache.remove(index).unwrap();
            self.displayed_resolution = entry.0.resolution;
            image = entry.1.clone();
            self.cache.push_back(entry);
        }
        if !exact_cached && previous != Some(key) && !bytes.is_empty() {
            let _ = self.requests.send(Request {
                key,
                bytes: bytes.to_vec(),
            });
        }
        image
    }

    pub fn poll(&mut self) -> Option<slint::Image> {
        let mut current = None;
        while let Ok(decoded) = self.decoded.try_recv() {
            // An import may replace a track while its old cover is being decoded.
            if decoded.key.generation != self.generation {
                continue;
            }
            let image = decoded
                .pixels
                .map(|pixels| {
                    slint::Image::from_rgba8(
                        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                            pixels.as_raw(),
                            pixels.width(),
                            pixels.height(),
                        ),
                    )
                })
                .unwrap_or_default();
            let wanted = self.wanted.is_some_and(|wanted| {
                wanted.track == decoded.key.track
                    && wanted.generation == decoded.key.generation
                    && (wanted == decoded.key
                        || decoded.key.resolution >= self.displayed_resolution)
            });
            // A late preview must never downgrade a sharper cover, or clear it on decode failure.
            if wanted && (image.size().width > 0 || self.displayed_resolution == 0) {
                self.displayed_resolution = decoded.key.resolution;
                current = Some(image.clone());
            }
            self.cache_image(decoded.key, image);
        }
        current
    }

    fn cache_image(&mut self, key: Key, image: slint::Image) {
        if let Some(index) = self.cache.iter().position(|(cached, _)| *cached == key) {
            self.cache.remove(index);
        }
        self.cache.push_back((key, image));
        while self.cache.len() > CACHE_ENTRIES || self.cache_bytes() > CACHE_BYTES {
            self.cache.pop_front();
        }
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

pub(super) use crate::shared::artwork::decode_thumbnail;
use crate::shared::artwork::jpeg_thumbnail;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn controlled_loader() -> (ArtworkLoader, Receiver<Request>, Sender<Decoded>) {
        let (requests, receiver) = mpsc::channel();
        let (sender, decoded) = mpsc::channel();
        (
            ArtworkLoader {
                requests,
                decoded,
                cache: VecDeque::new(),
                wanted: None,
                displayed_resolution: 0,
                generation: 0,
            },
            receiver,
            sender,
        )
    }

    fn result(key: Key, width: u32) -> Decoded {
        Decoded {
            key,
            pixels: Some(image::RgbaImage::new(width, 1)),
        }
    }

    #[test]
    fn old_track_and_invalidated_results_cannot_replace_current_cover() {
        let (mut loader, requests, results) = controlled_loader();
        loader.request(0, &[1], 10);
        let old = requests.recv().unwrap().key;
        loader.request(1, &[1], 10);
        let current = requests.recv().unwrap().key;
        results.send(result(old, 2)).unwrap();
        assert!(loader.poll().is_none());
        results.send(result(current, 3)).unwrap();
        assert_eq!(loader.poll().unwrap().size().width, 3);

        loader.clear();
        loader.request(1, &[1], 10);
        let refreshed = requests.recv().unwrap().key;
        assert_ne!(current, refreshed);
        results.send(result(current, 3)).unwrap();
        assert!(loader.poll().is_none());
        results.send(result(refreshed, 4)).unwrap();
        assert_eq!(loader.poll().unwrap().size().width, 4);

        loader.request(2, &[1], 10);
        let cancelled = requests.recv().unwrap().key;
        loader.cancel();
        results.send(result(cancelled, 5)).unwrap();
        assert!(loader.poll().is_none());
    }

    #[test]
    fn cache_reuses_recent_covers_and_evicts_the_least_recent() {
        let (mut loader, requests, results) = controlled_loader();
        for track in 0..CACHE_ENTRIES {
            loader.request(track, &[1], 10);
            results
                .send(result(requests.recv().unwrap().key, 2))
                .unwrap();
            loader.poll();
        }
        assert_eq!(loader.request(0, &[1], 10).size().width, 2);
        assert!(
            requests.try_recv().is_err(),
            "cache hit must avoid decoding"
        );
        loader.request(CACHE_ENTRIES, &[1], 10);
        results
            .send(result(requests.recv().unwrap().key, 2))
            .unwrap();
        loader.poll();
        assert_eq!(loader.cache.len(), CACHE_ENTRIES);
        assert!(loader.cache_bytes() <= CACHE_BYTES);
        loader.request(1, &[1], 10);
        assert_eq!(requests.recv().unwrap().key.track, 1);
        loader.request(0, &[1], 20);
        assert_eq!(requests.recv().unwrap().key.resolution, 20);
    }

    #[test]
    fn empty_and_invalid_covers_are_handled_without_retrying() {
        let (mut loader, requests, results) = controlled_loader();
        assert_eq!(loader.request(0, &[], 10).size().width, 0);
        assert!(requests.try_recv().is_err());
        loader.request(1, &[1], 10);
        let key = requests.recv().unwrap().key;
        loader.request(1, &[1], 10);
        assert!(
            requests.try_recv().is_err(),
            "pending requests are deduplicated"
        );
        results.send(Decoded { key, pixels: None }).unwrap();
        assert_eq!(loader.poll().unwrap().size().width, 0);
        loader.request(1, &[1], 10);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn large_covers_are_evicted_to_keep_the_cache_within_its_byte_budget() {
        let (mut loader, requests, results) = controlled_loader();
        for track in 0..4 {
            loader.request(track, &[1], 1536);
            results
                .send(Decoded {
                    key: requests.recv().unwrap().key,
                    pixels: Some(image::RgbaImage::new(1536, 1536)),
                })
                .unwrap();
            loader.poll();
        }
        assert_eq!(loader.cache.len(), 3);
        assert!(loader.cache_bytes() <= CACHE_BYTES);
        loader.request(0, &[1], 1536);
        assert_eq!(requests.recv().unwrap().key.track, 0);
    }

    #[test]
    fn worker_decodes_and_resizes_without_blocking_the_request() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(128, 64)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let mut loader = ArtworkLoader::default();
        assert_eq!(loader.request(0, bytes.get_ref(), 32).size().width, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        let image = loop {
            if let Some(image) = loader.poll() {
                break image;
            }
            assert!(Instant::now() < deadline, "artwork worker did not respond");
            thread::sleep(Duration::from_millis(1));
        };
        assert_eq!((image.size().width, image.size().height), (32, 16));
    }

    #[test]
    fn jpeg_worker_sends_a_small_preview_before_the_finished_cover() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(1024, 512, image::Rgb([230, 40, 20]))
            .write_to(&mut bytes, image::ImageFormat::Jpeg)
            .unwrap();
        let mut loader = ArtworkLoader::default();
        loader.request(0, bytes.get_ref(), 512);
        let preview = loader.decoded.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(preview.key.resolution, PREVIEW_RESOLUTION);
        let pixels = preview.pixels.unwrap();
        assert_eq!(pixels.dimensions(), (96, 48));
        assert!(
            pixels.get_pixel(0, 0)[0] > 200,
            "preview preserves the cover's colors"
        );
        let finished = loader.decoded.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(finished.key.resolution, 512);
        assert_eq!(finished.pixels.unwrap().dimensions(), (512, 256));
    }

    #[test]
    fn cached_preview_is_immediate_and_upgraded_without_late_downgrades() {
        let (mut loader, requests, results) = controlled_loader();
        let preview = slint::Image::from_rgba8(slint::SharedPixelBuffer::new(96, 48));
        loader.seed_preview(0, Some(preview));
        assert_eq!(loader.request(0, &[1], 512).size().width, 96);
        let key = requests.recv().unwrap().key;
        results.send(result(key, 512)).unwrap();
        assert_eq!(loader.poll().unwrap().size().width, 512);
        results
            .send(result(
                Key {
                    resolution: PREVIEW_RESOLUTION,
                    ..key
                },
                96,
            ))
            .unwrap();
        assert!(loader.poll().is_none());
        assert_eq!(loader.request(0, &[1], 512).size().width, 512);
        assert!(requests.try_recv().is_err());

        loader.request(1, &[1], 512);
        let other = requests.recv().unwrap().key;
        results
            .send(result(
                Key {
                    resolution: PREVIEW_RESOLUTION,
                    ..key
                },
                96,
            ))
            .unwrap();
        assert!(
            loader.poll().is_none(),
            "previous song's preview stays hidden"
        );
        results
            .send(result(
                Key {
                    resolution: PREVIEW_RESOLUTION,
                    ..other
                },
                96,
            ))
            .unwrap();
        assert_eq!(loader.poll().unwrap().size().width, 96);
        results
            .send(Decoded {
                key: other,
                pixels: None,
            })
            .unwrap();
        assert!(
            loader.poll().is_none(),
            "failed sharpening preserves the preview"
        );
        assert_eq!(loader.request(1, &[1], 512).size().width, 96);
    }
}
