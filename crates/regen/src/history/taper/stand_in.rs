//! The stand-in for the kernel's taper that tests run in its place
//! until the kernel's is built (see `crate::testing`).

use super::*;

/// A stand-in for the kernel's tapered extrude, for tests: a rectangle
/// along the frame's axes (one loop of four lines), over a span on one
/// side of the sketch's plane (touching it or not), is the untapered
/// extrude's box with each corner moved across the normal by
/// `|h| tan taper` towards the middle (away from it for a negative
/// taper), `h` its height above the plane: a frustum of a pyramid, each
/// wall still flat and keeping its name. One closing within the
/// resolution before the span's far end is [`TaperError::Closes`], one
/// past [`MAX_COORD`](varde_kernel_compat::MAX_COORD)
/// [`TaperError::OutOfRange`]; a zero taper is the extrude; anything
/// else (a span across the plane, whose walls the kernel splits there,
/// another profile) is [`KernelError::TooComplex`]. The result passes
/// the kernel's check as every solid does.
#[allow(clippy::too_many_arguments)]
pub(crate) fn by_frustum(
    profile: &Profile,
    frame: &Frame,
    from: f64,
    to: f64,
    taper: f64,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, TaperError> {
    use glam::{DVec2, DVec3};
    use varde_kernel_compat::mesh::{Edge, Form, Mesh, Surface};
    let too_complex = || TaperError::Failed(KernelError::TooComplex.into());
    let solid = varde_kernel_compat::extrude(profile, frame, from, to, feature, tol, budget)?;
    if taper == 0.0 {
        return Ok(solid);
    }
    let lines = (profile.loops.iter()).all(|lp| {
        lp.segments.len() == 4
            && (lp.segments.iter()).all(|segment| {
                let c = segment.conic;
                (c.c - (c.p0 + c.p1) / 2.0).length() <= 1e-12 * (1.0 + c.p0.distance(c.p1))
                    && (c.p0.x == c.p1.x || c.p0.y == c.p1.y)
            })
    });
    if profile.loops.len() != 1 || !lines || (from < 0.0 && to > 0.0) {
        return Err(too_complex());
    }
    let mesh = solid.mesh();
    if mesh.verts().len() != 8 {
        return Err(too_complex());
    }
    let normal = frame.normal();
    // A point in the frame: its (x, y) and height.
    let local = |p: DVec3| {
        let r = p - frame.origin;
        (DVec2::new(r.dot(frame.x), r.dot(frame.y)), r.dot(normal))
    };
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for &v in mesh.verts() {
        let (p, _) = local(v);
        (lo, hi) = (lo.min(p), hi.max(p));
    }
    let (sin, cos) = varde_kernel_compat::trig::sin_cos(taper);
    let tan = sin / cos;
    // The width left at the far end, the farthest from the plane.
    let far = from.abs().max(to.abs());
    let least = tol.resolution();
    if (hi - lo).min_element() - 2.0 * tan * far <= least {
        return Err(TaperError::Closes);
    }
    let middle = (lo + hi) / 2.0;
    let verts: Vec<DVec3> = (mesh.verts().iter())
        .map(|&v| {
            let (p, h) = local(v);
            let inward = tan * h.abs();
            let q = p - (p - middle).signum() * inward;
            frame.origin + frame.x * q.x + frame.y * q.y + normal * h
        })
        .collect();
    let reach = f64::from(varde_kernel_compat::MAX_COORD);
    if verts
        .iter()
        .any(|v| !v.is_finite() || v.abs().max_element() > reach)
    {
        return Err(TaperError::OutOfRange);
    }
    let mut edges = mesh.edges().to_vec();
    for tri in mesh.tris() {
        for (i, half) in tri.halfedges.iter().enumerate() {
            let end = tri.halfedges[(i + 1) % 3].start;
            edges[half.edge as usize] =
                Edge::straight(verts[half.start as usize], verts[end as usize]);
        }
    }
    let mut faces = mesh.faces().to_vec();
    for (index, face) in faces.iter_mut().enumerate() {
        let Form::Plane { n: old, .. } = face.form else {
            return Err(too_complex());
        };
        // A wall's new plane, through one of its triangles, facing out
        // as it did (it leans by under a right angle); a cap's stays.
        if old.dot(normal).abs() > 0.5 {
            continue;
        }
        let tri = (mesh.tris().iter())
            .find(|tri| tri.face as usize == index)
            .ok_or_else(too_complex)?;
        let [a, b, c] = tri.halfedges.map(|half| verts[half.start as usize]);
        let mut n = (b - a)
            .cross(c - a)
            .try_normalize()
            .ok_or_else(too_complex)?;
        if n.dot(old) < 0.0 {
            n = -n;
        }
        let d = n.dot(a);
        face.form = Form::Plane { n, d };
        face.surface = Surface::Plane { n, d };
    }
    let tapered = Mesh::from_parts(verts, edges, mesh.tris().to_vec(), faces)
        .with_aliases(mesh.aliases().to_vec());
    Solid::new(tapered, tol).map_err(|error| TaperError::Failed(error.into()))
}
