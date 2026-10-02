use std::ops::Range;

use glam::{Mat4, Vec2, Vec3, Vec4};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Projection {
    #[default]
    Orthographic,
    Perspective,
}

/// A side to look at the model from, like the faces of a view cube.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Top,
    Bottom,
    Front,
    Back,
    Right,
    Left,
}

impl View {
    pub const ALL: [View; 6] = [
        View::Top,
        View::Bottom,
        View::Front,
        View::Back,
        View::Right,
        View::Left,
    ];

    /// Yaw and pitch in radians. Front looks along +Y, Right along -X.
    fn angles(self) -> (f32, f32) {
        use std::f32::consts::{FRAC_PI_2, PI};
        match self {
            View::Top => (-FRAC_PI_2, FRAC_PI_2),
            View::Bottom => (-FRAC_PI_2, -FRAC_PI_2),
            View::Front => (-FRAC_PI_2, 0.0),
            View::Back => (FRAC_PI_2, 0.0),
            View::Right => (0.0, 0.0),
            View::Left => (PI, 0.0),
        }
    }

    /// Unit vector from the model towards the camera.
    pub fn normal(self) -> Vec3 {
        let (yaw, pitch) = self.angles();
        // Exact, although the angles aren't.
        backward(yaw, pitch).round()
    }

    /// Unit vector pointing right on screen when looking from this view.
    pub fn right(self) -> Vec3 {
        let (yaw, _) = self.angles();
        right(yaw).round()
    }

    /// Unit vector pointing up on screen when looking from this view.
    pub fn up(self) -> Vec3 {
        let (yaw, pitch) = self.angles();
        up(yaw, pitch).round()
    }
}

/// An orbiting camera. The world is Z-up.
///
/// Both projections show the same extent at the target, so switching between
/// them keeps the model roughly the same size on screen.
///
/// The fields are private so the methods can keep the camera finite and
/// bounded, which the depth range relies on: yaw finite, pitch within
/// [`Self::PITCH_LIMIT`], the target within [`Self::EXTENT`] and the
/// distance from [`Self::MIN_DISTANCE`] to [`Self::EXTENT`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    projection: Projection,
    target: Vec3,
    /// Rotation around the world Z axis, in radians.
    yaw: f32,
    /// Elevation above the XY plane, in radians.
    pitch: f32,
    /// At least [`Self::MIN_DISTANCE`].
    distance: f32,
    /// Vertical field of view, in radians. In orthographic mode this only
    /// determines how much of the scene is visible at `distance`.
    fov_y: f32,
}

/// Looking at the origin from above the front right.
impl Default for Camera {
    fn default() -> Self {
        Self {
            projection: Projection::default(),
            target: Vec3::ZERO,
            yaw: -60f32.to_radians(),
            pitch: 30f32.to_radians(),
            distance: 9.0,
            fov_y: 45f32.to_radians(),
        }
    }
}

impl Camera {
    /// Straight down or up. The view basis comes from yaw and pitch rather
    /// than a fixed up vector, so the poles are well defined.
    pub const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2;
    /// Bound on the target's coordinates and the orbit distance, the
    /// kernel's coordinate limit, so the camera stays finite however far it
    /// is panned or zoomed.
    pub const EXTENT: f32 = varde_kernel::MAX_COORD;
    /// The closest [`Self::zoom`] brings the eye to the target.
    pub const MIN_DISTANCE: f32 = 1e-3;

    pub fn projection(&self) -> Projection {
        self.projection
    }

    pub fn set_projection(&mut self, projection: Projection) {
        self.projection = projection;
    }

    /// The point the camera orbits and looks at.
    pub fn target(&self) -> Vec3 {
        self.target
    }

    /// Moves the target, clamped within [`Self::EXTENT`] of the origin like
    /// [`Self::pan`]. A target that isn't finite is ignored.
    pub fn set_target(&mut self, target: Vec3) {
        if target.is_finite() {
            let extent = Vec3::splat(Self::EXTENT);
            self.target = target.clamp(-extent, extent);
        }
    }

    /// How far the eye is from the target.
    pub fn distance(&self) -> f32 {
        self.distance
    }

    /// Where the eye is. In orthographic mode only its direction from the
    /// target means anything: the eye is as if at infinity there.
    pub fn eye(&self) -> Vec3 {
        self.target + self.backward() * self.distance
    }

    /// How far in front of the eye the perspective projection starts
    /// drawing: what's nearer is cut off.
    pub fn near(&self) -> f32 {
        (self.distance * 0.01).max(1e-3)
    }

    /// Unit vector from the target towards the camera.
    pub fn backward(&self) -> Vec3 {
        backward(self.yaw, self.pitch)
    }

    /// Unit vector pointing right on screen.
    pub fn right(&self) -> Vec3 {
        right(self.yaw)
    }

    /// Unit vector pointing up on screen.
    pub fn up(&self) -> Vec3 {
        up(self.yaw, self.pitch)
    }

    /// Height of the visible region at the target, in world units.
    pub fn view_height(&self) -> f32 {
        2.0 * self.distance * (self.fov_y * 0.5).tan()
    }

    /// Moves the eye along its line of sight so the view is `height` tall
    /// at the target, see [`Self::view_height`], within the distance's
    /// bounds as [`Self::zoom`] keeps it. A height that isn't positive and
    /// finite is ignored.
    pub fn set_view_height(&mut self, height: f32) {
        let distance = height / (2.0 * (self.fov_y * 0.5).tan());
        if height > 0.0 && distance.is_finite() {
            self.distance = distance.clamp(Self::MIN_DISTANCE, Self::EXTENT);
        }
    }

    /// Half the visible width and height at the target, for a viewport of
    /// `aspect`.
    pub(crate) fn half_extents(&self, aspect: f32) -> Vec2 {
        let half_height = self.view_height() * 0.5;
        Vec2::new(half_height * aspect, half_height)
    }

    /// The eye in homogeneous coordinates: w = 1, or in orthographic mode
    /// the direction towards the camera with w = 0, an eye at infinity.
    pub(crate) fn eye_homogeneous(&self) -> Vec4 {
        match self.projection {
            Projection::Perspective => self.eye().extend(1.0),
            Projection::Orthographic => self.backward().extend(0.0),
        }
    }

    pub(crate) fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, self.up())
    }

    /// The projection for a viewport of `aspect`, drawing `depth`, distances
    /// in front of the eye. `aspect` must be positive and finite, like
    /// [`Viewport::aspect`]'s.
    ///
    /// [`Viewport::aspect`]: crate::Viewport::aspect
    pub(crate) fn projection_matrix(&self, aspect: f32, depth: Range<f32>) -> Mat4 {
        match self.projection {
            Projection::Perspective => {
                let near = self.near();
                Mat4::perspective_rh(self.fov_y, aspect, near, depth.end.max(near * 2.0))
            }
            Projection::Orthographic => {
                let half = self.half_extents(aspect);
                // The near plane may be behind the eye, so zooming in never
                // clips geometry.
                Mat4::orthographic_rh(-half.x, half.x, -half.y, half.y, depth.start, depth.end)
            }
        }
    }

    /// Rotates the camera around its target by the given angles in radians.
    /// Non-finite angles are ignored.
    pub fn orbit(&mut self, delta_yaw: f32, delta_pitch: f32) {
        if !(delta_yaw.is_finite() && delta_pitch.is_finite()) {
            return;
        }
        self.yaw = (self.yaw + delta_yaw).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch + delta_pitch).clamp(-Self::PITCH_LIMIT, Self::PITCH_LIMIT);
    }

    /// Rotates the camera around `pivot` by the given angles in radians,
    /// as [`Self::orbit`] does around the target: the target turns with
    /// the view, so `pivot` stays where it shows on screen. The target
    /// stays within [`Self::EXTENT`] of the origin, and a pivot or angles
    /// that aren't finite are ignored.
    pub fn orbit_about(&mut self, pivot: Vec3, delta_yaw: f32, delta_pitch: f32) {
        if !(pivot.is_finite() && delta_yaw.is_finite() && delta_pitch.is_finite()) {
            return;
        }
        // The target from the pivot, in the view's axes, which the turn
        // keeps.
        let offset = self.target - pivot;
        let along = [self.right(), self.up(), self.backward()].map(|axis| offset.dot(axis));
        self.orbit(delta_yaw, delta_pitch);
        let turned = self.right() * along[0] + self.up() * along[1] + self.backward() * along[2];
        let target = pivot + turned;
        if target.is_finite() {
            let extent = Vec3::splat(Self::EXTENT);
            self.target = target.clamp(-extent, extent);
        }
    }

    /// Moves the target in the view plane. `delta` is in fractions of the
    /// viewport height, so panning tracks the cursor at the target depth.
    /// The target stays within [`Self::EXTENT`] of the origin, and a pan
    /// that isn't finite is ignored.
    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        let offset = (-self.right() * delta_x + self.up() * delta_y) * self.view_height();
        if offset.is_finite() {
            // Clamped rather than ignored if the sum overflows.
            let extent = Vec3::splat(Self::EXTENT);
            self.target = (self.target + offset).clamp(-extent, extent);
        }
    }

    /// Pans so `point` shows in the middle of the view: the target moves
    /// across the view, not along it, so the zoom and, in perspective, how
    /// far the eye is from what it sees stay. The target stays within
    /// [`Self::EXTENT`] of the origin, and a point that isn't finite is
    /// ignored.
    pub fn center_on(&mut self, point: Vec3) {
        let offset = point - self.target;
        let across = self.right() * offset.dot(self.right()) + self.up() * offset.dot(self.up());
        let target = self.target + across;
        if target.is_finite() {
            let extent = Vec3::splat(Self::EXTENT);
            self.target = target.clamp(-extent, extent);
        }
    }

    /// Looks at the target from `view`, keeping the distance. The same as
    /// [`Self::face`] with the view's normal and up.
    pub fn look_from(&mut self, view: View) {
        (self.yaw, self.pitch) = view.angles();
    }

    /// Looks straight at a plane whose `normal` points towards the camera,
    /// keeping the target and the distance, with `up` pointing up on screen
    /// as far as the camera can: it orbits without rolling, so up on
    /// screen is the world's Z as seen from `normal`, unless `normal` is
    /// vertical, when `up` chooses the way the camera turns.
    ///
    /// That's enough for the origin planes, seen from their normals with
    /// their y axis up, as their placements in the document are. A normal
    /// that's zero or not finite is ignored, and so is an `up` that doesn't
    /// say which way to turn.
    pub fn face(&mut self, normal: Vec3, up: Vec3) {
        let Some(normal) = normal.try_normalize() else {
            return;
        };
        if normal.truncate().length() > VERTICAL {
            self.yaw = normal.y.atan2(normal.x);
            self.pitch = normal.z.clamp(-1.0, 1.0).asin();
            return;
        }
        // Straight down or up, where up on screen is `-sin(pitch)` times
        // the horizontal direction `yaw` points to.
        let pitch = Self::PITCH_LIMIT.copysign(normal.z);
        if let Some(turn) = (-up.truncate() * pitch.signum()).try_normalize() {
            self.yaw = turn.y.atan2(turn.x);
        }
        self.pitch = pitch;
    }

    /// The camera a fraction `t` of the way from `self` to `to`, turning the
    /// short way round. Distance changes geometrically so zoom feels even.
    /// Projection and field of view are taken from `to`.
    pub fn lerp(&self, to: &Camera, t: f32) -> Camera {
        use std::f32::consts::{PI, TAU};
        let yaw = (to.yaw - self.yaw + PI).rem_euclid(TAU) - PI;
        Camera {
            target: self.target.lerp(to.target, t),
            yaw: self.yaw + yaw * t,
            pitch: self.pitch + (to.pitch - self.pitch) * t,
            distance: self.distance * (to.distance / self.distance).powf(t),
            ..*to
        }
    }

    /// Multiplies the orbit distance, e.g. `0.9` to move 10% closer. A `NaN`
    /// factor is ignored.
    pub fn zoom(&mut self, factor: f32) {
        if !factor.is_nan() {
            self.distance = (self.distance * factor).clamp(Self::MIN_DISTANCE, Self::EXTENT);
        }
    }

    /// Zooms by `factor`, as [`Self::zoom`] does, towards the point at
    /// the target's depth that shows `x` right and `y` down of the view's
    /// middle, in fractions of the viewport height like [`Self::pan`]'s:
    /// it stays where it shows, so the zoom follows the cursor. The target
    /// stays within [`Self::EXTENT`] of the origin, and a factor or
    /// offset that isn't finite is ignored.
    pub fn zoom_at(&mut self, factor: f32, x: f32, y: f32) {
        if !(factor.is_finite() && x.is_finite() && y.is_finite()) {
            return;
        }
        let at = self.target + (self.right() * x - self.up() * y) * self.view_height();
        let distance = self.distance;
        self.zoom(factor);
        // What the distance was multiplied by, once clamped: the target
        // moves towards `at` by the same proportion, so the view's scale
        // about it changes as it does about the target.
        let scaled = self.distance / distance;
        let target = at + (self.target - at) * scaled;
        if target.is_finite() {
            let extent = Vec3::splat(Self::EXTENT);
            self.target = target.clamp(-extent, extent);
        }
    }
}

/// How far from vertical, as the length of its horizontal part, a unit
/// normal [`Camera::face`] turns to may be and still count as straight up
/// or down: within rounding of it.
const VERTICAL: f32 = 1e-6;

/// Unit vector towards a camera at `yaw` and `pitch`, from its target.
fn backward(yaw: f32, pitch: f32) -> Vec3 {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    Vec3::new(cp * cy, cp * sy, sp)
}

/// Unit vector right on screen for a camera at `yaw`.
fn right(yaw: f32) -> Vec3 {
    let (sy, cy) = yaw.sin_cos();
    Vec3::new(-sy, cy, 0.0)
}

/// Unit vector up on screen for a camera at `yaw` and `pitch`.
fn up(yaw: f32, pitch: f32) -> Vec3 {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    Vec3::new(-sp * cy, -sp * sy, cp)
}

#[cfg(test)]
mod tests;
