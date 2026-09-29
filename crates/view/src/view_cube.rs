//! The view cube: a small cube in the viewport corner that turns with the
//! camera. Clicking a face looks at the model from that side.

use glam::{Vec2, Vec3};
use iced::alignment::Vertical;
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::Alignment;
use iced::{Color, Element, Event, Font, Length, Point, Radians, Rectangle, Renderer, Theme, font};
use iced::{Vector, mouse};
use varde_render::{Camera, View};

use crate::theme::{self, Palette};
use crate::{Look, Message};

/// Width and height of the widget.
const SIZE: f32 = 96.0;
/// Half the cube's edge length.
const HALF: f32 = 25.0;
const LETTER_SIZE: f32 = 17.0;

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
