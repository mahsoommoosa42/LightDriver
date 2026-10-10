//! The approved stem-free rainbow mark, shared by both native UIs.
use iced::{Element, widget::image, window};
use std::{io::Cursor, sync::OnceLock};

const LOGO: &[u8] = include_bytes!("../../../assets/branding/lightdriver-logo-arches-metro.png");
static IMAGE: OnceLock<image::Handle> = OnceLock::new();
static ICON: OnceLock<(Vec<u8>, u32)> = OnceLock::new();

fn decode() -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(Cursor::new(LOGO));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().expect("embedded logo header");
    let mut pixels = vec![0; reader.output_buffer_size().expect("bounded embedded logo")];
    let info = reader
        .next_frame(&mut pixels)
        .expect("embedded logo pixels");
    assert_eq!(
        info.color_type,
        png::ColorType::Rgba,
        "logo must have transparency"
    );
    pixels.truncate(info.buffer_size());
    (info.width, info.height, pixels)
}

pub fn mark<'a, Message: 'a>(width: u32) -> Element<'a, Message> {
    let handle = IMAGE.get_or_init(|| {
        let (w, h, pixels) = decode();
        image::Handle::from_rgba(w, h, pixels)
    });
    image(handle.clone())
        .width(width as f32)
        .height(width as f32 / 2.0)
        .into()
}

/// Letterbox the unchanged mark for square native icon surfaces.
fn icon_pixels() -> &'static (Vec<u8>, u32) {
    ICON.get_or_init(|| {
        let (w, h, pixels) = decode();
        let source = ::image::RgbaImage::from_raw(w, h, pixels).expect("embedded logo dimensions");
        let size = 256;
        let fitted = ::image::imageops::resize(
            &source,
            size,
            size / 2,
            ::image::imageops::FilterType::Lanczos3,
        );
        let mut square = ::image::RgbaImage::new(size, size);
        ::image::imageops::overlay(&mut square, &fitted, 0, i64::from(size / 4));
        (square.into_raw(), size)
    })
}

pub fn window_icon() -> Option<window::Icon> {
    let (pixels, size) = icon_pixels();
    window::icon::from_rgba(pixels.clone(), *size, *size).ok()
}

/// Used only by the developer icon-packaging example, never during normal launch.
pub fn packaged_icon() -> Vec<u8> {
    let (pixels, size) = icon_pixels();
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, *size, *size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .expect("icon encoder")
            .write_image_data(pixels)
            .expect("icon pixels");
    }
    let mut ico = vec![0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 32, 0];
    ico.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend(encoded);
    ico
}
