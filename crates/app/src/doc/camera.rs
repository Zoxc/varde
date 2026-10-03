//! Turning a document's camera to a new view, and the point it orbits.

use std::time::Duration;

use glam::Vec3;
use iced::time::Instant;
use varde_render::Camera;

use super::Doc;

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

impl Doc {
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
