//! Turning a document's camera to a new view, and the point it orbits.

use std::time::Duration;

use glam::Vec3;
use iced::time::Instant;
use varde_render::Camera;

use varde_expr::LengthUnit;

use super::{Doc, home_camera};

/// How long the camera takes to turn to a new view, every turn alike, as
/// the UI mock's `CAM_MS`.
pub(crate) const CAMERA_ANIMATION: Duration = Duration::from_millis(325);

/// How long the pivot's marker shows once picked before it fades, and how
/// long it takes to fade, then and when the cursor leaves the view cube.
pub(crate) const PIVOT_SHOWN: Duration = Duration::from_secs(2);
pub(crate) const PIVOT_FADE: Duration = Duration::from_millis(400);

/// How much room the camera leaves around what it frames: the view is
/// this many times as tall as it (a sketch's points on entering it, a
/// failure's box's diagonal on Show).
pub(super) const FRAME_MARGIN: f32 = 1.5;

/// A length that fits the view of `camera`, in millimetres, for a fresh
/// default of a tool in a design of `units`: `share` (positive, up to
/// one) of the view's height at the target, kept within that share of
/// [`MAX_COORD`](varde_kernel::MAX_COORD), rounded down to 1, 2 or 5
/// times a power of ten of `units`, and no less than a thousandth of
/// one. Tools pick their defaults with it so what they make fits the view
/// as it is: the camera never moves for them. Made as
/// [`snap_step`](varde_view::snap_step) makes its step, so it's the same
/// natively and on the web.
pub(crate) fn fitting_length(camera: &Camera, units: LengthUnit, share: f64) -> f64 {
    let height = f64::from(camera.view_height()).min(f64::from(varde_kernel::MAX_COORD));
    let most = height * share / units.mm();
    // The view's height is positive and bounded, so the decade is within
    // ±324. Should the logarithm round down across a power of ten, the 10
    // still finds that power.
    let decade = (varde_sketch::angle::log10(most).floor() as i32).max(-3);
    let length = [(1, 1), (5, 0), (2, 0), (1, 0)]
        .into_iter()
        .filter_map(|(m, up)| format!("{m}e{}", decade + up).parse::<f64>().ok())
        .find(|&length| length <= most)
        .unwrap_or(0.001);
    length * units.mm()
}

/// The point picked for the camera to orbit, with a middle click, and how
/// its marker shows: for [`PIVOT_SHOWN`] once picked, then fading, and
/// while the cursor is over the view cube.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pivot {
    pub(crate) at: Vec3,
    /// When the marker last showed whole without the cursor over the
    /// cube: when it was picked, or [`PIVOT_SHOWN`] before the cursor
    /// left the cube, so it fades from then.
    shown: Instant,
    /// How opaque the marker is, from 0 to 1, as of the last frame.
    pub(crate) opacity: f32,
}

impl Pivot {
    /// How opaque the marker is at `now`, the cursor off the cube.
    fn opacity_at(&self, now: Instant) -> f32 {
        let fading = now
            .saturating_duration_since(self.shown)
            .saturating_sub(PIVOT_SHOWN);
        (1.0 - fading.as_secs_f32() / PIVOT_FADE.as_secs_f32()).clamp(0.0, 1.0)
    }
}

/// The camera moving to a new view, eased out.
pub(crate) struct CameraAnimation {
    pub(crate) from: Camera,
    pub(crate) to: Camera,
    pub(crate) start: Instant,
}

impl CameraAnimation {
    /// The camera at `now`, or `None` once the animation is over.
    pub(crate) fn at(&self, now: Instant) -> Option<Camera> {
        let t = now.saturating_duration_since(self.start).as_secs_f32()
            / CAMERA_ANIMATION.as_secs_f32();
        (t < 1.0).then(|| {
            let eased = 1.0 - (1.0 - t).powi(3);
            self.from.lerp(&self.to, eased)
        })
    }
}

/// The least height, in millimetres, Home frames a model to: a tiny one is
/// shown with what's around it rather than filling the view.
const MIN_HOME_HEIGHT: f32 = 1.0;

impl Doc {
    /// The camera Home turns to outside a sketch: from where Home looks,
    /// framing the model shown, its bodies and its sketches, or on the
    /// origin if it shows nothing.
    pub(super) fn home_view(&self) -> Camera {
        let mut camera = home_camera(self.camera.projection());
        let bounds = [self.feed.mesh().bounds(), self.feed.sketches().bounds()]
            .into_iter()
            .flatten()
            .reduce(|a, b| varde_kernel::Aabb {
                min: a.min.min(b.min),
                max: a.max.max(b.max),
            });
        if let Some(bounds) = bounds {
            // The mesh and lines keep their positions finite and bounded,
            // and the camera clamps what it's given.
            camera.set_target(bounds.center());
            let diagonal = (bounds.max - bounds.min).length();
            camera.set_view_height((diagonal * FRAME_MARGIN).max(MIN_HOME_HEIGHT));
        }
        camera
    }

    /// Frames the model once the first one of a document opened shows,
    /// unless the camera was moved meanwhile: the camera starts out on
    /// the origin, a model away from it would open off the view.
    pub(super) fn fit_first_model(&mut self) {
        if !self.fit_on_model || self.feed.generation().is_none() {
            return;
        }
        self.fit_on_model = false;
        if self.animation.is_none() && self.camera == home_camera(self.camera.projection()) {
            self.camera = self.home_view();
        }
    }

    pub(crate) fn animate_camera(&mut self, to: Camera) {
        self.animation = Some(CameraAnimation {
            from: self.camera,
            to,
            start: Instant::now(),
        });
    }

    /// Orbits the camera by these angles about the pivot, or its target if
    /// none was picked.
    pub(crate) fn orbit(&mut self, yaw: f32, pitch: f32) {
        self.animation = None;
        match self.pivot {
            Some(pivot) => self.camera.orbit_about(pivot.at, yaw, pitch),
            None => self.camera.orbit(yaw, pitch),
        }
    }

    /// Orbits the camera about `at` from now on, marking it and panning
    /// to bring it to the middle of the view, or about its target if
    /// `None`.
    pub(crate) fn set_pivot(&mut self, at: Option<Vec3>, now: Instant) {
        if let Some(at) = at {
            let mut to = self
                .animation
                .as_ref()
                .map_or(self.camera, |animation| animation.to);
            to.center_on(at);
            self.animation = Some(CameraAnimation {
                from: self.camera,
                to,
                start: now,
            });
        }
        self.pivot = at.map(|at| Pivot {
            at,
            shown: now,
            opacity: 1.0,
        });
    }

    /// The cursor over the view cube, or off it, at `now`: the pivot's
    /// marker shows while it's over, and fades once it's off.
    pub(crate) fn hover_cube(&mut self, over: bool, now: Instant) {
        if self.cube_hovered == over {
            return;
        }
        self.cube_hovered = over;
        if let Some(pivot) = &mut self.pivot {
            if over {
                pivot.opacity = 1.0;
            } else {
                pivot.shown = now.checked_sub(PIVOT_SHOWN).unwrap_or(now);
            }
        }
    }

    /// The pivot's marker, if it shows.
    pub(crate) fn pivot_marker(&self) -> Option<varde_render::Pivot> {
        let pivot = self.pivot.filter(|pivot| pivot.opacity > 0.0)?;
        Some(varde_render::Pivot {
            at: pivot.at,
            opacity: pivot.opacity,
        })
    }

    /// Whether the pivot's marker is fading, which takes frames.
    pub(crate) fn pivot_fading(&self) -> bool {
        !self.cube_hovered && self.pivot.is_some_and(|pivot| pivot.opacity > 0.0)
    }

    /// Moves the camera along its animation to where it is at `now`, and
    /// fades the pivot's marker.
    pub(crate) fn animation_frame(&mut self, now: Instant) {
        if !self.cube_hovered
            && let Some(pivot) = &mut self.pivot
        {
            pivot.opacity = pivot.opacity_at(now);
        }
        if let Some(animation) = self.animation.take() {
            match animation.at(now) {
                Some(camera) => {
                    self.camera = camera;
                    self.animation = Some(animation);
                }
                None => self.camera = animation.to,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default camera made `height` mm tall at its target.
    fn holding(height: f32) -> Camera {
        let mut camera = Camera::default();
        camera.set_view_height(height);
        camera
    }

    #[test]
    fn a_fitting_length_is_a_round_share_of_the_view_in_its_units() {
        let mm = |height, share| fitting_length(&holding(height), LengthUnit::Mm, share);
        assert_eq!(mm(100.0, 0.25), 20.0);
        assert_eq!(mm(100.0, 0.02), 2.0);
        assert_eq!(mm(40.0, 0.25), 10.0);
        assert_eq!(mm(30.0, 0.25), 5.0);
        // In inches, a round number of them: a 100 mm view's quarter is
        // just under an inch, so half of one.
        let inches = fitting_length(&holding(100.0), LengthUnit::In, 0.25);
        assert!((inches - 0.5 * 25.4).abs() < 1e-9);
        let metres = fitting_length(&holding(10_000.0), LengthUnit::M, 0.25);
        assert!((metres - 2000.0).abs() < 1e-9);
    }

    #[test]
    fn a_fitting_length_stays_bounded_at_the_zoom_limits() {
        for unit in LengthUnit::ALL {
            // Zoomed in as far as the camera goes: a thousandth of a unit.
            let tiny = fitting_length(&holding(1e-6), unit, 0.01);
            assert!((tiny - 0.001 * unit.mm()).abs() < 1e-12);
            // Zoomed out past the coordinate limit: within its share.
            let huge = fitting_length(&holding(f32::MAX), unit, 0.25);
            assert!(huge.is_finite() && huge > 0.0);
            assert!(huge <= 0.25 * f64::from(varde_kernel::MAX_COORD));
        }
    }
}
