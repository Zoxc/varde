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

/// How near to along a line the cursor's ray may run and still drag a
/// handle along it ([`Projector::along_line`]), as a share of the ray's
/// length squared.
const ALONG_LINE: f64 = 1e-6;

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
        let (a, b) = self.in_front(self.placement.to_world(a), self.placement.to_world(b))?;
        Some((self.show(a), self.show(b)))
    }

    /// The part of the world segment from `a` to `b` in front of the near
    /// plane of a perspective view, all of it in an orthographic one,
    /// unless it's wholly behind it.
    pub(crate) fn in_front(&self, mut a: DVec3, mut b: DVec3) -> Option<(DVec3, DVec3)> {
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
        Some((a, b))
    }

    /// Where the world point `p` shows, as if on the near plane of a
    /// perspective view if it's nearer.
    pub(crate) fn show(&self, p: DVec3) -> DVec2 {
        let (lateral, depth) = self.view(p);
        self.screen(lateral, depth.max(self.near))
    }

    /// How far in front of the eye the world point `p` is, or of the
    /// target in an orthographic view: nearer is smaller.
    pub(crate) fn world_depth(&self, p: DVec3) -> f64 {
        self.view(p).1
    }

    /// A pixel's size at `depth` ([`Self::world_depth`]), in world units:
    /// the same at any depth in an orthographic view.
    pub(crate) fn pixel_at(&self, depth: f64) -> f64 {
        if self.perspective {
            depth.max(self.near) / (self.distance * self.scale)
        } else {
            1.0 / self.scale
        }
    }

    /// Whether it's a perspective view.
    pub(crate) fn perspective(&self) -> bool {
        self.perspective
    }

    /// How far in front of the eye a perspective view starts.
    pub(crate) fn near(&self) -> f64 {
        self.near
    }

    /// The eye, and the unit vector towards it from the target.
    pub(crate) fn eye(&self) -> (DVec3, DVec3) {
        (self.eye, self.backward)
    }

    /// The sketch point under the screen position `pixel`, and a pixel's
    /// size there. `None` where the cursor's ray misses the plane, runs
    /// along it, or meets it behind the eye or past [`MAX_COORD`].
    pub(crate) fn cursor(&self, pixel: DVec2) -> Option<Cursor> {
        let (origin, direction) = self.ray(pixel)?;
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

    /// The ray through the screen position `pixel`, as where it starts
    /// and its direction, not of unit length: from the eye in a
    /// perspective view, from the target's depth in an orthographic one.
    /// `None` for a position that isn't finite.
    pub(crate) fn ray(&self, pixel: DVec2) -> Option<(DVec3, DVec3)> {
        if !pixel.is_finite() {
            return None;
        }
        let offset = (pixel - self.center) / self.scale;
        let at_target = self.target + self.right * offset.x - self.up * offset.y;
        Some(if self.perspective {
            (self.eye, at_target - self.eye)
        } else {
            (at_target, -self.backward)
        })
    }

    /// How far along the line through `origin` along the unit `axis` the
    /// ray through the screen position `pixel` passes nearest it, from
    /// `origin`: where a handle dragged along the line goes. `None` with
    /// the ray running nearer along the line than [`ALONG_LINE`] says,
    /// where the cursor says next to nothing of how far along it is, or
    /// a position or a line that isn't finite.
    pub(crate) fn along_line(&self, origin: DVec3, axis: DVec3, pixel: DVec2) -> Option<f64> {
        let (from, ray) = self.ray(pixel)?;
        let w = from - origin;
        let (a, b) = (ray.dot(ray), ray.dot(axis));
        let (d, e) = (ray.dot(w), axis.dot(w));
        let denominator = a - b * b;
        if denominator.is_nan() || denominator <= ALONG_LINE * a {
            return None;
        }
        let t = (a * e - b * d) / denominator;
        t.is_finite().then_some(t)
    }

    /// How far in front of the eye the sketch point `at` is, or of the
    /// target in an orthographic view: nearer is smaller.
    pub(crate) fn depth(&self, at: DVec2) -> f64 {
        self.view(self.placement.to_world(at)).1
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
