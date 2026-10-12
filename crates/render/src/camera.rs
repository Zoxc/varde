use std::ops::Range;

use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Projection {
    #[default]
    Orthographic,
    Perspective,
}

/// How dragging turns the camera, see [`Camera::turn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    /// About the world's Z, and up or down as far as straight down or
    /// up: Z stays up on screen.
    #[default]
    Turntable,
    /// The same, but on over the top and under the bottom, the view
    /// upside down past them.
    OverTheTop,
    /// Free: a sideways drag turns the view about its own up, an upward
    /// one about its own right, so the view can roll and the world's Z
    /// needn't stay up on screen.
    Trackball,
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
/// [`Self::PITCH_LIMIT`] (within a half turn, turned over the top by
/// [`Rotation::OverTheTop`]), roll within a half turn either way (zero
/// but by [`Rotation::Trackball`]), the target within [`Self::EXTENT`]
/// and the distance from [`Self::MIN_DISTANCE`] to [`Self::EXTENT`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    projection: Projection,
    target: Vec3,
    /// Rotation around the world Z axis, in radians.
    yaw: f32,
    /// Elevation above the XY plane, in radians: past
    /// [`Self::PITCH_LIMIT`], up to a half turn either way, it's over the
    /// top, upside down.
    pitch: f32,
    /// How far the view is rolled about its line of sight from where yaw
    /// and pitch put it, in radians, counterclockwise on screen: up on
    /// screen turned towards the left. Zero but by [`Rotation::Trackball`];
    /// otherwise within a half turn either way.
    roll: f32,
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
            roll: 0.0,
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
    pub const EXTENT: f32 = varde_kernel_compat::MAX_COORD;
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
        self.rolled().0
    }

    /// Unit vector pointing up on screen.
    pub fn up(&self) -> Vec3 {
        self.rolled().1
    }

    /// How far the view is rolled, see [`Rotation::Trackball`]: zero
    /// unless a trackball turned it.
    pub fn roll(&self) -> f32 {
        self.roll
    }

    /// Right and up on screen: yaw and pitch's, turned by the roll.
    fn rolled(&self) -> (Vec3, Vec3) {
        let (right, up) = (right(self.yaw), up(self.yaw, self.pitch));
        if self.roll == 0.0 {
            return (right, up);
        }
        let (s, c) = self.roll.sin_cos();
        (right * c + up * s, up * c - right * s)
    }

    /// The view's orientation: what turns X, Y and Z to right, up and
    /// backward.
    fn orientation(&self) -> Quat {
        let (right, up) = self.rolled();
        Quat::from_mat3(&Mat3::from_cols(right, up, self.backward())).normalize()
    }

    /// Takes yaw, pitch and roll from `orientation`, see
    /// [`Self::orientation`], within their bounds: the pitch no further
    /// than straight down or up, rolled upside down rather than over the
    /// top. An orientation that isn't finite is ignored.
    fn set_orientation(&mut self, orientation: Quat) {
        use std::f32::consts::TAU;
        let length = orientation.length();
        if !(length.is_finite() && length > 0.0) {
            return;
        }
        let orientation = orientation / length;
        let (right, backward) = (orientation * Vec3::X, orientation * Vec3::Z);
        if !(right.is_finite() && backward.is_finite()) {
            return;
        }
        let level = backward.truncate().length();
        // atan2 rather than asin: exact to the poles.
        let pitch = backward.z.atan2(level);
        let yaw = if level > VERTICAL {
            backward.y.atan2(backward.x)
        } else {
            // Straight down or up, where only yaw and roll's sum counts:
            // all of it yaw, from where right points.
            (-right.x).atan2(right.y)
        };
        let (right0, up0) = (self::right(yaw), up(yaw, pitch));
        let roll = right.dot(up0).atan2(right.dot(right0));
        self.yaw = yaw.rem_euclid(TAU);
        self.pitch = pitch.clamp(-Self::PITCH_LIMIT, Self::PITCH_LIMIT);
        // Within rounding of none, none, so a view turned back level is
        // a turntable's again.
        self.roll = if roll.abs() < ROLL_SLACK { 0.0 } else { roll };
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

    /// Rotates the camera around its target by the given angles in radians,
    /// as a turntable: [`Self::turn`]. Non-finite angles are ignored.
    pub fn orbit(&mut self, delta_yaw: f32, delta_pitch: f32) {
        self.turn(Rotation::Turntable, delta_yaw, delta_pitch);
    }

    /// Rotates the camera around its target by the given angles in
    /// radians, as `rotation` turns it: yaw about the world's Z, pitch up
    /// or down, up to [`Self::PITCH_LIMIT`] as a turntable, on over the
    /// top otherwise. Upside down, past the top, the yaw turns the other
    /// way, so what's in view follows the drag as it does upright. A
    /// turntable's turn starts from the view upright, see
    /// [`Self::upright`], and one over the top from it unrolled, see
    /// [`Self::unroll`]. As a trackball, it turns about the view's own
    /// axes: by `delta_yaw` about up on screen, by `delta_pitch` about
    /// right (the opposite way, so the eye goes up), the two as one turn
    /// about the axis between, so the view rolls as a drag goes round.
    /// Non-finite angles are ignored, and so is a trackball turn too large
    /// to tell its axis.
    pub fn turn(&mut self, rotation: Rotation, delta_yaw: f32, delta_pitch: f32) {
        use std::f32::consts::{PI, TAU};
        if !(delta_yaw.is_finite() && delta_pitch.is_finite()) {
            return;
        }
        match rotation {
            Rotation::Turntable => {
                self.upright();
                self.yaw = (self.yaw + delta_yaw).rem_euclid(TAU);
                self.pitch =
                    (self.pitch + delta_pitch).clamp(-Self::PITCH_LIMIT, Self::PITCH_LIMIT);
            }
            Rotation::OverTheTop => {
                self.unroll();
                let delta_yaw = if self.upside_down() {
                    -delta_yaw
                } else {
                    delta_yaw
                };
                self.yaw = (self.yaw + delta_yaw).rem_euclid(TAU);
                // Within a half turn either way. Finite angles stay finite.
                self.pitch = (self.pitch + delta_pitch + PI).rem_euclid(TAU) - PI;
            }
            Rotation::Trackball => {
                if let Some(turn) = self.trackball_turn(delta_yaw, delta_pitch) {
                    self.set_orientation(turn * self.orientation());
                }
            }
        }
    }

    /// A trackball's turn by these angles, see [`Self::turn`], if it has
    /// an axis that can be told.
    fn trackball_turn(&self, delta_yaw: f32, delta_pitch: f32) -> Option<Quat> {
        let axis = self.up() * delta_yaw - self.right() * delta_pitch;
        (axis.is_finite() && axis.length().is_finite()).then(|| Quat::from_scaled_axis(axis))
    }

    /// Whether the view is upside down, turned over the top
    /// ([`Rotation::OverTheTop`]): the world's Z down on screen. Not
    /// within a thousandth of a radian of straight down or up, which rounding
    /// may take a pitch past: righting that would turn the view a half
    /// turn about its middle.
    pub fn upside_down(&self) -> bool {
        self.pitch.abs() > Self::PITCH_LIMIT + UPSIDE_DOWN_SLACK
    }

    /// Turns an upside-down or rolled view ([`Self::upside_down`],
    /// [`Self::roll`]) the right way up, the world's Z up on screen: the
    /// eye where it is, the view rolled back. Otherwise does nothing.
    pub fn upright(&mut self) {
        use std::f32::consts::{PI, TAU};
        self.roll = 0.0;
        if self.upside_down() {
            self.pitch =
                (PI.copysign(self.pitch) - self.pitch).clamp(-Self::PITCH_LIMIT, Self::PITCH_LIMIT);
            self.yaw = (self.yaw + PI).rem_euclid(TAU);
        }
    }

    /// Takes a rolled view's roll ([`Self::roll`]) away the shorter way:
    /// level if rolled less than a quarter turn, else upside down, over
    /// the top ([`Rotation::OverTheTop`]), the eye where it is either
    /// way. Otherwise does nothing.
    pub fn unroll(&mut self) {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        if self.roll == 0.0 {
            return;
        }
        if self.roll.abs() > FRAC_PI_2 {
            // The inverse of `upright`'s turn: the same flip.
            let sign = if self.pitch < 0.0 { -1.0 } else { 1.0 };
            self.pitch = PI * sign - self.pitch;
            self.yaw = (self.yaw + PI).rem_euclid(TAU);
        }
        self.roll = 0.0;
    }

    /// Rotates the camera around `pivot` by the given angles in radians,
    /// as [`Self::orbit`] does around the target: the target turns with
    /// the view, so `pivot` stays where it shows on screen. The target
    /// stays within [`Self::EXTENT`] of the origin, and a pivot or angles
    /// that aren't finite are ignored.
    pub fn orbit_about(&mut self, pivot: Vec3, delta_yaw: f32, delta_pitch: f32) {
        self.turn_about(Rotation::Turntable, pivot, delta_yaw, delta_pitch);
    }

    /// [`Self::orbit_about`], turning as `rotation` does, see
    /// [`Self::turn`].
    pub fn turn_about(
        &mut self,
        rotation: Rotation,
        pivot: Vec3,
        delta_yaw: f32,
        delta_pitch: f32,
    ) {
        if !(pivot.is_finite() && delta_yaw.is_finite() && delta_pitch.is_finite()) {
            return;
        }
        // The target from the pivot, in the view's axes, which the turn
        // keeps.
        match rotation {
            Rotation::Turntable => self.upright(),
            Rotation::OverTheTop => self.unroll(),
            Rotation::Trackball => {
                if self.trackball_turn(delta_yaw, delta_pitch).is_none() {
                    return;
                }
            }
        }
        let offset = self.target - pivot;
        let along = [self.right(), self.up(), self.backward()].map(|axis| offset.dot(axis));
        self.turn(rotation, delta_yaw, delta_pitch);
        let turned = self.right() * along[0] + self.up() * along[1] + self.backward() * along[2];
        self.set_target(pivot + turned);
    }

    /// Moves the target in the view plane. `delta` is in fractions of the
    /// viewport height, so panning tracks the cursor at the target depth.
    /// The target stays within [`Self::EXTENT`] of the origin, and a pan
    /// that isn't finite is ignored.
    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        let offset = (-self.right() * delta_x + self.up() * delta_y) * self.view_height();
        if offset.is_finite() {
            // Clamped rather than ignored if the sum overflows, so not
            // through `set_target`.
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
        self.set_target(self.target + across);
    }

    /// Looks at the target from `view`, keeping the distance. The same as
    /// [`Self::face`] with the view's normal and up: upright, unrolled.
    pub fn look_from(&mut self, view: View) {
        (self.yaw, self.pitch) = view.angles();
        self.roll = 0.0;
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
    /// say which way to turn. The view ends unrolled.
    pub fn face(&mut self, normal: Vec3, up: Vec3) {
        let Some(normal) = normal.try_normalize() else {
            return;
        };
        self.roll = 0.0;
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
    /// Projection and field of view are taken from `to`. With either end
    /// rolled ([`Rotation::Trackball`]), the view turns the shortest way
    /// from one to the other about a single axis (a slerp).
    pub fn lerp(&self, to: &Camera, t: f32) -> Camera {
        use std::f32::consts::{PI, TAU};
        if self.roll != 0.0 || to.roll != 0.0 {
            // Target, distance and the rest as unrolled, then the turn.
            let unrolled = |camera: &Camera| Camera {
                roll: 0.0,
                ..*camera
            };
            let mut camera = unrolled(self).lerp(&unrolled(to), t);
            // `Quat::slerp` goes the short way, whichever sign each has.
            camera.set_orientation(self.orientation().slerp(to.orientation(), t));
            return camera;
        }
        let yaw = (to.yaw - self.yaw + PI).rem_euclid(TAU) - PI;
        // Upside down at either end, the pitch too turns the short way
        // round, over the top if that's shorter; upright at both, it
        // turns through the side between them, never over the top.
        let over = self.upside_down() || to.upside_down();
        let pitch = if over {
            (to.pitch - self.pitch + PI).rem_euclid(TAU) - PI
        } else {
            to.pitch - self.pitch
        };
        let pitch = self.pitch + pitch * t;
        let pitch = if over {
            (pitch + PI).rem_euclid(TAU) - PI
        } else {
            pitch.clamp(-Self::PITCH_LIMIT, Self::PITCH_LIMIT)
        };
        let distance = self.distance * (to.distance / self.distance).powf(t);
        // The target moves in step with the distance, not with `t`: the
        // view then scales about the one point that stays put on screen,
        // rather than zooming in faster than it pans and swinging what it
        // turns to off the view and back. Without a zoom, it moves with `t`.
        let span = to.distance - self.distance;
        let along = if span.abs() > self.distance.min(to.distance) * 1e-3 {
            ((distance - self.distance) / span).clamp(0.0, 1.0)
        } else {
            t
        };
        Camera {
            target: self.target.lerp(to.target, along),
            yaw: self.yaw + yaw * t,
            pitch,
            distance,
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

    /// Narrows the field of view by `factor`, the eye staying put: the
    /// view at the target is `factor` times smaller in either projection,
    /// see [`Self::view_height`], so what it shows is that much larger.
    /// Below 1 it widens, to less than a half turn. A factor that isn't
    /// positive and finite is ignored.
    pub fn narrow(&mut self, factor: f32) {
        if factor > 0.0 && factor.is_finite() {
            let half = ((self.fov_y * 0.5).tan() / factor).atan();
            if half > 0.0 {
                self.fov_y = 2.0 * half;
            }
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
        self.set_target(at + (self.target - at) * scaled);
    }
}

/// How far past straight down or up, in radians, a pitch may be and
/// still not be [`Camera::upside_down`].
const UPSIDE_DOWN_SLACK: f32 = 1e-3;

/// How small a roll, in radians, [`Rotation::Trackball`]'s turns take as
/// none: within rounding of it.
const ROLL_SLACK: f32 = 1e-6;

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
