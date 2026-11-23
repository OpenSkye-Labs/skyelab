//! Turning a page snapshot into the PNG an agent gets: no wider than [`MAX_SHOT_WIDTH`], no bigger than
//! [`MAX_SHOT_BYTES`].

use anyhow::{Context as _, bail};
use image::{DynamicImage, ImageFormat, imageops::FilterType};

/// Widest screenshot handed to an agent, in pixels.
pub const MAX_SHOT_WIDTH: u32 = 1600;
/// Largest PNG handed to an agent.
pub const MAX_SHOT_BYTES: usize = 1024 * 1024;
/// Halving stops here: a busier page than this fits is refused, not shrunk to mush.
const MIN_SHOT_WIDTH: u32 = 200;

/// A screenshot ready for the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Decodes `raw` (any format the engine hands back; TIFF on macOS), scales it to at most [`MAX_SHOT_WIDTH`]
/// wide and encodes a PNG, halving the width until it fits [`MAX_SHOT_BYTES`].
pub fn encode_png(raw: &[u8]) -> anyhow::Result<Shot> {
    let image = image::load_from_memory(raw).context("decode the snapshot")?;
    let mut width = image.width().min(MAX_SHOT_WIDTH);
    loop {
        let png = png_at(&image, width)?;
        if png.len() <= MAX_SHOT_BYTES {
            let (w, h) = image::ImageReader::new(std::io::Cursor::new(&png))
                .with_guessed_format()
                .context("read the PNG back")?
                .into_dimensions()
                .context("read the PNG's size")?;
            return Ok(Shot {
                png,
                width: w,
                height: h,
            });
        }
        if width / 2 < MIN_SHOT_WIDTH {
            bail!(
                "the screenshot is over {MAX_SHOT_BYTES} bytes even at {width} px wide; scroll to a simpler part of the page"
            );
        }
        width /= 2;
    }
}

fn png_at(image: &DynamicImage, width: u32) -> anyhow::Result<Vec<u8>> {
    let scaled = match width < image.width() {
        true => {
            let height =
                (u64::from(image.height()) * u64::from(width) / u64::from(image.width())).max(1);
            let height = u32::try_from(height).context("screenshot height")?;
            image.resize_exact(width, height, FilterType::Triangle)
        }
        false => image.clone(),
    };
    let mut png = Vec::new();
    // RGBA8: WebKit's TIFF may be 16-bit or premultiplied, which agents' decoders handle worse.
    DynamicImage::ImageRgba8(scaled.to_rgba8())
        .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
        .context("encode the PNG")?;
    Ok(png)
}

#[cfg(test)]
mod tests;
