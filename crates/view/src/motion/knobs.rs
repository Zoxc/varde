//! The handles of the operations set up in the move's session that have
//! a value to drag: an offset face's, a shell's, a draft's, a chamfer's,
//! a fillet's, a scale's, an align's, a pattern's and a sweep's. The app works out where each
//! knob is and how it drags from what it knows of the model; the viewport
//! draws them as the extrude's handle (`viewport/handle.rs`) and drags
//! them (`viewport/knobs.rs`), sending [`MotionLook::DragKnob`] with the
//! value they're dragged to, which the app types into the knob's field.
//!
//! [`MotionLook::DragKnob`]: super::MotionLook::DragKnob

use glam::DVec3;

use super::MotionField;

/// A knob of an operation's handle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpKnob {
    /// The field its drag types into.
    pub field: MotionField,
    /// Where it's dragged along.
    pub path: KnobPath,
    /// Its value as the field last read, in the field's own units:
    /// millimetres, radians or a factor (signed where the app takes a
    /// side from the sign, as an offset face's inward). Where on its path
    /// it is: see [`KnobScale`].
    pub value: f64,
    /// How a value maps onto the path.
    pub scale: KnobScale,
    /// What it snaps to while dragged.
    pub snap: KnobSnap,
    /// The way its arrow points, of unit length: out of what it moves.
    pub out: DVec3,
    /// Where its shaft starts, as a value (zero, mostly: a pattern's
    /// count's from the original, 1), if it has one: drawn from there to
    /// it.
    pub shaft: Option<f64>,
    /// Its colours: its tool's icon category's.
    pub tone: KnobTone,
}

/// Where a knob drags along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KnobPath {
    /// The line through `origin` along the unit `along`.
    Line { origin: DVec3, along: DVec3 },
    /// Round the unit `axis` through `centre`, from the unit `radial`
    /// (square to it), right-handed about `axis`, `radius` out.
    Arc {
        centre: DVec3,
        axis: DVec3,
        radial: DVec3,
        radius: KnobRadius,
    },
}

/// How far out of its centre an arc is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KnobRadius {
    /// In millimetres.
    World(f64),
    /// In pixels, the same on the screen however far the camera is.
    Pixels(f64),
}

/// How a knob's value maps onto its path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KnobScale {
    /// The path's distance (millimetres along a line, radians round an
    /// arc) is the value times this.
    Times(f64),
    /// Along a line, a value of 1 is this many pixels: a slider the same
    /// size on the screen however far the camera is (a scale's factor).
    Pixels(f64),
}

/// What a knob's value snaps to while it's dragged: the roundest steps
/// at least 6 pixels apart along its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnobSnap {
    /// A length: 1, 2 or 5 × 10ⁿ of the design's units.
    Length,
    /// An angle: as a move's ring snaps, 1, 2, 5, 10, 15, 30, 45 or 90°.
    Angle,
    /// A factor: 1, 2 or 5 × 10ⁿ.
    Factor,
    /// A count: whole numbers.
    Count,
}

/// The colours of a knob: those of its tool's icon category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnobTone {
    /// Create's and Transform's (an align's): the extrude's handle's.
    Create,
    /// Modify's: red, with a blue accent.
    Modify,
    /// A count's beside another value's knob (a pattern's), a sweep's
    /// twist, or a helix's turns beside its pitch: teal, so they read
    /// apart from the operation's own colours.
    Count,
}

impl OpKnob {
    /// How far along its path the value `value` is: millimetres along a
    /// line, radians round an arc. `pixel` is a pixel's size at the
    /// line's origin, for a [`KnobScale::Pixels`] slider.
    pub(crate) fn along(&self, value: f64, pixel: f64) -> f64 {
        match self.scale {
            KnobScale::Times(times) => value * times,
            KnobScale::Pixels(pixels) => value * pixels * pixel,
        }
    }

    /// The value at `along` on its path: [`OpKnob::along`] undone.
    pub(crate) fn value_at(&self, along: f64, pixel: f64) -> f64 {
        match self.scale {
            KnobScale::Times(times) => along / times,
            KnobScale::Pixels(pixels) => along / (pixels * pixel),
        }
    }
}
