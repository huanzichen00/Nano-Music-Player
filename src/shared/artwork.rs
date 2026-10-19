//! Scaled cover decoding shared by desktop and phone workers.
pub(crate) fn jpeg_thumbnail(bytes: &[u8], resolution: u32) -> Option<image::RgbaImage> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    let requested = u16::try_from(resolution).ok()?;
    decoder.scale(requested, requested).ok()?;
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    let image = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(width, height, pixels)?)
        }
        jpeg_decoder::PixelFormat::L8 => {
            image::DynamicImage::ImageLuma8(image::GrayImage::from_raw(width, height, pixels)?)
        }
        _ => return None,
    };
    Some(image.thumbnail(resolution, resolution).to_rgba8())
}

pub(crate) fn decode_thumbnail(bytes: &[u8], resolution: u32) -> Option<image::RgbaImage> {
    jpeg_thumbnail(bytes, resolution).or_else(|| {
        image::load_from_memory(bytes)
            .ok()
            .map(|image| image.thumbnail(resolution, resolution).to_rgba8())
    })
}
