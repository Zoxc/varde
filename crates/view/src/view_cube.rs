//! The view cube: a small cube in the viewport corner that turns with the
//! camera. Clicking a face looks at the model from that side. The X, Y and
//! Z axes run along edges of it in sight and past it, in the scene's axis
//! colours, each lettered at its tip.

use glam::{Vec2, Vec3};
use iced::alignment::Vertical;
use iced::widget::canvas::{self, Action, Frame, Geometry, LineCap, LineJoin, Path, Stroke, Text};
use iced::widget::text::Alignment;
use iced::{Color, Element, Event, Font, Length, Point, Radians, Rectangle, Renderer, Theme, font};
use iced::{Vector, mouse};
use varde_render::{Camera, Srgb, View};

use crate::theme::{self, Palette};
use crate::{Look, Message};

/// Width and height of the widget: the cube, and room around it for the
/// axes' tips and letters.
const SIZE: f32 = 116.0;
/// Half the cube's edge length.
const HALF: f32 = 25.0;
const LETTER_SIZE: f32 = 17.0;
/// How far the axes run past the cube.
const AXIS_OVERHANG: f32 = 12.0;
const AXIS_WIDTH: f32 = 2.0;
/// The arrowheads at the axes' tips: their length and half their width.
const ARROW_LENGTH: f32 = 6.0;
const ARROW_HALF_WIDTH: f32 = 3.0;
/// The axes' letters: how tall, and their strokes' width.
const AXIS_LETTER_SIZE: f32 = 8.0;
const LETTER_WIDTH: f32 = 1.6;
/// How far past an axis's tip, on screen, its letter's centre is.
const AXIS_LETTER_GAP: f32 = 8.0;
/// How close to the widget's sides a letter's centre may come.
const AXIS_LETTER_MARGIN: f32 = 6.0;

pub fn view_cube<'a>(camera: &Camera) -> Element<'a, Message> {
    canvas::Canvas::new(ViewCube {
        basis: Basis::new(camera),
    })
    .width(Length::Fixed(SIZE))
    .height(Length::Fixed(SIZE))
    .into()
}

/// The camera's screen axes in world space.
#[derive(Debug, Clone, Copy)]
struct Basis {
    right: Vec3,
    up: Vec3,
    backward: Vec3,
}

impl Basis {
    fn new(camera: &Camera) -> Self {
        Self {
            right: camera.right(),
            up: camera.up(),
            backward: camera.backward(),
        }
    }

    /// Projects a direction onto the screen, y down.
    fn project(&self, v: Vec3) -> Vec2 {
        Vec2::new(v.dot(self.right), -v.dot(self.up))
    }

    /// Projects a point relative to the cube centre into widget coordinates.
    fn point(&self, v: Vec3) -> Vec2 {
        self.project(v) + Vec2::splat(SIZE / 2.0)
    }
}

/// A face as drawn: its view, letter and corners in widget coordinates.
struct Face {
    view: View,
    letter: &'static str,
    corners: [Vec2; 4],
    center: Vec2,
    /// Screen images of the face's letter axes, for drawing it in the face plane.
    u: Vec2,
    v: Vec2,
}

fn letter(view: View) -> &'static str {
    match view {
        View::Top => "T",
        View::Bottom => "U",
        View::Front => "F",
        View::Back => "B",
        View::Right => "R",
        View::Left => "L",
    }
}

/// The faces turned towards the camera. They never overlap on screen.
fn visible_faces(basis: &Basis) -> impl Iterator<Item = Face> + '_ {
    View::ALL
        .into_iter()
        .filter(|view| view.normal().dot(basis.backward) > 1e-3)
        .map(|view| {
            // In-plane axes, with `v` pointing down the letter, so it's
            // upright when the face is seen head on.
            let (u, v) = (view.right(), -view.up());
            let c = view.normal() * HALF;
            let corner = |a: f32, b: f32| basis.point(c + (u * a + v * b) * HALF);
            Face {
                view,
                letter: letter(view),
                corners: [
                    corner(-1.0, -1.0),
                    corner(1.0, -1.0),
                    corner(1.0, 1.0),
                    corner(-1.0, 1.0),
                ],
                center: basis.point(c),
                u: basis.project(u),
                v: basis.project(v),
            }
        })
}

/// Whether `p` is inside the convex quad, whichever way it winds.
fn contains(corners: &[Vec2; 4], p: Vec2) -> bool {
    let sides = (0..4).map(|i| {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        (b - a).perp_dot(p - a)
    });
    let (mut pos, mut neg) = (false, false);
    for side in sides {
        pos |= side > 0.0;
        neg |= side < 0.0;
    }
    !(pos && neg)
}

fn face_at(basis: &Basis, p: Vec2) -> Option<View> {
    visible_faces(basis)
        .find(|face| contains(&face.corners, p))
        .map(|face| face.view)
}

/// Splits a 2×2 matrix with columns `x` and `y` into rotate, scale, rotate:
/// `M = R(phi) · diag(sx, sy) · R(theta)`. Canvas frames have no shear, so
/// this is how a letter is drawn into a face seen at an angle.
fn decompose(x: Vec2, y: Vec2) -> (f32, Vec2, f32) {
    let e = (x.x + y.y) / 2.0;
    let f = (x.x - y.y) / 2.0;
    let g = (x.y + y.x) / 2.0;
    let h = (x.y - y.x) / 2.0;
    let q = e.hypot(h);
    let r = f.hypot(g);
    let a1 = g.atan2(f);
    let a2 = h.atan2(e);
    ((a2 + a1) / 2.0, Vec2::new(q + r, q - r), (a2 - a1) / 2.0)
}

/// Lightness of a face, from the mock's key light.
fn fill(palette: &Palette, basis: &Basis, normal: Vec3) -> Color {
    let light = (-0.45 * basis.right + 0.7 * basis.up + 0.6 * basis.backward).normalize();
    let i = normal.dot(light).max(0.0);
    let (a, b) = (palette.cube_shade, palette.cube_lit);
    Color::from_rgb(
        a.r + (b.r - a.r) * i,
        a.g + (b.g - a.g) * i,
        a.b + (b.b - a.b) * i,
    )
}

/// The world's X, Y and Z directions.
const WORLD_AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

/// An axis as drawn: along an edge of the cube parallel to it, from the
/// edge's start to its end, then on past the cube to its tip, in widget
/// coordinates, with where its letter goes.
#[derive(Debug, Clone, Copy)]
struct Axis {
    /// 0, 1 or 2 for X, Y or Z.
    index: usize,
    start: Vec2,
    end: Vec2,
    tip: Vec2,
    /// Where the axis points on screen, of unit length.
    direction: Vec2,
    letter: Vec2,
    /// Whether the cube hides none of the part past it, which is then
    /// drawn over the faces; otherwise under them, which hide what's
    /// behind them.
    tip_shown: bool,
}

/// Whether the cube hides `p`, a point on or outside it: the ray from it
/// towards the camera passes through the cube's inside.
fn hidden(basis: &Basis, p: Vec3) -> bool {
    let inside = HALF * (1.0 - 1e-3);
    let (mut near, mut far) = (1e-3_f32, f32::INFINITY);
    for i in 0..3 {
        let (o, d) = (p[i], basis.backward[i]);
        if d.abs() < 1e-9 {
            if o.abs() >= inside {
                return false;
            }
            continue;
        }
        let (a, b) = ((-inside - o) / d, (inside - o) / d);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    near < far
}

/// The X, Y and Z axes as the cube is turned, each along one of the four
/// edges parallel to it: of those on a face turned to the camera, one
/// whose part past the cube nothing hides if there is one, and of those
/// the one lowest and furthest left on screen, so the axes gather at the
/// cube's bottom left like a triad. None for an axis pointing straight at
/// the camera or away, which no edge in sight runs along.
fn axes(basis: &Basis) -> [Option<Axis>; 3] {
    let facing = |normal: Vec3| normal.dot(basis.backward) > 1e-3;
    std::array::from_fn(|index| {
        let along = WORLD_AXES[index];
        let projected = basis.project(along);
        if projected.length() < 0.05 {
            return None;
        }
        let direction = projected.normalize();
        let (u, v) = (WORLD_AXES[(index + 1) % 3], WORLD_AXES[(index + 2) % 3]);
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .into_iter()
            .filter(|&(a, b)| facing(u * a) || facing(v * b))
            .map(|(a, b)| {
                let start = (u * a + v * b - along) * HALF;
                let end = start + along * 2.0 * HALF;
                let tip = end + along * AXIS_OVERHANG;
                let tip_shown =
                    (1..=8).all(|i| !hidden(basis, end + (tip - end) * (i as f32 / 8.0)));
                let (start, end, tip) = (basis.point(start), basis.point(end), basis.point(tip));
                let letter = (tip + direction * AXIS_LETTER_GAP).clamp(
                    Vec2::splat(AXIS_LETTER_MARGIN),
                    Vec2::splat(SIZE - AXIS_LETTER_MARGIN),
                );
                Axis {
                    index,
                    start,
                    end,
                    tip,
                    direction,
                    letter,
                    tip_shown,
                }
            })
            .max_by(|a, b| {
                let score = |axis: &Axis| {
                    let middle = (axis.start + axis.end) / 2.0;
                    (axis.tip_shown, middle.y - middle.x)
                };
                let (a, b) = (score(a), score(b));
                a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
            })
    })
}

/// The colour of axis `index`, as the scene draws it.
fn axis_color(palette: &Palette, index: usize) -> Color {
    let Srgb([r, g, b]) = palette.scene.axes[index];
    Color::from_rgb(r, g, b)
}

/// The strokes of the letter of axis `index`, centred on the origin, a
/// unit high. Drawn as paths rather than text, which a canvas puts over
/// all its shapes, so the faces can hide one behind them.
fn letter_strokes(index: usize) -> &'static [&'static [(f32, f32)]] {
    const W: f32 = 0.38;
    match index {
        0 => &[&[(-W, -0.5), (W, 0.5)], &[(W, -0.5), (-W, 0.5)]],
        1 => &[
            &[(-W, -0.5), (0.0, 0.0), (W, -0.5)],
            &[(0.0, 0.0), (0.0, 0.5)],
        ],
        _ => &[&[(-W, -0.5), (W, -0.5), (-W, 0.5), (W, 0.5)]],
    }
}

/// Draws the parts of `axes` drawn over the faces if `over`, else those
/// drawn under them. Their edges are always over: each runs along a face
/// turned to the camera.
fn draw_axes(frame: &mut Frame, palette: &Palette, axes: &[Option<Axis>; 3], over: bool) {
    for axis in axes.iter().flatten() {
        let color = axis_color(palette, axis.index);
        let stroke = Stroke::default()
            .with_color(color)
            .with_width(AXIS_WIDTH)
            .with_line_cap(LineCap::Round);
        if over {
            frame.stroke(&Path::line(point(axis.start), point(axis.end)), stroke);
        }
        if axis.tip_shown != over {
            continue;
        }
        let base = axis.tip - axis.direction * ARROW_LENGTH;
        // The shaft stops where the arrowhead starts, if there's room for
        // one.
        let room = (axis.tip - axis.end).length() > ARROW_LENGTH;
        let shaft_end = if room { base } else { axis.tip };
        frame.stroke(&Path::line(point(axis.end), point(shaft_end)), stroke);
        if room {
            let side = axis.direction.perp() * ARROW_HALF_WIDTH;
            let head = Path::new(|b| {
                b.move_to(point(axis.tip));
                b.line_to(point(base + side));
                b.line_to(point(base - side));
                b.close();
            });
            frame.fill(&head, color);
        }
        let letter = Path::new(|b| {
            for stroke in letter_strokes(axis.index) {
                let at =
                    |&(x, y): &(f32, f32)| point(axis.letter + Vec2::new(x, y) * AXIS_LETTER_SIZE);
                b.move_to(at(&stroke[0]));
                for p in &stroke[1..] {
                    b.line_to(at(p));
                }
            }
        });
        frame.stroke(
            &letter,
            Stroke::default()
                .with_color(color)
                .with_width(LETTER_WIDTH)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round),
        );
    }
}

fn point(v: Vec2) -> Point {
    Point::new(v.x, v.y)
}

struct ViewCube {
    basis: Basis,
}

impl ViewCube {
    /// The face under the cursor as the cube is now turned. Drawing and the
    /// cursor use this rather than the state, which only catches up on the
    /// next mouse event and so goes stale when the camera turns under a
    /// still cursor.
    fn face_under(&self, bounds: Rectangle, cursor: mouse::Cursor) -> Option<View> {
        cursor
            .position_in(bounds)
            .and_then(|p| face_at(&self.basis, Vec2::new(p.x, p.y)))
    }
}

impl canvas::Program<Message> for ViewCube {
    /// The face under the cursor when last drawn, to redraw when it changes.
    type State = Option<View>;

    fn update(
        &self,
        hovered: &mut Option<View>,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Event::Mouse(event) = event else {
            return None;
        };
        let face = self.face_under(bounds, cursor);

        if let mouse::Event::ButtonPressed(mouse::Button::Left) = event
            && let Some(face) = face
        {
            return Some(Action::publish(Message::Look(Look::LookFrom(face))).and_capture());
        }
        if face != *hovered {
            *hovered = face;
            return Some(Action::request_redraw());
        }
        None
    }

    fn draw(
        &self,
        _state: &Option<View>,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let hovered = self.face_under(bounds, cursor);
        let palette = theme::palette(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        let axes = axes(&self.basis);
        draw_axes(&mut frame, palette, &axes, false);

        for face in visible_faces(&self.basis) {
            let outline = Path::new(|b| {
                b.move_to(point(face.corners[0]));
                for &c in &face.corners[1..] {
                    b.line_to(point(c));
                }
                b.close();
            });
            let color = if hovered == Some(face.view) {
                palette.hl_line
            } else {
                fill(palette, &self.basis, face.view.normal())
            };
            frame.fill(&outline, color);
            frame.stroke(
                &outline,
                Stroke::default()
                    .with_color(Color {
                        a: 0.45,
                        ..palette.edge
                    })
                    .with_width(0.6),
            );

            let (phi, scale, theta) = decompose(face.u, face.v);
            frame.with_save(|frame| {
                frame.translate(Vector::new(face.center.x, face.center.y));
                frame.rotate(Radians(phi));
                frame.scale_nonuniform(Vector::new(scale.x, scale.y));
                frame.rotate(Radians(theta));
                frame.fill_text(Text {
                    content: face.letter.into(),
                    position: Point::ORIGIN,
                    color: Color {
                        a: 0.7,
                        ..palette.text
                    },
                    size: LETTER_SIZE.into(),
                    font: Font {
                        weight: font::Weight::Bold,
                        ..Font::DEFAULT
                    },
                    align_x: Alignment::Center,
                    align_y: Vertical::Center,
                    ..Text::default()
                });
            });
        }

        draw_axes(&mut frame, palette, &axes, true);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Option<View>,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match self.face_under(bounds, cursor) {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }
}

#[cfg(test)]
mod tests;
