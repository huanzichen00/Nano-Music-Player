//! Decode covers on a worker; construct Slint images only on the UI thread.
pub(super) use crate::shared::artwork::decode_thumbnail;
use std::{
    io::Cursor,
    sync::mpsc::{self, Receiver, Sender},
};

pub(super) struct Artwork {
    requests: Sender<(usize, Vec<u8>)>,
    results: Receiver<(usize, Option<image::RgbaImage>, Vec<u8>)>,
}

pub(super) struct Cover {
    pub image: slint::Image,
    pub media_bytes: Vec<u8>,
}

impl Default for Artwork {
    fn default() -> Self {
        let (requests, receiver) = mpsc::channel::<(usize, Vec<u8>)>();
        let (sender, results) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(request) = receiver.recv() {
                let (session, bytes) = receiver.try_iter().last().unwrap_or(request);
                let pixels = (|| {
                    let mut reader = image::ImageReader::new(Cursor::new(bytes))
                        .with_guessed_format()
                        .ok()?;
                    let mut limits = image::Limits::default();
                    limits.max_image_width = Some(8192);
                    limits.max_image_height = Some(8192);
                    limits.max_alloc = Some(128 * 1024 * 1024);
                    reader.limits(limits);
                    Some(reader.decode().ok()?.thumbnail(768, 768).to_rgba8())
                })();
                let mut encoded = Vec::new();
                if let Some(pixels) = &pixels {
                    let rgb = image::DynamicImage::ImageRgba8(pixels.clone()).to_rgb8();
                    if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 80)
                        .encode_image(&rgb)
                        .is_err()
                    {
                        encoded.clear();
                    }
                }
                if sender.send((session, pixels, encoded)).is_err() {
                    break;
                }
            }
        });
        Self { requests, results }
    }
}

impl Artwork {
    pub fn request(&self, session: usize, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            let _ = self.requests.send((session, bytes));
        }
    }

    pub fn poll(&self, session: usize) -> Option<Cover> {
        self.results
            .try_iter()
            .filter(|(id, _, _)| *id == session)
            .last()
            .map(|(_, pixels, media_bytes)| Cover {
                image: pixels
                    .map(|pixels| {
                        slint::Image::from_rgba8(
                            slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                                pixels.as_raw(),
                                pixels.width(),
                                pixels.height(),
                            ),
                        )
                    })
                    .unwrap_or_default(),
                media_bytes,
            })
    }
}
