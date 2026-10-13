//! How the viewport's handles snap what they drag: lengths and factors
//! to round steps of their units, angles to round degrees, the steps a
//! few pixels apart on the screen. Snapping sketch points onto the
//! sketch is `snap.rs`.

use varde_expr::LengthUnit;
use varde_sketch::angle;

/// How far a handle's snapping steps are apart at least, in pixels: a
/// length's step is the roundest length in the design's units at least
/// this many pixels long, a ring's the first of [`ANGLE_STEPS`] at least
/// this many pixels along it.
pub(crate) const SNAP_PIXELS: f64 = 6.0;

/// The steps, in degrees, a ring's angle snaps to: the first at least
/// [`SNAP_PIXELS`] along the ring.
const ANGLE_STEPS: [f64; 8] = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 45.0, 90.0];

/// The step, in millimetres, a length dragged at `pixel` millimetres a
/// pixel snaps to in `units`: the roundest 1, 2 or 5 times a power of ten
/// of the units at least a few pixels long. `None` for a pixel
/// that isn't finite and above zero, or a step past the largest double.
///
/// The step is saved with the extrude it snaps, so it's made the same
/// natively and on the web: its decade by libm's `log10`, and the step
/// the nearest double to its decimal, read from its text (exact and
/// portable, where `pow(10, k)` can be an ulp off for negative `k`).
pub fn snap_step(pixel: f64, units: LengthUnit) -> Option<f64> {
    let step = round_step(pixel * SNAP_PIXELS / units.mm())?;
    Some(step * units.mm()).filter(|step| step.is_finite())
}

/// The roundest step, 1, 2 or 5 times a power of ten, at least `least`
/// long, as the nearest double to its decimal (see [`snap_step`]).
/// `None` for a `least` that isn't finite and above zero, or a step past
/// the largest double.
pub(crate) fn round_step(least: f64) -> Option<f64> {
    if !(least > 0.0 && least.is_finite()) {
        return None;
    }
    // Within ±324 for any positive finite `least`. Should the logarithm
    // round down across a power of ten, the 10 still finds that power.
    let decade = angle::log10(least).floor() as i32;
    [(1, 0), (2, 0), (5, 0), (1, 1)]
        .into_iter()
        .filter_map(|(m, up)| format!("{m}e{}", decade + up).parse::<f64>().ok())
        .find(|&step| step >= least)
        .filter(|step| step.is_finite())
}

/// The step, in degrees, a ring `radius` pixels across snaps its angle
/// to: the first of [`ANGLE_STEPS`] at least [`SNAP_PIXELS`] along it,
/// or the last.
pub(crate) fn angle_step(radius: f64) -> f64 {
    let least = (SNAP_PIXELS / radius).to_degrees();
    (ANGLE_STEPS.into_iter())
        .find(|&step| step >= least)
        .unwrap_or(ANGLE_STEPS[ANGLE_STEPS.len() - 1])
}

#[cfg(test)]
mod tests;
