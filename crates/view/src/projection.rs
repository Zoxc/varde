//! Between a sketch's plane and the viewport: where a sketch point shows,
//! and which sketch point is under the cursor, for the input on the sketch
//! being edited, hit testing and the widgets anchored to it. Matches what
//! the renderer draws for the same camera.

use glam::{DVec2, DVec3};
use varde_document::{MAX_COORD, Placement};
use varde_render::{Camera, Projection};

/// How far from along the plane, as the cosine of the angle between a
/// cursor's ray and the plane's normal, the ray has to be to meet the
/// plane: nearer to along it, it meets it far off if at all, and the
/// camera's axes, in `f32`, aren't exact enough to say where.
const PARALLEL: f64 = 1e-6;

/// Maps between a sketch's plane and a viewport of a given size seen by a
/// camera. Screen positions are in logical pixels from the viewport's top
/// left, y down.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Projector {
    placement: Placement,
    perspective: bool,
    target: DVec3,
    eye: DVec3,
    right: DVec3,
    up: DVec3,
    backward: DVec3,
    distance: f64,
    /// How far in front of the eye a perspective view starts, see
    /// [`Camera::near`].
    near: f64,
    /// Pixels per world unit at the target's depth.
    scale: f64,
    /// The viewport's centre, where the target shows.
    center: DVec2,
}

/// The sketch point under the cursor, and how big a pixel is there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cursor {
    /// In sketch coordinates, within [`MAX_COORD`] of zero.
    pub(crate) at: DVec2,
    /// A pixel's size at `at`, in sketch units: above zero and at most
    /// [`MAX_COORD`].
    pub(crate) pixel: f64,
}

impl Projector {
    /// The projector for a sketch on `placement` seen by `camera` in a
    /// viewport `width` by `height` logical pixels. `None` for a viewport
    /// too small to show anything.
    pub(crate) fn new(
        camera: &Camera,
        placement: Placement,
        width: f32,
        height: f32,
    ) -> Option<Self> {
        if !(width >= 1.0 && height >= 1.0 && width.is_finite() && height.is_finite()) {
            return None;
        }
        // The camera keeps itself finite, with a distance above zero, so
        // its view height is too.
        let scale = f64::from(height) / f64::from(camera.view_height());
        let projector = Self {
            placement,
            perspective: camera.projection() == Projection::Perspective,
            target: camera.target().as_dvec3(),
            eye: camera.eye().as_dvec3(),
            right: camera.right().as_dvec3(),
            up: camera.up().as_dvec3(),
            backward: camera.backward().as_dvec3(),
            distance: f64::from(camera.distance()),
            near: f64::from(camera.near()),
            scale,
            center: DVec2::new(f64::from(width), f64::from(height)) / 2.0,
        };
        scale.is_finite().then_some(projector)
    }

    /// A pixel's size at the target's depth, in sketch units: what a few
    /// pixels come to where the view looks, the same wherever the cursor
    /// is. Above zero and finite.
    pub(crate) fn pixel(&self) -> f64 {
        1.0 / self.scale
    }

    /// Where the sketch point `at` shows, unless it's behind the near
    /// plane of a perspective view.
    pub(crate) fn project(&self, at: DVec2) -> Option<DVec2> {
        let (lateral, depth) = self.view(self.placement.to_world(at));
        (!self.perspective || depth >= self.near).then(|| self.screen(lateral, depth))
    }

    /// Where the segment from sketch points `a` to `b` shows, cut where it
    /// passes behind the near plane of a perspective view, unless it's
    /// wholly behind it.
    pub(crate) fn segment(&self, a: DVec2, b: DVec2) -> Option<(DVec2, DVec2)> {
        let (mut a, mut b) = (self.placement.to_world(a), self.placement.to_world(b));
        if self.perspective {
            let (da, db) = (self.view(a).1, self.view(b).1);
            let near = self.near;
            if da < near && db < near {
                return None;
            }
            // Only one is behind, so the depths differ.
            if da < near {
                a += (b - a) * ((near - da) / (db - da));
            } else if db < near {
                b += (a - b) * ((near - db) / (da - db));
            }
        }
        let screen = |p| {
            let (lateral, depth) = self.view(p);
            self.screen(lateral, depth.max(self.near))
        };
        Some((screen(a), screen(b)))
    }

    /// The sketch point under the screen position `pixel`, and a pixel's
    /// size there. `None` where the cursor's ray misses the plane, runs
    /// along it, or meets it behind the eye or past [`MAX_COORD`].
    pub(crate) fn cursor(&self, pixel: DVec2) -> Option<Cursor> {
        if !pixel.is_finite() {
            return None;
        }
        let offset = (pixel - self.center) / self.scale;
        let at_target = self.target + self.right * offset.x - self.up * offset.y;
        let (origin, direction) = if self.perspective {
            (self.eye, at_target - self.eye)
        } else {
            (at_target, -self.backward)
        };
        let Placement {
            origin: plane,
            x,
            y,
            normal,
        } = self.placement;
        let facing = direction.dot(normal);
        if facing.abs() <= PARALLEL * direction.length() {
            return None;
        }
        let t = (plane - origin).dot(normal) / facing;
        // An orthographic view sees what's behind its eye too.
        if self.perspective && t <= 0.0 {
            return None;
        }
        let hit = origin + direction * t;
        let at = DVec2::new((hit - plane).dot(x), (hit - plane).dot(y));
        if !(at.is_finite() && at.abs().max_element() <= f64::from(MAX_COORD)) {
            return None;
        }
        let pixel = if self.perspective {
            self.view(hit).1 / (self.distance * self.scale)
        } else {
            1.0 / self.scale
        };
        let pixel = pixel.min(f64::from(MAX_COORD));
        (pixel > 0.0).then_some(Cursor { at, pixel })
    }

    /// The world point `p` in the view: across the screen (right, up) and
    /// how far in front of the eye it is, or of the target in an
    /// orthographic view, where it doesn't change what's shown.
    fn view(&self, p: DVec3) -> (DVec2, f64) {
        let from = if self.perspective {
            self.eye
        } else {
            self.target
        };
        let d = p - from;
        (
            DVec2::new(d.dot(self.right), d.dot(self.up)),
            -d.dot(self.backward),
        )
    }

    /// The screen position of what's `lateral` across the view at `depth`.
    fn screen(&self, lateral: DVec2, depth: f64) -> DVec2 {
        let scale = if self.perspective {
            self.scale * self.distance / depth
        } else {
            self.scale
        };
        self.center + DVec2::new(lateral.x, -lateral.y) * scale
    }
}

/// Looking straight down at the XY plane with the origin in the middle,
/// 20 units across the view's height: a unit is 10 pixels in a viewport
/// 200 pixels tall. What the tests of drawing and hit testing look through.
#[cfg(test)]
pub(crate) fn top_camera() -> Camera {
    let mut camera = Camera::default();
    camera.look_from(varde_render::View::Top);
    camera.set_target(glam::Vec3::ZERO);
    camera.zoom(20.0 / camera.view_height());
    camera
}

#[cfg(test)]
mod tests;
