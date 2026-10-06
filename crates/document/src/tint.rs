//! The colour a body is drawn in, see [`Tint`].

use serde::{Deserialize, Serialize};

/// The colour a body is drawn in in place of the theme's: a hue and a
/// saturation, the lightness staying the theme's shading's. Saturation
/// is capped at [`Tint::MAX_SATURATION`], so a body's colour stays
/// clearly duller than the selection's and the hover's highlights.
///
/// Serialized as its two whole numbers. Deserializing doesn't check their
/// ranges, as postcard would drop the message of an error raised then: a
/// document holding one does, in [`Document::check`](crate::Document::check).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tint {
    /// Degrees round the colour wheel, below 360.
    hue: u16,
    /// A whole percent, up to [`Tint::MAX_SATURATION`].
    saturation: u8,
}

impl Tint {
    /// The most saturated a body's colour may be, in percent.
    pub const MAX_SATURATION: u8 = 30;

    /// The saturation a body's colour starts at, in percent: picking a
    /// hue for a body with no colour of its own, or none saturated.
    pub const DEFAULT_SATURATION: u8 = 17;

    /// The tint of `hue` degrees and `saturation` percent, if the hue is
    /// below 360 and the saturation at most [`Tint::MAX_SATURATION`].
    pub fn new(hue: u16, saturation: u8) -> Option<Tint> {
        (hue < 360 && saturation <= Self::MAX_SATURATION).then_some(Tint { hue, saturation })
    }

    /// `hue` degrees and `saturation` percent rounded to whole ones, the
    /// hue wrapped round the wheel and the saturation clamped to the
    /// range, for input such as a slider's. NaN is 0.
    pub fn clamped(hue: f32, saturation: f32) -> Tint {
        let hue = if hue.is_finite() {
            hue.round().rem_euclid(360.0)
        } else {
            0.0
        };
        let saturation = if saturation.is_nan() {
            0.0
        } else {
            (saturation.round()).clamp(0.0, f32::from(Self::MAX_SATURATION))
        };
        // In range, so the casts are exact (a hue rounding up to 360.0 by
        // `rem_euclid` of a tiny negative is wrapped too).
        Tint {
            hue: (hue as u16) % 360,
            saturation: saturation as u8,
        }
    }

    /// Degrees round the colour wheel, from 0 to 359.
    pub fn hue(self) -> u16 {
        self.hue
    }

    /// The saturation in percent, from 0 to [`Tint::MAX_SATURATION`].
    pub fn saturation(self) -> u8 {
        self.saturation
    }

    /// Whether it's one [`Tint::new`] takes, which one deserialized may
    /// not be.
    pub(crate) fn in_range(self) -> bool {
        Self::new(self.hue, self.saturation).is_some()
    }
}

#[cfg(test)]
mod tests;
