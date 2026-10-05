//! A design's thumbnail: the picture of its model the app renders as it
//! saves, once in each theme's colours, written into the file as two PNG
//! [`Preview`]s after the record (see [`vrdp`](crate::vrdp)), told apart
//! by their media type's `theme` parameter, and read back for the welcome
//! screen, which shows the one of its theme. Encoding and decoding happen
//! in the lane, off the UI thread, which only sends and takes plain
//! pixels, a [`Thumbnail`] of two [`Image`]s.
//!
//! The media types are `image/png; theme=light` and `image/png;
//! theme=dark`. Reading, a theme without its own image takes a plain
//! `image/png` (a thumbnail of one image, as saves wrote before), else
//! the other theme's; a parameter it doesn't know, or a theme it doesn't,
//! is no image of either.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::vrdp::Preview;

/// The media type thumbnails are written as, with a `theme` parameter
/// (see [`LIGHT`] and [`DARK`]).
pub const MEDIA_TYPE: &str = "image/png";

/// The media type of the light theme's image.
pub const LIGHT: &str = "image/png; theme=light";

/// The media type of the dark theme's image.
pub const DARK: &str = "image/png; theme=dark";

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

/// A design's thumbnail: its picture in the light theme's colours and in
/// the dark's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thumbnail {
    pub light: Image,
    pub dark: Image,
}

/// Its size, not its pixels.
impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({}×{})", self.width, self.height)
    }
}

/// `image` as a PNG preview of `media_type`, or `None` if it won't encode
/// or is larger than a preview may be, which is logged: the design is
/// saved without.
pub(crate) fn encode(image: &Image, media_type: &str) -> Option<Preview> {
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
    let preview = Preview::new(media_type, bytes);
    if preview.is_none() {
        log::error!("The thumbnail is too large to save");
    }
    preview
}

/// The previews a save writes: `thumbnail`'s images as PNGs, the light
/// one first, if there's one, each that encodes. Public for writing
/// designs as a save would, with [`crate::vrdp::to_bytes`].
pub fn previews(thumbnail: Option<&Thumbnail>) -> Vec<Preview> {
    let Some(thumbnail) = thumbnail else {
        return Vec::new();
    };
    [(&thumbnail.light, LIGHT), (&thumbnail.dark, DARK)]
        .into_iter()
        .filter_map(|(image, media_type)| encode(image, media_type))
        .collect()
}

/// Which theme a thumbnail's image is for, by its media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Theme {
    /// Plain `image/png`: either.
    Either,
    Light,
    Dark,
}

/// The theme `preview` is an image for, if it's a PNG whose parameters,
/// if any, are a `theme` this knows (names compared without case).
fn theme(preview: &Preview) -> Option<Theme> {
    if !preview.is(MEDIA_TYPE) {
        return None;
    }
    let mut parameters = preview.media_type().split(';').skip(1);
    let Some(parameter) = parameters.next() else {
        return Some(Theme::Either);
    };
    if parameters.next().is_some() {
        return None;
    }
    let (name, value) = parameter.split_once('=')?;
    if !name.trim().eq_ignore_ascii_case("theme") {
        return None;
    }
    let value = value.trim();
    let value = (value.strip_prefix('"'))
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    if value.eq_ignore_ascii_case("light") {
        Some(Theme::Light)
    } else if value.eq_ignore_ascii_case("dark") {
        Some(Theme::Dark)
    } else {
        None
    }
}

/// The thumbnail the whole design file `bytes` holds, if it has one that
/// decodes (see [`decode`]): what a sample design built into the app
/// shows.
pub fn of_file(bytes: &[u8]) -> Option<Thumbnail> {
    match crate::vrdp::end(bytes).ok()? {
        crate::vrdp::FileEnd::Design { previews, .. } => decode(&previews),
        crate::vrdp::FileEnd::NotADesign => None,
    }
}

/// The thumbnail `previews` hold, or `None` if they hold no image of one
/// that decodes: each theme's the first of its own that does, else a
/// plain one's, else the other theme's, see the module docs.
pub(crate) fn decode(previews: &[Preview]) -> Option<Thumbnail> {
    let first = |wanted| {
        (previews.iter())
            .filter(|preview| theme(preview) == Some(wanted))
            .find_map(decode_png)
    };
    let (light, dark, either) = (
        first(Theme::Light),
        first(Theme::Dark),
        first(Theme::Either),
    );
    let fallback = either.or_else(|| light.clone()).or_else(|| dark.clone())?;
    Some(Thumbnail {
        light: light.unwrap_or_else(|| fallback.clone()),
        dark: dark.unwrap_or(fallback),
    })
}

/// The pixels of the PNG `preview`, or `None` if it isn't one this
/// decodes: a PNG of any colour type and depth within [`MAX_SIDE`], made
/// 8 bit RGBA. A side past it is refused before its pixels are
/// allocated, and the decoder allocates at most a little more than they
/// take.
fn decode_png(preview: &Preview) -> Option<Image> {
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
