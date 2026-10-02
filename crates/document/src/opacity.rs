//! How opaque a body is drawn, see [`Opacity`].

use std::fmt;

use serde::{Deserialize, Serialize};

/// How opaque a body is drawn: a whole percent from [`Opacity::MIN`] to
/// [`Opacity::MAX`], opaque by default. It never reaches 0: hiding a body
/// is for making it invisible.
///
/// It's serialized as the percent, a `u8`. Deserializing doesn't check
/// the range, as postcard would drop the message of an error raised then:
/// a document holding one does, in [`Document::check`](crate::Document::check),
/// so one read from a file is in range too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Opacity(u8);

impl Opacity {
    /// The faintest a body can be drawn.
    pub const MIN: Opacity = Opacity(10);
    /// Opaque.
    pub const MAX: Opacity = Opacity(100);

    /// `percent` as an opacity, if it's from [`MIN`](Opacity::MIN) to
    /// [`MAX`](Opacity::MAX).
    pub fn new(percent: u8) -> Option<Opacity> {
        (Self::MIN.0..=Self::MAX.0)
            .contains(&percent)
            .then_some(Opacity(percent))
    }

    /// `percent` rounded to a whole one and clamped to the range, for
    /// input such as a slider's. NaN is opaque.
    pub fn clamped(percent: f32) -> Opacity {
        if percent.is_nan() {
            return Self::MAX;
        }
        let clamped = percent
            .round()
            .clamp(f32::from(Self::MIN.0), f32::from(Self::MAX.0));
        // In range, so the cast is exact.
        Opacity(clamped as u8)
    }

    /// The whole percent, from 10 to 100.
    pub fn percent(self) -> u8 {
        self.0
    }

    /// The opacity as an alpha, from 0.1 to 1.
    pub fn alpha(self) -> f32 {
        f32::from(self.0) / 100.0
    }

    pub fn is_opaque(self) -> bool {
        self == Self::MAX
    }

    /// Whether the opacity is one [`Opacity::new`] takes, which one
    /// deserialized may not be.
    pub(crate) fn in_range(self) -> bool {
        Self::new(self.0).is_some()
    }
}

impl Default for Opacity {
    fn default() -> Self {
        Self::MAX
    }
}

impl fmt::Display for Opacity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} %", self.0)
    }
}

#[cfg(test)]
mod tests;
