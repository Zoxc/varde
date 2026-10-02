//! A design's thumbnail: the picture of its model the app renders as it
//! saves, written into the file as a PNG [`Preview`] after the record
//! (see [`vrdp`](crate::vrdp)), and read back for the welcome screen.
//! Encoding and decoding happen in the lane, off the UI thread, which
//! only sends and takes plain pixels, an [`Image`].

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::vrdp::Preview;

/// The media type thumbnails are written as.
pub const MEDIA_TYPE: &str = "image/png";

/// The widest and tallest an [`Image`] may be, in pixels: far more than a
/// thumbnail needs, and small enough that its pixels, at most 16 MiB, are
/// no burden to read or send.
pub const MAX_SIDE: u32 = 2048;

/// A picture's pixels: `width` by `height` of them, rows top down, each
/// red, green, blue and alpha, the colour sRGB encoded and not
/// premultiplied. Both sides are from 1 to [`MAX_SIDE`] and the bytes four
/// a pixel, checked as it's made and as it's decoded from the wire.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Unchecked")]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// An [`Image`] as it crosses the wire, before it's checked.
#[derive(Deserialize)]
struct Unchecked {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl TryFrom<Unchecked> for Image {
    type Error = &'static str;

    fn try_from(image: Unchecked) -> Result<Self, Self::Error> {
        Image::new(image.width, image.height, image.rgba).ok_or("an image of the wrong size")
    }
}

impl Image {
    /// The image of `width` by `height` pixels in `rgba`, unless a side
    /// is 0 or past [`MAX_SIDE`], or there aren't four bytes a pixel.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Image> {
        let sides = 1..=MAX_SIDE;
        let bytes = (u64::from(width) * u64::from(height)).checked_mul(4)?;
        (sides.contains(&width)
            && sides.contains(&height)
            && u64::try_from(rgba.len()).ok() == Some(bytes))
        .then_some(Image {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Its pixels, as [`Image`] says.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }
}

/// Its size, not its pixels.
impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({}×{})", self.width, self.height)
    }
}

/// `image` as a PNG preview, or `None` if it won't encode or is larger
/// than a preview may be, which is logged: the design is saved without.
pub(crate) fn encode(image: &Image) -> Option<Preview> {
    let mut bytes = Vec::new();
    let encoded = (|| {
        let mut encoder = png::Encoder::new(&mut bytes, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&image.rgba)?;
        writer.finish()
    })();
    if let Err(error) = encoded {
        log::error!("Couldn't encode the thumbnail: {error}");
        return None;
    }
    let preview = Preview::new(MEDIA_TYPE, bytes);
    if preview.is_none() {
        log::error!("The thumbnail is too large to save");
    }
    preview
}

/// The previews a save writes: `thumbnail` as a PNG, if there's one and
/// it encodes.
pub(crate) fn previews(thumbnail: Option<&Image>) -> Vec<Preview> {
    thumbnail.and_then(encode).into_iter().collect()
}

/// The pixels of the PNG `preview`, or `None` if it isn't one this
/// decodes: a PNG of any colour type and depth within [`MAX_SIDE`], made
/// 8 bit RGBA. A side past it is refused before its pixels are
/// allocated, and the decoder allocates at most a little more than they
/// take.
// The web has no recent files to read them from.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn decode(preview: &Preview) -> Option<Image> {
    if !preview.is(MEDIA_TYPE) {
        return None;
    }
    // The pixels at 16 bits a channel, and a row to filter with.
    let limit = (MAX_SIDE as usize).pow(2) * 8 + (MAX_SIDE as usize) * 16;
    let mut decoder = png::Decoder::new_with_limits(preview.data(), png::Limits { bytes: limit });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let (width, height) = reader.info().size();
    if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
        return None;
    }
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).ok()?;
    if frame.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let pixels = pixels.get(..frame.buffer_size())?;
    let rgba = match frame.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => (pixels.as_chunks::<3>().0.iter())
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => (pixels.as_chunks::<2>().0.iter())
            .flat_map(|&[l, a]| [l, l, l, a])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&l| [l, l, l, 255]).collect(),
        // Expanded to RGB by the transformations.
        png::ColorType::Indexed => return None,
    };
    Image::new(width, height, rgba)
}

#[cfg(test)]
mod tests;
