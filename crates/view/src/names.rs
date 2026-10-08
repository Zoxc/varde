//! The names overlay: a label for each face, edge and corner selected,
//! and every one of a body selected whole, with the name references
//! store for it (a face's key, an edge's two keys, a corner's three),
//! placed outside the bodies where there's room and joined to what it
//! names by a curved leader. Labels never overlap each other; each goes
//! to the free place nearest what it names, so its leader is as short as
//! it can be. The label under the cursor glows, and what it names is
//! highlighted faintly.

use glam::{DVec2, DVec3};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::Shaping;
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};
use varde_document::{BodyId, Document, FeatureId};
use varde_kernel::mesh::{FaceKey, PartKey};
use varde_render::Camera;

use crate::Message;
use crate::pick::{PickIndex, Picked, Snapped};
use crate::projection::Projector;
use crate::status::STATUS_BAR_ROOM;
use crate::theme;
use crate::{SketchItem, SketchLines};

/// The overlay over a viewport showing `index`'s model from `camera`,
/// naming everything of `bodies` and the faces, edges and corners
/// `targets`, laid out outside `around`; and of the finished sketches
/// `sketches` shown, the curves and points `items` and every one of the
/// sketches `whole`.
#[expect(clippy::too_many_arguments)]
pub(crate) fn overlay<'a>(
    index: &'a PickIndex,
    bodies: Vec<BodyId>,
    targets: Vec<Picked>,
    around: Vec<BodyId>,
    sketches: Vec<SketchLines<'a>>,
    items: Vec<SketchItem>,
    whole: Vec<FeatureId>,
    camera: Camera,
    document: &'a Document,
) -> Element<'a, Message> {
    Canvas::new(Names {
        index,
        bodies,
        targets,
        around,
        sketches,
        items,
        whole,
        camera,
        document,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

struct Names<'a> {
    index: &'a PickIndex,
    /// The bodies named whole.
    bodies: Vec<BodyId>,
    /// The faces, edges and corners named.
    targets: Vec<Picked>,
    /// The bodies whose outline the labels go outside.
    around: Vec<BodyId>,
    /// The finished sketches shown, where they're placed.
    sketches: Vec<SketchLines<'a>>,
    /// Their curves and points named.
    items: Vec<SketchItem>,
    /// The sketches named whole.
    whole: Vec<FeatureId>,
    camera: Camera,
    document: &'a Document,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Face,
    Edge,
    Corner,
    /// A finished sketch's curve.
    Curve,
    /// A finished sketch's point.
    Point,
}

impl Kind {
    /// Its labels' colour.
    fn colour(self) -> Color {
        match self {
            Kind::Face => Color::from_rgb8(0x0e, 0xa5, 0xe9),
            Kind::Edge => Color::from_rgb8(0xe1, 0x1d, 0x48),
            Kind::Corner => Color::from_rgb8(0x22, 0xc5, 0x5e),
            Kind::Curve => Color::from_rgb8(0x8b, 0x5c, 0xf6),
            Kind::Point => Color::from_rgb8(0xd9, 0x46, 0xef),
        }
    }
}

/// A label wanted: what it names, where that shows, and its words.
struct Wanted {
    kind: Kind,
    /// Where it is in the world, a face's normal there and its direction
    /// (an edge's; any across a face's normal), unit or zero.
    point: DVec3,
    normal: DVec3,
    tangent: DVec3,
    /// What it names, to highlight with the label hovered.
    target: Target,
    anchor: DVec2,
    words: Vec<Piece>,
}

/// What a label names, as its hover highlights it.
enum Target {
    Model(Picked),
    /// A sketch's curve, in the world.
    Line(Vec<DVec3>),
    /// A sketch's point.
    Point(DVec3),
}

/// A piece of a label's words, drawn in turn.
enum Piece {
    /// A feature's name.
    Feature(String),
    /// A part's name, after its feature's.
    Part(String),
    /// The chevron between them.
    Chevron,
    /// The bar between two keys.
    Join,
}

impl Piece {
    fn width(&self) -> f32 {
        match self {
            Piece::Feature(text) | Piece::Part(text) => text_width(text),
            Piece::Chevron => 10.0,
            Piece::Join => 11.0,
        }
    }
}

/// The most labels drawn: past it the overlay is only clutter.
const MOST: usize = 300;
const TEXT_SIZE: f32 = 11.0;
const LABEL_HEIGHT: f32 = 18.0;
const RADIUS: f32 = 3.0;
const PAD: f32 = 6.0;
/// The room the kind's icon takes at a label's start.
const MARK_ROOM: f32 = 14.0;
/// The space kept between labels, and between a label and the model.
const GAP: f32 = 3.0;

/// What the overlay keeps between draws: where the labels went, and the
/// one under the cursor, so a move redraws only when that changes.
#[derive(Default)]
struct Hovering {
    rects: std::cell::RefCell<Vec<Option<Rectangle>>>,
    hovered: Option<usize>,
}

impl Hovering {
    /// The label at `at`, of those placed last.
    fn at(&self, at: Option<Point>) -> Option<usize> {
        let at = at?;
        let rects = self.rects.borrow();
        // The last drawn is on top.
        (0..rects.len())
            .rev()
            .find(|&i| rects[i].is_some_and(|rect| rect.contains(at)))
    }
}

impl canvas::Program<Message> for Names<'_> {
    type State = Hovering;

    fn update(
        &self,
        state: &mut Hovering,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) = event
        else {
            return None;
        };
        let hovered = state.at(cursor.position_in(bounds));
        (hovered != state.hovered).then(|| {
            state.hovered = hovered;
            canvas::Action::request_redraw()
        })
    }

    fn draw(
        &self,
        state: &Hovering,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = [bounds.width, bounds.height];
        let Some(projector) = Projector::world(&self.camera, size[0], size[1]) else {
            return Vec::new();
        };
        let wanted = self.wanted(&projector, size);
        // Outside the bodies and the sketches' curves and points named.
        let mut outline = self.silhouette(&projector);
        for want in &wanted {
            match &want.target {
                Target::Line(points) => outline.extend(points.iter().map(|p| projector.show(*p))),
                Target::Point(p) => outline.push(projector.show(*p)),
                Target::Model(_) => {}
            }
        }
        let hull = hull(&outline);
        let widths: Vec<f32> = (wanted.iter())
            .map(|want| want.words.iter().map(Piece::width).sum())
            .collect();
        let placed = place(&wanted, &widths, &hull, bounds.size());
        let p = theme::palette(theme);
        *state.rects.borrow_mut() = placed.clone();
        let hovered = state.at(cursor.position_in(bounds));
        if let Some(i) = hovered {
            let colour = wanted[i].kind.colour();
            match &wanted[i].target {
                &Target::Model(target) => self.draw_target(&mut frame, &projector, target, colour),
                Target::Line(points) => {
                    let path = Path::new(|b| {
                        for (k, p) in points.iter().enumerate() {
                            let s = projector.show(*p);
                            let s = Point::new(s.x as f32, s.y as f32);
                            if k == 0 {
                                b.move_to(s);
                            } else {
                                b.line_to(s);
                            }
                        }
                    });
                    frame.stroke(
                        &path,
                        Stroke::default()
                            .with_color(Color { a: 0.35, ..colour })
                            .with_width(6.0)
                            .with_line_cap(canvas::LineCap::Round)
                            .with_line_join(canvas::LineJoin::Round),
                    );
                }
                Target::Point(p) => {
                    let s = projector.show(*p);
                    frame.fill(
                        &Path::circle(Point::new(s.x as f32, s.y as f32), 9.0),
                        Color { a: 0.15, ..colour },
                    );
                }
            }
        }
        for (i, (want, rect)) in wanted.iter().zip(&placed).enumerate() {
            if let Some(rect) = rect
                && hovered != Some(i)
            {
                draw_label(&mut frame, &projector, p, want, *rect, false);
            }
        }
        // The one hovered over the rest.
        if let Some(i) = hovered
            && let Some(rect) = placed[i]
        {
            draw_label(&mut frame, &projector, p, &wanted[i], rect, true);
        }
        vec![frame.into_geometry()]
    }
}

impl Names<'_> {
    /// The labels wanted: of what's selected itself, in view; of the
    /// bodies selected whole, what's in view and nothing hides, on a
    /// side turned towards the eye.
    fn wanted(&self, projector: &Projector, size: [f32; 2]) -> Vec<Wanted> {
        let mesh = self.index.mesh();
        let picking = self.index.picking();
        let positions = mesh.positions();
        let at = |i: u32| glam::Vec3::from(positions[i as usize]).as_dvec3();
        let in_front = |p: glam::DVec3| {
            !projector.perspective() || projector.world_depth(p) >= projector.near()
        };
        // Whether `p` is in view, and unless what's there is selected
        // itself (labelled hidden or not), nothing hides it.
        let shows = |p: glam::DVec3, picked: bool| {
            let s = projector.show(p);
            in_front(p)
                && s.x >= 0.0
                && s.y >= 0.0
                && s.x <= f64::from(size[0])
                && s.y <= f64::from(size[1])
                && (picked || !self.index.hides(&self.camera, size, p))
        };
        let normals = mesh.normals();
        let normal = |i: u32| glam::Vec3::from(normals[i as usize]).as_dvec3();
        let (eye, backward) = projector.eye();
        // Whether a surface at `p` with normal `n` turns towards the eye.
        let faces_eye = |p: glam::DVec3, n: glam::DVec3| {
            let towards = if projector.perspective() {
                eye - p
            } else {
                backward
            };
            n.dot(towards) > 0.0
        };
        // Whether face `face` turns towards the eye near `p`: its
        // triangle nearest there does, by its normals.
        let facing = |face: u32, p: glam::DVec3| {
            let Some(range) = mesh.face_indices(face as usize) else {
                return false;
            };
            let nearest = (mesh.indices()[range].as_chunks::<3>().0.iter())
                .map(|t| {
                    let c = (at(t[0]) + at(t[1]) + at(t[2])) / 3.0;
                    (c.distance_squared(p), t)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            nearest.is_some_and(|(_, t)| faces_eye(p, normal(t[0]) + normal(t[1]) + normal(t[2])))
        };
        // The unit normal of face `face` near `p`, by its nearest
        // triangle's normals.
        let normal_near = |face: u32, p: glam::DVec3| {
            let Some(range) = mesh.face_indices(face as usize) else {
                return glam::DVec3::ZERO;
            };
            (mesh.indices()[range].as_chunks::<3>().0.iter())
                .map(|t| {
                    let c = (at(t[0]) + at(t[1]) + at(t[2])) / 3.0;
                    (c.distance_squared(p), t)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map_or(glam::DVec3::ZERO, |(_, t)| {
                    (normal(t[0]) + normal(t[1]) + normal(t[2])).normalize_or_zero()
                })
        };
        let across = |n: glam::DVec3| n.any_orthonormal_vector();
        // The corners of the vertices selected: a selected vertex is a
        // vertex of the mesh, not a corner of the tables.
        let corners: Vec<u32> = (self.targets.iter())
            .filter_map(|target| match *target {
                Picked::Vertex(vertex) => self.index.vertex_corner(vertex),
                _ => None,
            })
            .collect();
        let named = |face: u32| {
            (self.index.face_body(face)).is_some_and(|body| self.bodies.contains(&body))
        };
        let (near_faces, near_edges, near_corners) = self.connected(&corners);
        let mut wanted = Vec::new();

        {
            for face in 0..mesh.face_count() {
                let picked = self.targets.contains(&Picked::Face(face as u32));
                if !named(face as u32) && !picked && !near_faces.contains(&(face as u32)) {
                    continue;
                }
                let Some(key) = picking.faces().get(face).map(|f| f.key) else {
                    continue;
                };
                let Some(range) = mesh.face_indices(face) else {
                    continue;
                };
                let tris = mesh.indices()[range].as_chunks::<3>().0;
                let triangles: Vec<_> = tris
                    .iter()
                    .map(|t| [at(t[0]), at(t[1]), at(t[2])])
                    .collect();
                // The centroid by area, then the triangles nearest it,
                // on the face, until one shows.
                let mut total = 0.0;
                let mut sum = glam::DVec3::ZERO;
                for [a, b, c] in &triangles {
                    let area = (*b - *a).cross(*c - *a).length();
                    total += area;
                    sum += (*a + *b + *c) / 3.0 * area;
                }
                if total <= 0.0 {
                    continue;
                }
                let centre = sum / total;
                // Only the triangles turned towards the eye: a face seen
                // from behind is hidden.
                let mut centres: Vec<_> = (triangles.iter().zip(tris))
                    .filter(|&([a, b, c], t)| {
                        picked
                            || faces_eye(
                                (*a + *b + *c) / 3.0,
                                normal(t[0]) + normal(t[1]) + normal(t[2]),
                            )
                    })
                    .map(|([a, b, c], _)| (*a + *b + *c) / 3.0)
                    .collect();
                centres.sort_by(|a, b| {
                    a.distance_squared(centre)
                        .total_cmp(&b.distance_squared(centre))
                });
                if let Some(p) = centres.into_iter().take(8).find(|p| shows(*p, picked)) {
                    let n = normal_near(face as u32, p);
                    wanted.push(Wanted {
                        kind: Kind::Face,
                        point: p,
                        normal: n,
                        tangent: if n == glam::DVec3::ZERO { n } else { across(n) },
                        target: Target::Model(Picked::Face(face as u32)),
                        anchor: projector.show(p),
                        words: self.words(&[key]),
                    });
                }
            }
        }

        {
            for edge in 0..mesh.edge_count() {
                let picked = self.targets.contains(&Picked::Edge(edge as u32));
                if !(self.index.edge_faces(edge as u32)).is_some_and(|[a, _]| named(a))
                    && !picked
                    && !near_edges.contains(&(edge as u32))
                {
                    continue;
                }
                let Some(keys) = picking.edge_keys(mesh, edge as u32) else {
                    continue;
                };
                let Some(line) = mesh.polyline(edge) else {
                    continue;
                };
                let points: Vec<_> = line.iter().map(|&i| at(i)).collect();
                let Some(p) = halfway(&points) else {
                    continue;
                };
                // One of its faces must turn towards the eye there.
                let sides = self.index.edge_faces(edge as u32).unwrap_or_default();
                if (picked || sides.iter().any(|&f| facing(f, p))) && shows(p, picked) {
                    // The direction of its segment through `p`.
                    let tangent = (points.windows(2))
                        .map(|w| (w[1] - w[0], (w[0] + w[1]) / 2.0))
                        .min_by(|a, b| a.1.distance_squared(p).total_cmp(&b.1.distance_squared(p)))
                        .map_or(glam::DVec3::ZERO, |(d, _)| d.normalize_or_zero());
                    wanted.push(Wanted {
                        kind: Kind::Edge,
                        point: p,
                        normal: glam::DVec3::ZERO,
                        tangent,
                        target: Target::Model(Picked::Edge(edge as u32)),
                        anchor: projector.show(p),
                        words: self.words(&keys),
                    });
                }
            }
        }

        {
            for (corner, c) in picking.corners().iter().enumerate() {
                let p = glam::DVec3::from(c.point);
                let picked = corners.contains(&(corner as u32));
                if (named(c.faces[0]) || picked || near_corners.contains(&(corner as u32)))
                    && (picked || c.faces.iter().any(|&f| facing(f, p)))
                    && shows(p, picked)
                {
                    let keys = picking.corner_keys(corner as u32);
                    wanted.push(Wanted {
                        kind: Kind::Corner,
                        point: p,
                        normal: glam::DVec3::ZERO,
                        tangent: glam::DVec3::ZERO,
                        target: Target::Model(Picked::Vertex(corner as u32)),
                        anchor: projector.show(p),
                        words: self.words(&keys),
                    });
                }
            }
        }

        self.sketch_wanted(projector, size, &mut wanted);
        wanted.truncate(MOST);
        wanted
    }

    /// The named bodies' outline on screen: their faces' positions
    /// shown, a sample of them where there are many.
    fn silhouette(&self, projector: &Projector) -> Vec<DVec2> {
        let mesh = self.index.mesh();
        let positions = mesh.positions();
        let vertices: Vec<u32> = (0..mesh.face_count())
            .filter(|&face| {
                (self.index.face_body(face as u32)).is_some_and(|body| self.around.contains(&body))
            })
            .filter_map(|face| mesh.face_indices(face))
            .flat_map(|range| mesh.indices()[range].iter().copied())
            .collect();
        let step = (vertices.len() / 20_000).max(1);
        vertices
            .iter()
            .step_by(step)
            .map(|&i| glam::Vec3::from(positions[i as usize]).as_dvec3())
            .filter(|p| !projector.perspective() || projector.world_depth(*p) >= projector.near())
            .map(|p| projector.show(p))
            .collect()
    }

    /// What's connected to the faces, edges and corners (`corners`, of
    /// the vertices) selected, labelled with them as what the model
    /// shows is: a face's border edges and its corners, an edge's faces
    /// and the corners at its ends, the edges ending at a corner and the
    /// faces meeting there. Faces, edges and corners, by id.
    fn connected(&self, corners: &[u32]) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
        let mesh = self.index.mesh();
        let (mut faces, mut edges, mut ends) = (Vec::new(), Vec::new(), Vec::new());
        let corners_of = |target: Picked| {
            (self.index.snaps(target).into_iter()).filter_map(|(snapped, _)| match snapped {
                Snapped::Corner(corner) => Some(corner),
                Snapped::EdgePoint(_) => None,
            })
        };
        for &target in &self.targets {
            match target {
                Picked::Face(face) => {
                    edges.extend(
                        (0..mesh.edge_count() as u32).filter(|&e| {
                            self.index.edge_faces(e).is_some_and(|f| f.contains(&face))
                        }),
                    );
                    ends.extend(corners_of(target));
                }
                Picked::Edge(edge) => {
                    faces.extend(self.index.edge_faces(edge).into_iter().flatten());
                    ends.extend(corners_of(target));
                }
                Picked::Vertex(_) => {}
            }
        }
        for &corner in corners {
            let Some(c) = self.index.picking().corners().get(corner as usize) else {
                continue;
            };
            faces.extend(c.faces);
            edges.extend((0..mesh.edge_count() as u32).filter(|&e| {
                self.index
                    .edge_faces(e)
                    .is_some_and(|f| f.iter().any(|f| c.faces.contains(f)))
                    && corners_of(Picked::Edge(e)).any(|end| end == corner)
            }));
        }
        (faces, edges, ends)
    }

    /// The labels wanted of the finished sketches: of their curves and
    /// points selected, those in view; of the sketches selected whole,
    /// those in view that the model doesn't hide. A curve is named by
    /// its sketch, its name and its id (which a face swept from it
    /// carries, "S5"), a point by its sketch and name.
    fn sketch_wanted(&self, projector: &Projector, size: [f32; 2], wanted: &mut Vec<Wanted>) {
        let in_view = |p: DVec3, picked: bool| {
            let s = projector.show(p);
            (!projector.perspective() || projector.world_depth(p) >= projector.near())
                && s.x >= 0.0
                && s.y >= 0.0
                && s.x <= f64::from(size[0])
                && s.y <= f64::from(size[1])
                && (picked || !self.index.hides(&self.camera, size, p))
        };
        for lines in &self.sketches {
            let whole = self.whole.contains(&lines.feature);
            let picked = |id| {
                (self.items.iter()).any(|item| item.sketch == lines.feature && item.item == id)
            };
            // What's connected to the items selected: a curve's points
            // (a spline's ends, not its control points), and the curves
            // a point is one of.
            let sketch = lines.sketch;
            let mut near: Vec<varde_sketch::Id> = Vec::new();
            for entry in &sketch.curves {
                let shape = &entry.curve;
                let points: Vec<_> = match shape {
                    varde_sketch::Curve::Spline(_) => shape.ends().into_iter().flatten().collect(),
                    _ => shape.points().collect(),
                };
                if picked(entry.id) {
                    near.extend(&points);
                }
                if points.iter().any(|&point| picked(point)) {
                    near.push(entry.id);
                }
            }
            let sketch_name = (self.document.features().iter())
                .find(|f| f.id == lines.feature)
                .map_or_else(String::new, |f| f.name.clone());
            let world = |p: glam::DVec2| lines.placement.to_world(p);
            for entry in &lines.sketch.curves {
                let picked = picked(entry.id);
                if !whole && !picked && !near.contains(&entry.id) {
                    continue;
                }
                let Some(flat) = lines.sketch.flatten(&entry.curve) else {
                    continue;
                };
                let points: Vec<DVec3> = flat.into_iter().map(world).collect();
                let Some(p) = halfway(&points) else {
                    continue;
                };
                if !in_view(p, picked) {
                    continue;
                }
                let tangent = (points.windows(2))
                    .map(|w| (w[1] - w[0], (w[0] + w[1]) / 2.0))
                    .min_by(|a, b| a.1.distance_squared(p).total_cmp(&b.1.distance_squared(p)))
                    .map_or(DVec3::ZERO, |(d, _)| d.normalize_or_zero());
                wanted.push(Wanted {
                    kind: Kind::Curve,
                    point: p,
                    normal: DVec3::ZERO,
                    tangent,
                    anchor: projector.show(p),
                    words: vec![
                        Piece::Feature(sketch_name.clone()),
                        Piece::Chevron,
                        Piece::Part(format!("{} #{}", entry.name(), entry.id)),
                    ],
                    target: Target::Line(points),
                });
            }
            for point in &lines.sketch.points {
                let picked = picked(point.id);
                if !whole && !picked && !near.contains(&point.id) {
                    continue;
                }
                let p = world(point.at);
                if !in_view(p, picked) {
                    continue;
                }
                wanted.push(Wanted {
                    kind: Kind::Point,
                    point: p,
                    normal: DVec3::ZERO,
                    tangent: DVec3::ZERO,
                    anchor: projector.show(p),
                    words: vec![
                        Piece::Feature(sketch_name.clone()),
                        Piece::Chevron,
                        Piece::Part(lines.sketch.point_name(point)),
                    ],
                    target: Target::Point(p),
                });
            }
        }
    }

    /// `keys` written short, joined.
    fn words(&self, keys: &[FaceKey]) -> Vec<Piece> {
        let mut pieces = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            if i > 0 {
                pieces.push(Piece::Join);
            }
            let feature = (self.document.features().iter())
                .find(|f| f.id.get() == key.feature)
                .map_or_else(|| format!("#{}", key.feature), |f| f.name.clone());
            let instance = if key.instance == 0 {
                String::new()
            } else {
                format!("#{}", small(key.instance))
            };
            pieces.push(Piece::Feature(feature));
            pieces.push(Piece::Chevron);
            pieces.push(Piece::Part(part_words(&key.part) + &instance));
        }
        pieces
    }

    /// `target` highlighted faintly in `colour`: a face's triangles
    /// turned towards the eye filled and its border drawn, an edge drawn
    /// wide, a corner haloed.
    fn draw_target(&self, frame: &mut Frame, projector: &Projector, target: Picked, colour: Color) {
        let mesh = self.index.mesh();
        let at = |i: u32| glam::Vec3::from(mesh.positions()[i as usize]).as_dvec3();
        let normal = |i: u32| glam::Vec3::from(mesh.normals()[i as usize]).as_dvec3();
        let screen = |p: DVec3| {
            let s = projector.show(p);
            Point::new(s.x as f32, s.y as f32)
        };
        let (eye, backward) = projector.eye();
        let towards = |p: DVec3| {
            if projector.perspective() {
                eye - p
            } else {
                backward
            }
        };
        let faint = |a: f32| Color { a, ..colour };
        let polyline = |b: &mut canvas::path::Builder, line: &[u32]| {
            for (k, &i) in line.iter().enumerate() {
                if k == 0 {
                    b.move_to(screen(at(i)));
                } else {
                    b.line_to(screen(at(i)));
                }
            }
        };
        let line = |frame: &mut Frame, path: &Path, width: f32| {
            frame.stroke(
                path,
                Stroke::default()
                    .with_color(faint(0.35))
                    .with_width(width + 2.0)
                    .with_line_cap(canvas::LineCap::Round)
                    .with_line_join(canvas::LineJoin::Round),
            );
        };
        match target {
            Picked::Face(face) => {
                let Some(range) = mesh.face_indices(face as usize) else {
                    return;
                };
                let path = Path::new(|b| {
                    for t in mesh.indices()[range].as_chunks::<3>().0 {
                        let centre = (at(t[0]) + at(t[1]) + at(t[2])) / 3.0;
                        let n = normal(t[0]) + normal(t[1]) + normal(t[2]);
                        if n.dot(towards(centre)) <= 0.0 {
                            continue;
                        }
                        b.move_to(screen(at(t[0])));
                        b.line_to(screen(at(t[1])));
                        b.line_to(screen(at(t[2])));
                        b.close();
                    }
                });
                frame.fill(&path, faint(0.14));
                // Its border: the edges it's on either side of.
                let border = Path::new(|b| {
                    for (edge, sides) in mesh.edge_faces().iter().enumerate() {
                        if sides.contains(&face)
                            && let Some(l) = mesh.polyline(edge)
                        {
                            polyline(b, l);
                        }
                    }
                });
                line(frame, &border, 2.0);
            }
            Picked::Edge(edge) => {
                let Some(l) = mesh.polyline(edge as usize) else {
                    return;
                };
                line(frame, &Path::new(|b| polyline(b, l)), 4.0);
            }
            Picked::Vertex(corner) => {
                let Some(c) = self.index.picking().corners().get(corner as usize) else {
                    return;
                };
                frame.fill(
                    &Path::circle(screen(DVec3::from(c.point)), 9.0),
                    faint(0.15),
                );
            }
        }
    }
}

/// `want`'s label in `rect`, an outlined badge in its kind's colour, and
/// its curved leader and end; glowing if `hovered`.
fn draw_label(
    frame: &mut Frame,
    projector: &Projector,
    p: &theme::Palette,
    want: &Wanted,
    rect: Rectangle,
    hovered: bool,
) {
    let colour = want.kind.colour();
    let at = Point::new(want.anchor.x as f32, want.anchor.y as f32);
    let attach = nearest_on(rect, at);

    // The leader, then its end on what it names.
    frame.stroke(
        &Path::new(|b| {
            b.move_to(at);
            b.quadratic_curve_to(Point::new(at.x, attach.y), attach);
        }),
        Stroke::default().with_color(colour).with_width(1.2),
    );
    draw_end(frame, projector, want, colour, p.panel);

    // The badge.
    let body = Path::rounded_rectangle(rect.position(), rect.size(), RADIUS.into());
    if hovered {
        frame.stroke(
            &body,
            Stroke::default()
                .with_color(Color { a: 0.35, ..colour })
                .with_width(7.0),
        );
    }
    frame.fill(&body, p.panel);
    frame.stroke(&body, Stroke::default().with_color(colour).with_width(1.2));

    let mid = rect.center_y();
    draw_icon(
        frame,
        want.kind,
        Point::new(rect.x + PAD + 5.0, mid),
        colour,
    );
    draw_words(frame, want, (rect.x + PAD + MARK_ROOM, mid), p.text, colour);
}

/// The kind's hollow icon centred at `c`: a square for a face, a stroke
/// for an edge, a ring for a corner.
fn draw_icon(frame: &mut Frame, kind: Kind, c: Point, ink: Color) {
    let stroke = Stroke::default().with_color(ink).with_width(1.4);
    match kind {
        Kind::Face => frame.stroke(
            &Path::rectangle(Point::new(c.x - 3.5, c.y - 3.5), Size::new(7.0, 7.0)),
            stroke,
        ),
        Kind::Edge => frame.stroke(
            &Path::line(Point::new(c.x - 4.5, c.y), Point::new(c.x + 4.5, c.y)),
            stroke,
        ),
        Kind::Corner => frame.stroke(&Path::circle(c, 3.5), stroke),
        Kind::Curve => frame.stroke(
            &Path::new(|b| {
                b.move_to(Point::new(c.x - 4.5, c.y + 3.0));
                b.quadratic_curve_to(Point::new(c.x, c.y - 5.0), Point::new(c.x + 4.5, c.y + 3.0));
            }),
            stroke,
        ),
        Kind::Point => frame.stroke(
            &Path::new(|b| {
                b.move_to(Point::new(c.x, c.y - 4.0));
                b.line_to(Point::new(c.x + 4.0, c.y));
                b.line_to(Point::new(c.x, c.y + 4.0));
                b.line_to(Point::new(c.x - 4.0, c.y));
                b.close();
            }),
            stroke,
        ),
    }
}

/// The pieces of `want`'s words from `x` along the line at `mid`, in
/// `ink`, the chevrons and bars in `border`.
fn draw_words(
    frame: &mut Frame,
    want: &Wanted,
    (mut x, mid): (f32, f32),
    ink: Color,
    border: Color,
) {
    for piece in &want.words {
        let width = piece.width();
        let c = Point::new(x + width / 2.0, mid);
        match piece {
            Piece::Feature(text) | Piece::Part(text) => frame.fill_text(Text {
                content: text.clone(),
                position: Point::new(x, mid),
                color: ink,
                size: TEXT_SIZE.into(),
                shaping: Shaping::Advanced,
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            }),
            Piece::Chevron => frame.stroke(
                &Path::new(|b| {
                    b.move_to(Point::new(c.x - 1.5, c.y - 3.5));
                    b.line_to(Point::new(c.x + 2.0, c.y));
                    b.line_to(Point::new(c.x - 1.5, c.y + 3.5));
                }),
                Stroke::default().with_color(border).with_width(1.4),
            ),
            // The badge's full height.
            Piece::Join => frame.fill(
                &Path::rectangle(
                    Point::new(c.x - 0.6, mid - LABEL_HEIGHT / 2.0),
                    Size::new(1.2, LABEL_HEIGHT),
                ),
                border,
            ),
        }
        x += width;
    }
}

/// Where the world point `p` moved `pixels` along the unit `direction`
/// shows, the move as long as `pixels` at `p`'s depth.
fn along(projector: &Projector, p: DVec3, direction: DVec3, pixels: f32) -> Point {
    let world = projector.pixel_at(projector.world_depth(p)) * f64::from(pixels);
    let s = projector.show(p + direction * world);
    Point::new(s.x as f32, s.y as f32)
}

/// The end of `want`'s leader on what it names, drawn in the world: a
/// disc lying on a face, a stroke along an edge; a ring, filled with
/// `back`, at a corner, or where there's no normal or direction to draw
/// by. Sized in pixels.
fn draw_end(frame: &mut Frame, projector: &Projector, want: &Wanted, colour: Color, back: Color) {
    let (p, n, t) = (want.point, want.normal, want.tangent);
    let to = |d: DVec3, pixels: f32| along(projector, p, d, pixels);
    let stroke = Stroke::default().with_color(colour).with_width(1.6);
    match want.kind {
        Kind::Face if n != DVec3::ZERO && t != DVec3::ZERO => {
            let b = n.cross(t);
            let disc = Path::new(|path| {
                for k in 0..=24 {
                    let angle = f64::from(k) / 24.0 * std::f64::consts::TAU;
                    let q = to(
                        t * varde_sketch::angle::cos(angle) + b * varde_sketch::angle::sin(angle),
                        7.0,
                    );
                    if k == 0 {
                        path.move_to(q);
                    } else {
                        path.line_to(q);
                    }
                }
                path.close();
            });
            frame.fill(&disc, Color { a: 0.35, ..colour });
            frame.stroke(&disc, stroke);
        }
        Kind::Edge | Kind::Curve if t != DVec3::ZERO => frame.stroke(
            &Path::line(to(t, -9.0), to(t, 9.0)),
            Stroke {
                width: 4.0,
                line_cap: canvas::LineCap::Round,
                ..stroke
            },
        ),
        _ => {
            let ring = Path::circle(to(DVec3::ZERO, 0.0), 3.5);
            frame.fill(&ring, back);
            frame.stroke(
                &ring,
                Stroke {
                    width: 1.4,
                    ..stroke
                },
            );
        }
    }
}

/// How wide `content` is drawn at [`TEXT_SIZE`], as the label draws it.
fn text_width(content: &str) -> f32 {
    use iced::advanced::text::{self, Paragraph as _};
    <<Renderer as text::Renderer>::Paragraph>::with_text(text::Text {
        content,
        bounds: Size::INFINITE,
        size: TEXT_SIZE.into(),
        line_height: text::LineHeight::default(),
        font: iced::Font::default(),
        align_x: text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: text::Wrapping::None,
    })
    .min_width()
}

/// A part's words, short.
fn part_words(part: &PartKey) -> String {
    match *part {
        PartKey::StartCap => "Start".into(),
        PartKey::EndCap => "End".into(),
        PartKey::Side { curve } => format!("S{}", small(curve)),
        PartKey::Split(n) => format!("Sp{n}"),
        PartKey::Blend { edge } => format!("B{}", small(edge)),
        PartKey::Corner { vertex } => format!("C{}", small(vertex)),
        PartKey::Offset { of } => format!("O{}", small(of)),
        PartKey::BackSide { curve } => format!("K{}", small(curve)),
        PartKey::Swept { curve } => format!("W{}", small(curve)),
        PartKey::Lofted { curve, span } => format!("L{}.{span}", small(curve)),
    }
}

/// A number short: as it is if small, else its last hex digits, as the
/// mixed names (a blend's edge) are.
fn small(n: u64) -> String {
    if n < 10_000 {
        n.to_string()
    } else {
        format!("{:04x}", n & 0xffff)
    }
}

/// The point halfway along a polyline, by length.
fn halfway(points: &[glam::DVec3]) -> Option<glam::DVec3> {
    let length: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut left = length / 2.0;
    for w in points.windows(2) {
        let d = w[0].distance(w[1]);
        if d >= left && d > 0.0 {
            return Some(w[0].lerp(w[1], left / d));
        }
        left -= d;
    }
    points.first().copied()
}

/// The point of `rect`'s border nearest `p`, or its nearest side's
/// middle height where `p` is beside it.
fn nearest_on(rect: Rectangle, p: Point) -> Point {
    let x = p.x.clamp(rect.x, rect.x + rect.width);
    let y = p.y.clamp(rect.y, rect.y + rect.height);
    if p.x < rect.x || p.x > rect.x + rect.width {
        Point::new(x, rect.center_y())
    } else {
        Point::new(x, y)
    }
}

/// The convex hull of `points`, anticlockwise on screen (y down), by the
/// monotone chain.
fn hull(points: &[DVec2]) -> Vec<DVec2> {
    let mut pts: Vec<_> = points.iter().copied().filter(|p| p.is_finite()).collect();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<DVec2> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<DVec2> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn inside_hull(hull: &[DVec2], p: DVec2) -> bool {
    hull.len() >= 3
        && (0..hull.len()).all(|i| {
            let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
            (b - a).perp_dot(p - a) >= 0.0
        })
}

/// Whether `rect` and the hull overlap: a sample of the rectangle's
/// points in the hull, or a hull point in the rectangle.
fn meets_hull(hull: &[DVec2], rect: Rectangle) -> bool {
    let (x0, y0) = (f64::from(rect.x), f64::from(rect.y));
    let (x1, y1) = (x0 + f64::from(rect.width), y0 + f64::from(rect.height));
    let samples = (0..=4).flat_map(|i| {
        let x = x0 + (x1 - x0) * f64::from(i) / 4.0;
        [
            DVec2::new(x, y0),
            DVec2::new(x, (y0 + y1) / 2.0),
            DVec2::new(x, y1),
        ]
    });
    samples.into_iter().any(|p| inside_hull(hull, p))
        || hull
            .iter()
            .any(|p| p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1)
}

/// Where each label goes: the free place nearest what it names, apart
/// from the labels placed before it, in the viewport, clear of the
/// status bar, and outside the bodies' outline `hull` unless there's
/// no room there. `None` for a label with no room at all.
fn place(wanted: &[Wanted], widths: &[f32], hull: &[DVec2], size: Size) -> Vec<Option<Rectangle>> {
    let room = Rectangle::new(
        Point::ORIGIN,
        Size::new(size.width, (size.height - STATUS_BAR_ROOM).max(0.0)),
    );
    let centre = if hull.is_empty() {
        DVec2::new(f64::from(size.width), f64::from(size.height)) / 2.0
    } else {
        hull.iter().sum::<DVec2>() / hull.len() as f64
    };

    // The outermost first: they have the shortest way out, and leave the
    // places nearer the model to those behind them.
    let mut order: Vec<usize> = (0..wanted.len()).collect();
    order.sort_by(|&a, &b| {
        let d = |i: usize| wanted[i].anchor.distance_squared(centre);
        d(b).total_cmp(&d(a))
    });

    let mut placed: Vec<Rectangle> = Vec::new();
    let mut out = vec![None; wanted.len()];
    let fits = |rect: &Rectangle, placed: &[Rectangle]| {
        rect.x >= room.x
            && rect.y >= room.y
            && rect.x + rect.width <= room.width
            && rect.y + rect.height <= room.height
            && !placed.iter().any(|o| o.expand(GAP).intersects(rect))
    };
    for i in order {
        let want = &wanted[i];
        let width = widths[i] + 2.0 * PAD + MARK_ROOM;
        let label = Size::new(width, LABEL_HEIGHT);
        let anchor = Point::new(want.anchor.x as f32, want.anchor.y as f32);
        let at = |c: Point| {
            Rectangle::new(
                Point::new(c.x - label.width / 2.0, c.y - label.height / 2.0),
                label,
            )
        };
        let allowed =
            |rect: &Rectangle, outside: bool| !outside || !meets_hull(hull, rect.expand(GAP));

        // Rings of places round the anchor, nearest first, the first free
        // one taken: outside the model if it has room, else anywhere.
        'passes: for outside in [true, false] {
            let mut r = 0.0f32;
            let most = size.width.max(size.height);
            while r <= most {
                let steps = if r == 0.0 {
                    1
                } else {
                    ((r * std::f32::consts::TAU / 8.0) as usize).clamp(8, 96)
                };
                let mut best: Option<(f32, Rectangle)> = None;
                for s in 0..steps {
                    let angle = s as f32 / steps as f32 * std::f32::consts::TAU;
                    // The label's nearest side `r` from the anchor.
                    let (dx, dy) = (angle.cos(), angle.sin());
                    let c = Point::new(
                        anchor.x + dx * (r + label.width / 2.0 * dx.abs().min(1.0)),
                        anchor.y + dy * (r + label.height / 2.0 * dy.abs().min(1.0)),
                    );
                    let rect = at(c);
                    if fits(&rect, &placed) && allowed(&rect, outside) {
                        let length = anchor.distance(nearest_on(rect, anchor));
                        if best.is_none_or(|(l, _)| length < l) {
                            best = Some((length, rect));
                        }
                    }
                }
                if let Some((_, rect)) = best {
                    placed.push(rect);
                    out[i] = Some(rect);
                    break 'passes;
                }
                r += if r < 40.0 { 4.0 } else { 10.0 };
            }
        }
    }
    out
}
