//! Picking the model shown in the viewport: which face, edge and body is
//! under the cursor, on the CPU, against the mesh the viewport draws and
//! its picking tables ([`Picking`]); no GPU picking. A [`PickIndex`]
//! holds a bounding volume hierarchy over the mesh's triangles and one
//! over its feature edges. The cursor's ray meets the nearest triangle,
//! whose face is picked; but a feature edge within [`EDGE_REACH`] pixels
//! of the cursor on screen, not hidden by what's in front of it (a ray
//! from the eye to its point nearest the cursor meets nothing nearer by
//! more than [`HIDDEN_PULL`] view heights), wins over the face. A body is
//! the body of the face or edge picked.
//!
//! The index also builds the [`Highlight`] of picked faces and edges
//! from the mesh's triangles and edges.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{DVec2, DVec3, Vec3};
use varde_document::{BodyId, OriginPlane, Plane};
use varde_kernel::RenderMesh;
use varde_regen::Picking;
use varde_render::{Camera, Emphasis, Highlight};

use crate::projection::Projector;

/// How near the cursor an edge must show to be picked, in pixels.
pub const EDGE_REACH: f64 = 6.0;

/// How much nearer the eye than an edge's point, in view heights, what's
/// in front of it must be to hide it: as far as the renderer pulls the
/// edges towards the camera, so an edge the faces either side of it
/// would hide but for that pull isn't hidden here either.
const HIDDEN_PULL: f64 = 0.002;

/// The most edges near the cursor looked at for one that isn't hidden,
/// nearest first: the rest are as good as hidden.
const MAX_EDGE_TESTS: usize = 64;

/// The most triangles or edges in a leaf of a hierarchy.
const LEAF: usize = 4;

/// A face or an edge of the model shown: an index into its picking
/// tables' [`Picking::faces`] or [`Picking::chains`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Picked {
    Face(u32),
    Edge(u32),
}

/// What's under the cursor in the model shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pick {
    /// Which model's tables `target` is an index into: the
    /// [`PickIndex::model`] of the index that picked it.
    pub model: u64,
    pub target: Picked,
    /// The body of the face or edge.
    pub body: BodyId,
    /// Where on it: where the cursor's ray meets the face, or the edge's
    /// point that shows nearest the cursor. In world coordinates, as the
    /// mesh is drawn.
    pub at: DVec3,
}

/// The model shown, made ready for picking: its mesh, its tables, and
/// hierarchies over the mesh's triangles and feature edges.
#[derive(Debug)]
pub struct PickIndex {
    mesh: Arc<RenderMesh>,
    picking: Arc<Picking>,
    model: u64,
    /// Over the mesh's triangles, by index.
    triangles: Bvh,
    /// Over the mesh's edges on a chain, by index into the mesh's edges.
    edges: Bvh,
    /// Each face's triangles.
    face_triangles: Groups,
    /// Each chain's edges.
    chain_edges: Groups,
}

impl PickIndex {
    /// The index of `mesh` and its tables `picking`, which came with it;
    /// its picks carry `model`. Tables that don't go with the mesh pick
    /// nothing.
    pub fn new(mesh: Arc<RenderMesh>, picking: Arc<Picking>, model: u64) -> Self {
        let fits = picking.triangles().len() == mesh.triangle_count()
            && picking.edges().len() == mesh.edges().len();
        let (triangles, edges, face_triangles, chain_edges) = if fits {
            let corners = |triangle: &[u32; 3]| triangle.map(|i| position(&mesh, i));
            let triangles = mesh.indices().as_chunks::<3>().0;
            let boxes = triangles.iter().map(|t| bounds(&corners(t)));
            let on_chains = |&(_, &chain): &(usize, &u32)| chain != Picking::NONE;
            let edges: Vec<u32> = (picking.edges().iter().enumerate())
                .filter(on_chains)
                .filter_map(|(edge, _)| u32::try_from(edge).ok())
                .collect();
            let edge_boxes = edges.iter().map(|&edge| {
                let [a, b] = mesh.edges()[edge as usize];
                bounds(&[position(&mesh, a), position(&mesh, b)])
            });
            (
                Bvh::new(boxes.collect()),
                Bvh::with_items(edge_boxes.collect(), edges),
                Groups::new(picking.faces().len(), picking.triangles()),
                Groups::new(picking.chains().len(), picking.edges()),
            )
        } else {
            Default::default()
        };
        Self {
            mesh,
            picking,
            model,
            triangles,
            edges,
            face_triangles,
            chain_edges,
        }
    }

    /// Which model its picks name: see [`Pick::model`].
    pub fn model(&self) -> u64 {
        self.model
    }

    pub fn mesh(&self) -> &Arc<RenderMesh> {
        &self.mesh
    }

    pub fn picking(&self) -> &Arc<Picking> {
        &self.picking
    }

    /// The body of `target`, if it's in the tables.
    pub fn body(&self, target: Picked) -> Option<BodyId> {
        let face = match target {
            Picked::Face(face) => face,
            Picked::Edge(chain) => self.picking.chains().get(chain as usize)?.faces[0],
        };
        Some(self.picking.faces().get(face as usize)?.body)
    }

    /// What's under the screen position `at`, in logical pixels from the
    /// top left of a viewport `size` big seen by `camera`: a feature edge
    /// showing within [`EDGE_REACH`] of it that nothing hides, the
    /// nearest, or else the face the cursor's ray first meets.
    pub fn pick(&self, camera: &Camera, size: [f32; 2], at: DVec2) -> Option<Pick> {
        let placement = Plane::Origin(OriginPlane::XY).placement();
        let projector = Projector::new(camera, placement, size[0], size[1])?;
        let ray = self.ray(camera, &projector, at)?;
        let face = self.first_hit(&ray, ray.from, f64::INFINITY);
        let edge = self.edge_near(camera, &projector, &ray, at);
        let (target, at) = match (edge, face) {
            (Some((chain, at)), _) => (Picked::Edge(chain), at),
            (None, Some((t, triangle))) => {
                let face = *self.picking.triangles().get(triangle as usize)?;
                (Picked::Face(face), ray.at(t))
            }
            (None, None) => return None,
        };
        let pick = Pick {
            model: self.model,
            target,
            body: self.body(target)?,
            at,
        };
        Some(pick)
    }

    /// The highlight of `items`: each face's triangles and each edge's
    /// lines in its emphasis. Items not in the tables are left out.
    pub fn highlight(&self, items: impl IntoIterator<Item = (Picked, Emphasis)>) -> Highlight {
        let mut highlight = Highlight::default();
        for (target, emphasis) in items {
            match target {
                Picked::Face(face) => {
                    for &triangle in self.face_triangles.get(face) {
                        let Some(corners) = self.corners(triangle) else {
                            continue;
                        };
                        let normal = |i: u32| {
                            let normal = self.mesh.normals().get(i as usize).copied();
                            Vec3::from(normal.unwrap_or_default())
                        };
                        let positions = corners.map(|i| position(&self.mesh, i));
                        highlight.triangle(emphasis, positions, corners.map(normal));
                    }
                }
                Picked::Edge(chain) => {
                    let segments = (self.chain_edges.get(chain).iter())
                        .filter_map(|&edge| self.mesh.edges().get(edge as usize))
                        .map(|&[a, b]| [a, b].map(|i| position(&self.mesh, i)));
                    for polyline in polylines(segments.collect()) {
                        highlight.edge(emphasis, polyline);
                    }
                }
            }
        }
        highlight
    }

    /// The corners of the mesh's triangle `triangle`, if it has one.
    fn corners(&self, triangle: u32) -> Option<[u32; 3]> {
        let start = (triangle as usize).checked_mul(3)?;
        let corners = self.mesh.indices().get(start..start.checked_add(3)?)?;
        Some([corners[0], corners[1], corners[2]])
    }

    /// The cursor's ray at `at`, from where the model may show along it.
    fn ray(&self, camera: &Camera, projector: &Projector, at: DVec2) -> Option<Ray> {
        let (origin, direction) = projector.ray(at)?;
        if projector.perspective() {
            // At the target's depth `t` is 1, so a depth `t` times the
            // camera's distance.
            let distance = f64::from(camera.distance());
            return Some(Ray {
                origin,
                direction,
                from: projector.near() / distance,
            });
        }
        // An orthographic view sees what's behind its eye too: from far
        // enough back that the whole mesh is ahead.
        let direction = direction.try_normalize()?;
        let back = self.back(origin);
        Some(Ray {
            origin: origin - direction * back,
            direction,
            from: 0.0,
        })
    }

    /// How far back from `origin` a ray must start to have the whole mesh
    /// ahead of it, whichever way it goes.
    fn back(&self, origin: DVec3) -> f64 {
        self.mesh.bounds().map_or(0.0, |bounds| {
            let (min, max) = (bounds.min.as_dvec3(), bounds.max.as_dvec3());
            (max - min).length() + (origin - (min + max) / 2.0).length()
        })
    }

    /// Where along `ray` between `from` and `to` it first meets a
    /// triangle, and which, if it does.
    fn first_hit(&self, ray: &Ray, from: f64, to: f64) -> Option<(f64, u32)> {
        self.triangles.nearest(ray, from, to, |triangle| {
            let corners = self.corners(triangle)?;
            let corners = corners.map(|i| position(&self.mesh, i).as_dvec3());
            ray_hits(ray.origin, ray.direction, corners)
        })
    }

    /// The edge showing nearest `at` within [`EDGE_REACH`] that nothing
    /// hides, of those [`MAX_EDGE_TESTS`] nearest, and its point showing
    /// nearest `at`: its chain and that point.
    fn edge_near(
        &self,
        camera: &Camera,
        projector: &Projector,
        ray: &Ray,
        at: DVec2,
    ) -> Option<(u32, DVec3)> {
        // An edge showing within reach is within that many pixels of the
        // ray, at its depth, so within a box grown by that much at its
        // deepest.
        let reach = |min: DVec3, max: DVec3| {
            let (_, backward) = projector.eye();
            let half = (max - min) / 2.0;
            let deepest = projector.world_depth((min + max) / 2.0) + half.dot(backward.abs());
            EDGE_REACH * projector.pixel_at(deepest)
        };
        let mut near = Vec::new();
        self.edges.near(ray, reach, |edge| {
            let [a, b] = self
                .mesh
                .edges()
                .get(edge as usize)?
                .map(|i| position(&self.mesh, i));
            let (a, b) = projector.in_front(a.as_dvec3(), b.as_dvec3())?;
            let (pa, pb) = (projector.show(a), projector.show(b));
            let (distance, s) = segment_distance(at, pa, pb);
            if distance.is_nan() || distance > EDGE_REACH {
                return None;
            }
            // Perspective divides by depth, so the point showing `s` of
            // the way along isn't `s` of the way along in the world.
            let u = if projector.perspective() {
                let (da, db) = (projector.world_depth(a), projector.world_depth(b));
                let denominator = (1.0 - s) * db + s * da;
                if denominator > 0.0 {
                    s * da / denominator
                } else {
                    s
                }
            } else {
                s
            };
            let point = a + (b - a) * u.clamp(0.0, 1.0);
            near.push((distance, projector.world_depth(point), edge, point));
            Some(())
        });
        near.sort_by(|a, b| {
            (a.0.total_cmp(&b.0))
                .then(a.1.total_cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        let view_height = f64::from(camera.view_height());
        near.into_iter()
            .take(MAX_EDGE_TESTS)
            .find(|&(_, _, _, point)| !self.hidden(projector, point, view_height))
            .and_then(|(_, _, edge, point)| {
                let chain = *self.picking.edges().get(edge as usize)?;
                (chain != Picking::NONE).then_some((chain, point))
            })
    }

    /// Whether something of the mesh is in front of the world point
    /// `point`, nearer the eye by more than [`HIDDEN_PULL`] view heights
    /// and the mesh's `f32` rounding.
    fn hidden(&self, projector: &Projector, point: DVec3, view_height: f64) -> bool {
        let scale = self.mesh.bounds().map_or(0.0, |bounds| {
            f64::from(bounds.min.abs().max(bounds.max.abs()).max_element())
        });
        let slack = HIDDEN_PULL * view_height + 8.0 * f64::from(f32::EPSILON) * scale;
        let (eye, backward) = projector.eye();
        if projector.perspective() {
            // At `point` `t` is 1, so a depth `t` times its depth.
            let depth = projector.world_depth(point);
            if depth.is_nan() || depth <= slack {
                return false;
            }
            let ray = Ray {
                origin: eye,
                direction: point - eye,
                from: 0.0,
            };
            let from = projector.near() / depth;
            let to = 1.0 - slack / depth;
            self.first_hit(&ray, from, to).is_some()
        } else {
            let back = self.back(point);
            let ray = Ray {
                origin: point + backward * back,
                direction: -backward,
                from: 0.0,
            };
            self.first_hit(&ray, 0.0, back - slack).is_some()
        }
    }
}

/// A ray: the points `origin + direction * t`, those with `t` at least
/// `from` showing.
#[derive(Debug, Clone, Copy)]
struct Ray {
    origin: DVec3,
    direction: DVec3,
    from: f64,
}

impl Ray {
    fn at(&self, t: f64) -> DVec3 {
        self.origin + self.direction * t
    }
}

/// The mesh's vertex `index`, or the origin if it has none.
fn position(mesh: &RenderMesh, index: u32) -> Vec3 {
    Vec3::from(
        mesh.positions()
            .get(index as usize)
            .copied()
            .unwrap_or_default(),
    )
}

/// The box around `points`.
fn bounds(points: &[Vec3]) -> [Vec3; 2] {
    let first = points.first().copied().unwrap_or_default();
    (points.iter()).fold([first; 2], |[min, max], &p| [min.min(p), max.max(p)])
}

/// How far `p` is from the segment from `a` to `b`, and where along it
/// the nearest point is, as a fraction of the way from `a`.
fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> (f64, f64) {
    let ab = b - a;
    let length = ab.length_squared();
    let s = if length > 0.0 {
        ((p - a).dot(ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.distance(a + ab * s), s)
}

/// The segments joined end to end into polylines where they meet, by
/// their ends' bits: a polyline closing on itself ends where it starts.
/// In the order of their first segments, each running the way its first
/// segment does.
fn polylines(segments: Vec<[Vec3; 2]>) -> Vec<Vec<Vec3>> {
    let key = |p: Vec3| p.to_array().map(f32::to_bits);
    let mut at: BTreeMap<[u32; 3], Vec<usize>> = BTreeMap::new();
    for (i, segment) in segments.iter().enumerate() {
        for &end in segment {
            at.entry(key(end)).or_default().push(i);
        }
    }
    let mut used = vec![false; segments.len()];
    // The unused segment at `p`, and its other end.
    let next = |p: Vec3, used: &mut Vec<bool>| {
        let i = *at.get(&key(p))?.iter().find(|&&i| !used[i])?;
        used[i] = true;
        let [a, b] = segments[i];
        Some(if key(a) == key(p) { b } else { a })
    };
    let mut polylines = Vec::new();
    for i in 0..segments.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let [a, b] = segments[i];
        let mut forward = vec![a, b];
        let mut end = b;
        while let Some(p) = next(end, &mut used) {
            forward.push(p);
            end = p;
        }
        let mut backward = Vec::new();
        let mut start = a;
        while let Some(p) = next(start, &mut used) {
            backward.push(p);
            start = p;
        }
        backward.reverse();
        backward.extend(forward);
        polylines.push(backward);
    }
    polylines
}

/// How far along the ray from `origin` along `direction` it meets the
/// triangle `corners`, either side of it, if it does (Möller–Trumbore).
pub(crate) fn ray_hits(origin: DVec3, direction: DVec3, [a, b, c]: [DVec3; 3]) -> Option<f64> {
    let (ab, ac) = (b - a, c - a);
    let p = direction.cross(ac);
    let determinant = ab.dot(p);
    if determinant.abs() <= f64::EPSILON * ab.length() * ac.length() * direction.length() {
        return None;
    }
    let s = origin - a;
    let u = s.dot(p) / determinant;
    let q = s.cross(ab);
    let v = direction.dot(q) / determinant;
    let inside = (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0;
    let t = ac.dot(q) / determinant;
    (inside && t.is_finite()).then_some(t)
}

/// Where the ray from `origin` along `direction` is inside the box `min`
/// to `max`, as how far along it it goes in and comes out, within `from`
/// and `to`, if it is.
fn through_box(
    origin: DVec3,
    direction: DVec3,
    [min, max]: [DVec3; 2],
    from: f64,
    to: f64,
) -> Option<(f64, f64)> {
    let (mut near, mut far) = (from, to);
    for axis in 0..3 {
        let (o, d) = (origin[axis], direction[axis]);
        if d == 0.0 {
            if o < min[axis] || o > max[axis] {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((min[axis] - o) / d, (max[axis] - o) / d);
        near = near.max(t0.min(t1));
        far = far.min(t0.max(t1));
    }
    (near <= far).then_some((near, far))
}

/// Items grouped by what they belong to: a face's triangles, a chain's
/// edges.
#[derive(Debug, Default)]
struct Groups {
    /// Where each group's items start in `items`, and the end.
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Groups {
    /// The items of `groups` groups, item `i` in group `of[i]`, unless
    /// that's past the groups ([`Picking::NONE`]). None if there are more
    /// items than `u32`s number, which no mesh has.
    fn new(groups: usize, of: &[u32]) -> Self {
        if u32::try_from(of.len()).is_err() {
            return Self::default();
        }
        let group = |g: u32| Some(g as usize).filter(|&g| g < groups);
        // Counts, then running sums: no more than `of.len()`, a `u32`.
        let mut starts = vec![0u32; groups + 1];
        for g in of.iter().filter_map(|&g| group(g)) {
            starts[g + 1] += 1;
        }
        for g in 0..groups {
            starts[g + 1] += starts[g];
        }
        let mut filled = starts.clone();
        let mut items = vec![0u32; starts[groups] as usize];
        for (item, g) in of.iter().enumerate() {
            if let Some(g) = group(*g) {
                items[filled[g] as usize] = item as u32;
                filled[g] += 1;
            }
        }
        Self { starts, items }
    }

    /// The items of `group`, none if there's no such group.
    fn get(&self, group: u32) -> &[u32] {
        let group = group as usize;
        let (Some(&start), Some(&end)) = (self.starts.get(group), self.starts.get(group + 1))
        else {
            return &[];
        };
        self.items.get(start as usize..end as usize).unwrap_or(&[])
    }
}

/// A bounding volume hierarchy over items with boxes: split in two at the
/// median of their middles along the box's longest side, until a few
/// are left. Built sequentially, the same for the same items.
#[derive(Debug, Default)]
struct Bvh {
    nodes: Vec<Node>,
    /// The items in the leaves' order.
    items: Vec<u32>,
}

/// A node of a [`Bvh`]: its box, and its leaf's items or its children.
#[derive(Debug, Clone, Copy)]
struct Node {
    min: Vec3,
    max: Vec3,
    /// A leaf's first item in [`Bvh::items`], or an inner node's second
    /// child; its first comes right after it.
    start: u32,
    /// A leaf's number of items, 0 for an inner node.
    count: u32,
}

impl Bvh {
    /// Over items `0..boxes.len()`, each in its box.
    fn new(boxes: Vec<[Vec3; 2]>) -> Self {
        let items = (0..boxes.len())
            .filter_map(|i| u32::try_from(i).ok())
            .collect();
        Self::with_items(boxes, items)
    }

    /// Over `items`, the `i`th in `boxes[i]`.
    fn with_items(boxes: Vec<[Vec3; 2]>, items: Vec<u32>) -> Self {
        let mut order: Vec<usize> = (0..boxes.len().min(items.len())).collect();
        let mut nodes = Vec::new();
        if !order.is_empty() {
            build(&boxes, &mut order, 0, &mut nodes);
        }
        let items = order.into_iter().map(|i| items[i]).collect();
        Self { nodes, items }
    }

    /// The item `hit` says the ray meets first between `from` and `to`,
    /// and where: `hit` gives where along the ray an item is met, if it
    /// is.
    fn nearest(
        &self,
        ray: &Ray,
        from: f64,
        to: f64,
        mut hit: impl FnMut(u32) -> Option<f64>,
    ) -> Option<(f64, u32)> {
        let mut best: Option<(f64, u32)> = None;
        let mut stack = Vec::new();
        if !self.nodes.is_empty() {
            stack.push(0usize);
        }
        while let Some(index) = stack.pop() {
            let node = self.nodes[index];
            let limit = best.map_or(to, |(t, _)| t);
            let Some(_) = through_box(ray.origin, ray.direction, node.bounds(), from, limit) else {
                continue;
            };
            if node.count > 0 {
                for &item in self.leaf(node) {
                    if let Some(t) = hit(item).filter(|&t| t >= from && t <= to)
                        && best.is_none_or(|(b, i)| (t, item) < (b, i))
                    {
                        best = Some((t, item));
                    }
                }
                continue;
            }
            // The nearer child first, so the further is more often cut.
            let (first, second) = (index + 1, node.start as usize);
            let enter = |child: usize| {
                let bounds = self.nodes[child].bounds();
                through_box(ray.origin, ray.direction, bounds, from, limit).map(|(t, _)| t)
            };
            let (a, b) = (enter(first), enter(second));
            match (a, b) {
                (Some(a), Some(b)) if b < a => stack.extend([first, second]),
                _ => stack.extend([second, first]),
            }
        }
        best
    }

    /// Visits each item whose box, grown by `reach` of the box, the ray
    /// passes through from its `from` on: `visit` says nothing back that
    /// matters.
    fn near(
        &self,
        ray: &Ray,
        reach: impl Fn(DVec3, DVec3) -> f64,
        mut visit: impl FnMut(u32) -> Option<()>,
    ) {
        let mut stack = Vec::new();
        if !self.nodes.is_empty() {
            stack.push(0usize);
        }
        while let Some(index) = stack.pop() {
            let node = self.nodes[index];
            let [min, max] = node.bounds();
            let grow = DVec3::splat(reach(min, max));
            let grown = [min - grow, max + grow];
            if through_box(ray.origin, ray.direction, grown, ray.from, f64::INFINITY).is_none() {
                continue;
            }
            if node.count > 0 {
                for &item in self.leaf(node) {
                    let _ = visit(item);
                }
            } else {
                stack.extend([node.start as usize, index + 1]);
            }
        }
    }

    /// A leaf's items.
    fn leaf(&self, node: Node) -> &[u32] {
        let start = node.start as usize;
        self.items
            .get(start..start.saturating_add(node.count as usize))
            .unwrap_or(&[])
    }
}

impl Node {
    fn bounds(&self) -> [DVec3; 2] {
        [self.min.as_dvec3(), self.max.as_dvec3()]
    }
}

/// Builds the nodes over `order`, the items `offset..` of the whole, onto
/// `nodes`, reordering `order` into the leaves' order.
fn build(boxes: &[[Vec3; 2]], order: &mut [usize], offset: usize, nodes: &mut Vec<Node>) {
    let [min, max] = (order.iter()).fold([Vec3::INFINITY, Vec3::NEG_INFINITY], |[lo, hi], &i| {
        [lo.min(boxes[i][0]), hi.max(boxes[i][1])]
    });
    let index = nodes.len();
    // Items and nodes number fewer than the mesh's triangles, whose
    // indices are `u32`s.
    let at = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    nodes.push(Node {
        min,
        max,
        start: at(offset),
        count: at(order.len()),
    });
    if order.len() <= LEAF {
        return;
    }
    let middle = |i: usize| boxes[i][0] + boxes[i][1];
    let [lo, hi] = (order.iter()).fold([Vec3::INFINITY, Vec3::NEG_INFINITY], |[lo, hi], &i| {
        [lo.min(middle(i)), hi.max(middle(i))]
    });
    let axis = (hi - lo).max_position();
    let half = order.len() / 2;
    order.select_nth_unstable_by(half, |&a, &b| {
        (middle(a)[axis].total_cmp(&middle(b)[axis])).then(a.cmp(&b))
    });
    let (left, right) = order.split_at_mut(half);
    build(boxes, left, offset, nodes);
    let second = nodes.len();
    build(boxes, right, offset + half, nodes);
    nodes[index].start = at(second);
    nodes[index].count = 0;
}

#[cfg(test)]
mod tests;
