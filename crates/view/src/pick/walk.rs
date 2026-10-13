//! Rays and boxes: where a ray meets a triangle or passes through a box,
//! and the walks of a [`Bvh`] picking with one.

use glam::DVec3;
use varde_regen::{Bvh, BvhNode};

/// A ray: the points `origin + direction * t`, those with `t` at least
/// `from` showing.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ray {
    pub(super) origin: DVec3,
    pub(super) direction: DVec3,
    pub(super) from: f64,
}

impl Ray {
    pub(super) fn at(&self, t: f64) -> DVec3 {
        self.origin + self.direction * t
    }
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

/// The corners of `bounds`, as [`through_box`] takes them.
pub(crate) fn aabb(bounds: varde_kernel_compat::Aabb) -> [DVec3; 2] {
    [bounds.min.as_dvec3(), bounds.max.as_dvec3()]
}

/// Where the ray from `origin` along `direction` is inside the box `min`
/// to `max`, as how far along it it goes in and comes out, within `from`
/// and `to`, if it is.
pub(crate) fn through_box(
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

/// The walks of a [`Bvh`] a ray picks with.
pub(super) trait Walk {
    /// The item `hit` says the ray meets first between `from` and `to`,
    /// and where: `hit` gives where along the ray an item is met, if it
    /// is.
    fn nearest(
        &self,
        ray: &Ray,
        from: f64,
        to: f64,
        hit: impl FnMut(u32) -> Option<f64>,
    ) -> Option<(f64, u32)>;

    /// Visits each item whose box, grown by `reach` of the box, the ray
    /// passes through from its `from` on: `visit` says nothing back that
    /// matters.
    fn near(
        &self,
        ray: &Ray,
        reach: impl Fn(DVec3, DVec3) -> f64,
        visit: impl FnMut(u32) -> Option<()>,
    );
}

impl Walk for Bvh {
    fn nearest(
        &self,
        ray: &Ray,
        from: f64,
        to: f64,
        mut hit: impl FnMut(u32) -> Option<f64>,
    ) -> Option<(f64, u32)> {
        let nodes = self.nodes();
        let mut best: Option<(f64, u32)> = None;
        let mut stack = Vec::new();
        if !nodes.is_empty() {
            stack.push(0usize);
        }
        while let Some(index) = stack.pop() {
            let node = &nodes[index];
            let limit = best.map_or(to, |(t, _)| t);
            let Some(_) = through_box(ray.origin, ray.direction, node_box(node), from, limit)
            else {
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
                let bounds = node_box(&nodes[child]);
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

    fn near(
        &self,
        ray: &Ray,
        reach: impl Fn(DVec3, DVec3) -> f64,
        mut visit: impl FnMut(u32) -> Option<()>,
    ) {
        let nodes = self.nodes();
        let mut stack = Vec::new();
        if !nodes.is_empty() {
            stack.push(0usize);
        }
        while let Some(index) = stack.pop() {
            let node = &nodes[index];
            let [min, max] = node_box(node);
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
}

/// A node's box, as [`through_box`] takes it.
fn node_box(node: &BvhNode) -> [DVec3; 2] {
    aabb(node.aabb())
}
