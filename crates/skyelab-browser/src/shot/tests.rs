use super::*;
use image::{ImageFormat, Rgba, RgbaImage};

fn encoded(image: RgbaImage, format: ImageFormat) -> Vec<u8> {
    let mut out = Vec::new();
    DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut out), format)
        .expect("encode the test image");
    out
}

#[test]
fn wide_snapshot_scaled_to_max_width() {
    let raw = encoded(
        RgbaImage::from_pixel(3200, 400, Rgba([10, 200, 30, 255])),
        ImageFormat::Tiff,
    );
    let shot = encode_png(&raw).expect("encode");
    assert_eq!((shot.width, shot.height), (MAX_SHOT_WIDTH, 200));
    let back = image::load_from_memory_with_format(&shot.png, ImageFormat::Png).expect("a PNG");
    assert_eq!(
        back.to_rgba8().get_pixel(800, 100),
        &Rgba([10, 200, 30, 255])
    );
}

#[test]
fn narrow_snapshot_kept_as_is() {
    let raw = encoded(
        RgbaImage::from_pixel(300, 120, Rgba([0, 0, 0, 255])),
        ImageFormat::Png,
    );
    let shot = encode_png(&raw).expect("encode");
    assert_eq!((shot.width, shot.height), (300, 120));
}

/// Pixels a PNG can't compress.
fn noise(width: u32, height: u32) -> RgbaImage {
    let mut x: u32 = 0x9e37_79b9;
    RgbaImage::from_fn(width, height, |_, _| {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let [a, b, c, _] = x.to_le_bytes();
        Rgba([a, b, c, 255])
    })
}

#[test]
fn busy_snapshot_halved_until_under_byte_cap() {
    // 1600 x 400 of noise is ~2.5 MB as a PNG, so it halves to 800 px (~640 KB).
    let shot = encode_png(&encoded(noise(1600, 400), ImageFormat::Png)).expect("fits once halved");
    assert!(shot.png.len() <= MAX_SHOT_BYTES, "{} bytes", shot.png.len());
    assert_eq!((shot.width, shot.height), (800, 200));
}

#[test]
fn snapshot_too_busy_even_small_refused() {
    // Tall noise stays over the cap all the way down to the minimum width.
    let err = encode_png(&encoded(noise(400, 4000), ImageFormat::Png)).expect_err("too big");
    assert!(err.to_string().contains("bytes"), "{err}");
}

#[test]
fn garbage_refused() {
    assert!(encode_png(b"not an image").is_err());
}
