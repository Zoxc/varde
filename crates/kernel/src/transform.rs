//! Rigid motions of solids, and copies of them assembled into one.
//!
//! A [`Motion`] is an affine map `x ↦ L·x + t`, built as a move, a turn
//! about a line or a mirror in a plane, or composed of them
//! ([`Motion::then`]). [`Solid::transformed`] maps every vertex and edge
//! control point by it (weights stay: a rational curve's image under an
//! affine map is the curve of the mapped control points with the same
//! weights), each face's claim and form with it, and reverses every
//! triangle of a mirror, which would otherwise face in. Copies are named
//! by an [`Instance`]; a move keeps the names.
//!
//! [`assemble`] makes one solid of several, such as a pattern's copies:
//! those that can't meet are put side by side in one mesh, the others
//! unioned. Patterns place copy `k` directly ([`Motion::pattern_step`],
//! [`Motion::pattern_turn`]), never by composing `k` steps, so no error
//! piles up along them.
//!
//! Angles are in degrees, so that the quarter turns users type are exact:
//! a turn by a multiple of 90° takes its sine and cosine as `0` and `±1`,
//! not from [`trig`](crate::trig), and about a coordinate axis its matrix
//! has only those entries, so it maps coordinates to the bit. Other angles
//! go through [`trig`](crate::trig), the same bits on every platform.

use glam::{DMat3, DVec3};

use crate::boolean::{Op, boolean};
use crate::budget::Work;
use crate::mesh::{Edge, Face, FaceKey, FaceName, Form, Halfedge, Mesh, Quadric, Surface, Tri};
use crate::patch::Bounds3;
use crate::{Budget, KernelError, MAX_PATCHES, Solid, Tolerance, in_range};

/// Units of work a patch for [`Solid::transformed`], beside the check's
/// integration ([`Solid::new_within`]): mapping it and checking its hulls
/// and tags again, a few units, as a boolean's final check is charged.
const TRANSFORM_WORK: usize = 5;

/// An affine map of space, `x ↦ linear·x + offset`: so far always rigid
/// (a move, turn, mirror, or a composition of them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    linear: DMat3,
    /// How normals and the coefficients of planes and quadrics map: the
    /// inverse transpose of `linear`, kept beside it rather than worked
    /// out, so a turn's is its own matrix to the bit.
    normal: DMat3,
    offset: DVec3,
    /// Whether it turns space inside out (`linear`'s determinant is
    /// negative): an odd number of mirrors.
    mirrors: bool,
}

/// Which copy a [`Solid::transformed`] makes: copy `index` that feature
/// `feature` (a pattern or a mirror) makes. Its faces are named by
/// [`FaceName::copy`], so copies of copies stay unique and stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instance {
    pub feature: u64,
    pub index: u64,
}

impl Motion {
    /// The motion that moves nothing.
    pub const IDENTITY: Motion = Motion {
        linear: DMat3::IDENTITY,
        normal: DMat3::IDENTITY,
        offset: DVec3::ZERO,
        mirrors: false,
    };

    /// The move by `offset`; `None` if it isn't finite.
    pub fn translation(offset: DVec3) -> Option<Motion> {
        offset.is_finite().then_some(Motion {
            offset,
            ..Motion::IDENTITY
        })
    }

    /// The turn by `degrees` about the line through `point` along `axis`,
    /// counter-clockwise looking down `axis` (the right-hand rule).
    /// Multiples of 90° are exact (see the [module](self) docs); `None`
    /// for an angle or point that isn't finite, or a zero or non-finite
    /// axis.
    pub fn turn(point: DVec3, axis: DVec3, degrees: f64) -> Option<Motion> {
        if !point.is_finite() || !degrees.is_finite() {
            return None;
        }
        let k = axis.try_normalize()?;
        let (sin, cos) = sin_cos_degrees(degrees);
        // Rodrigues: cos·I + sin·[k]× + (1 − cos)·k·kᵀ.
        let cross = DMat3::from_cols(
            DVec3::new(0.0, k.z, -k.y),
            DVec3::new(-k.z, 0.0, k.x),
            DVec3::new(k.y, -k.x, 0.0),
        );
        let linear = DMat3::IDENTITY * cos + cross * sin + outer(k, k) * (1.0 - cos);
        Some(Motion {
            linear,
            normal: linear,
            offset: point - linear * point,
            mirrors: false,
        })
    }

    /// The mirror in the plane through `point` square to `normal`
    /// (`x ↦ x − 2·(n·(x − point))·n/|n|²`); exact in planes square to a
    /// coordinate axis. `None` for a point that isn't finite or a zero or
    /// non-finite normal.
    pub fn mirror(point: DVec3, normal: DVec3) -> Option<Motion> {
        let n = normal;
        let nn = n.length_squared();
        if !point.is_finite() || !(nn > 0.0 && nn.is_finite()) {
            return None;
        }
        // I − 2·n·nᵀ/|n|², symmetric and orthogonal: its own inverse
        // transpose.
        let linear = DMat3::IDENTITY - outer(n, n * (2.0 / nn));
        Some(Motion {
            linear,
            normal: linear,
            offset: n * (2.0 * n.dot(point) / nn),
            mirrors: true,
        })
    }

    /// Copy `k` of a linear pattern: the move by `k·spacing` along
    /// `direction` (any length), placed directly. `None` for a zero or
    /// non-finite direction, or an offset that isn't finite.
    pub fn pattern_step(direction: DVec3, spacing: f64, k: u32) -> Option<Motion> {
        let d = direction.try_normalize()?;
        Motion::translation(d * (f64::from(k) * spacing))
    }

    /// Copy `k` of a circular pattern of `count` copies spread evenly over
    /// `degrees` (360 for a full turn): the turn by `k·degrees/count`
    /// about the line through `point` along `axis`, placed directly.
    /// `None` for no copies, or as [`Motion::turn`].
    pub fn pattern_turn(
        point: DVec3,
        axis: DVec3,
        degrees: f64,
        k: u32,
        count: u32,
    ) -> Option<Motion> {
        if count == 0 {
            return None;
        }
        Motion::turn(point, axis, f64::from(k) * degrees / f64::from(count))
    }

    /// `self`, then `next`.
    pub fn then(&self, next: &Motion) -> Motion {
        Motion {
            linear: next.linear * self.linear,
            normal: next.normal * self.normal,
            offset: next.linear * self.offset + next.offset,
            mirrors: self.mirrors != next.mirrors,
        }
    }

    /// Where it takes the point `p`.
    pub fn point(&self, p: DVec3) -> DVec3 {
        self.linear * p + self.offset
    }

    /// Where it takes the direction `v` (a difference of points).
    pub fn vector(&self, v: DVec3) -> DVec3 {
        self.linear * v
    }

    /// Where it takes the normal `n` of a surface: square to the mapped
    /// surface, pointing to the image of the side `n` pointed to. Not
    /// normalized.
    pub fn normal(&self, n: DVec3) -> DVec3 {
        self.normal * n
    }

    /// Whether it turns space inside out: a mirror.
    pub fn mirrors(&self) -> bool {
        self.mirrors
    }

    /// The plane `n·x = d` mapped: `(n', d')` with `n'·x = d'` on the
    /// image, `n'` pointing to the image of the side `n` points to.
    fn plane(&self, n: DVec3, d: f64) -> (DVec3, f64) {
        let n2 = self.normal(n);
        (n2, d + n2.dot(self.offset))
    }

    /// The claim mapped.
    fn surface(&self, surface: Surface) -> Surface {
        match surface {
            Surface::Plane { n, d } => {
                let (n, d) = self.plane(n, d);
                Surface::Plane { n, d }
            }
            // F(x) = y·A·y + 2·b·y + c for y = x − origin; on the image
            // y = M·y' with M = linear⁻¹ = normalᵀ and y' = x' − origin'.
            Surface::Quadric(q) => Surface::Quadric(Quadric {
                origin: self.point(q.origin),
                a: self.normal * q.a * self.normal.transpose(),
                b: self.normal * q.b,
                c: q.c,
            }),
            Surface::Free => Surface::Free,
        }
    }

    /// The form mapped, for a rigid motion: lengths, radii and angles
    /// stay; points map as points, axes as directions; a plane's normal
    /// as a normal, so it still points out of the mapped solid (whose
    /// triangles a mirror reverses).
    fn form(&self, form: Form) -> Form {
        let unit = |v: DVec3| self.vector(v).normalize_or_zero();
        match form {
            Form::Unknown => Form::Unknown,
            Form::Plane { n, d } => {
                let (n, d) = self.plane(n, d);
                Form::plane(n, d)
            }
            Form::Cylinder {
                point,
                axis,
                radius,
            } => Form::Cylinder {
                point: self.point(point),
                axis: unit(axis),
                radius,
            },
            Form::ConicCylinder { conic, along } => Form::ConicCylinder {
                conic: crate::patch::Conic3 {
                    p0: self.point(conic.p0),
                    c: self.point(conic.c),
                    w: conic.w,
                    p1: self.point(conic.p1),
                },
                along: unit(along),
            },
            Form::Cone {
                apex,
                axis,
                cos,
                sin,
            } => Form::Cone {
                apex: self.point(apex),
                axis: unit(axis),
                cos,
                sin,
            },
            Form::Sphere { centre, radius } => Form::Sphere {
                centre: self.point(centre),
                radius,
            },
            Form::Torus {
                centre,
                axis,
                major,
                minor,
            } => Form::Torus {
                centre: self.point(centre),
                axis: unit(axis),
                major,
                minor,
            },
            // The meridian is drawn in a half-plane through the axis, in
            // distance from it and height along it: a rigid motion keeps
            // both.
            Form::Revolved {
                origin,
                axis,
                meridian,
            } => Form::Revolved {
                origin: self.point(origin),
                axis: unit(axis),
                meridian,
            },
        }
    }
}

/// The sine and cosine of `degrees`: exactly `0` and `±1` at multiples of
/// 90°, else through [`trig`](crate::trig) after reducing the angle to
/// `[−180°, 180°]` (the reductions by 360° and 90° are exact: `%` on
/// floats is).
fn sin_cos_degrees(degrees: f64) -> (f64, f64) {
    let mut r = degrees % 360.0;
    if r > 180.0 {
        r -= 360.0;
    } else if r < -180.0 {
        r += 360.0;
    }
    if r % 90.0 == 0.0 {
        // r is one of −180, −90, 0, 90, 180.
        return match r as i32 {
            90 => (1.0, 0.0),
            -90 => (-1.0, 0.0),
            180 | -180 => (0.0, -1.0),
            _ => (0.0, 1.0),
        };
    }
    crate::trig::sin_cos(r * (std::f64::consts::PI / 180.0))
}

/// `u·vᵀ`.
fn outer(u: DVec3, v: DVec3) -> DMat3 {
    DMat3::from_cols(u * v.x, u * v.y, u * v.z)
}

impl FaceKey {
    /// The key of copy `index` that `feature` makes of a face so keyed:
    /// what [`FaceName::copy`] makes of its names' keys.
    pub fn copy(self, feature: u64, index: u64) -> FaceKey {
        FaceKey {
            instance: crate::topology::mix(&[self.instance, feature, index]),
            ..self
        }
    }
}

impl Solid {
    /// The solid moved by `motion`, its faces renamed as copy `copy` (see
    /// [`Instance`]) or, with `None`, keeping their names (a move).
    ///
    /// Vertices and edge control points map by the motion, weights stay,
    /// and faces' claims and forms map with it, so exact claims stay
    /// exact to a rounding (to the bit for moves by representable
    /// offsets, quarter turns about coordinate axes and mirrors in
    /// coordinate planes, through the origin or points whose coordinates
    /// the arithmetic keeps). A mirror reverses every triangle's corners,
    /// so the copy faces out. A point mapped past
    /// [`MAX_COORD`](crate::MAX_COORD) is refused
    /// ([`KernelError::Patch`]), and the result passes `check` or fails
    /// with [`KernelError::Invalid`] (rounding can bring hulls a hair
    /// closer), within `budget`: a few units a patch, and the volumes the
    /// check integrates.
    pub fn transformed(
        &self,
        motion: &Motion,
        copy: Option<Instance>,
        tol: &Tolerance,
        budget: &Budget,
    ) -> Result<Solid, KernelError> {
        let mut work = Work::new(budget);
        let mesh = self.mesh();
        work.spend(mesh.tris().len().saturating_mul(TRANSFORM_WORK))?;
        let mapped = |p: DVec3| {
            let q = motion.point(p);
            in_range(q).map(|()| q)
        };
        let verts = mesh
            .verts()
            .iter()
            .map(|&p| mapped(p))
            .collect::<Result<Vec<_>, _>>()?;
        let edges = mesh
            .edges()
            .iter()
            .map(|e| {
                Ok(Edge {
                    ctrl: mapped(e.ctrl)?,
                    weight: e.weight,
                })
            })
            .collect::<Result<Vec<_>, crate::patch::PatchError>>()?;
        let tris = if motion.mirrors() {
            mesh.tris().iter().enumerate().map(reversed).collect()
        } else {
            mesh.tris().to_vec()
        };
        let name = |n: FaceName| match copy {
            Some(c) => n.copy(c.feature, c.index),
            None => n,
        };
        let faces = mesh
            .faces()
            .iter()
            .map(|f| Face {
                name: name(f.name),
                surface: motion.surface(f.surface),
                form: motion.form(f.form),
            })
            .collect();
        let aliases = mesh
            .aliases()
            .iter()
            .map(|&(f, key)| match copy {
                Some(c) => (f, key.copy(c.feature, c.index)),
                None => (f, key),
            })
            .collect();
        let mesh = Mesh::from_parts(verts, edges, tris, faces).with_aliases(aliases);
        Solid::new_within(mesh, tol, &mut work)
    }
}

/// Triangle `t` turned over: corners `[a, b, c]` become `[a, c, b]`. Its
/// halfedge `i` becomes halfedge `2 − i`, running the other way along the
/// same edge, so its pair is the pair's image.
fn reversed((t, tri): (usize, &Tri)) -> Tri {
    let image = |h: u32| h - h % 3 + (2 - h % 3);
    let [h0, h1, h2] = tri.halfedges;
    debug_assert_eq!(image(3 * t as u32), 3 * t as u32 + 2);
    let he = |start: u32, along: Halfedge| Halfedge {
        start,
        pair: image(along.pair),
        edge: along.edge,
    };
    Tri {
        halfedges: [he(h0.start, h2), he(h2.start, h1), he(h1.start, h0)],
        face: tri.face,
    }
}

/// One solid of `parts` (a pattern's copies, say), within `tol`.
///
/// Parts that can't meet are put side by side in one mesh, one shell (or
/// more) each, with no boolean: those whose boxes are more than the
/// resolution apart, and those whose boxes come closer but whose patches'
/// hulls are all more than the resolution apart with no vertex of either
/// in the other's box (a part inside another, or in its void, would have
/// all its vertices in that one's box). The others are unioned, each
/// connected group of them pairwise in a balanced tree by index (`0 ∪ 1`,
/// `2 ∪ 3`, … then those results the same way), each union within
/// `budget`. The groups' results are then put side by side in index order
/// of their lowest parts and checked, with `budget` for that and the
/// grouping, so a part nested where no box shows it (in a void that only
/// several other parts close) fails the check
/// ([`KernelError::Invalid`]) rather than give a wrong solid.
///
/// The work is linear in the patches when the parts' boxes are apart,
/// as a pattern's spaced copies are. No parts, or only empty ones, give
/// the empty solid; one gives itself.
pub fn assemble(parts: &[Solid], tol: &Tolerance, budget: &Budget) -> Result<Solid, KernelError> {
    let mut work = Work::new(budget);
    let solids: Vec<&Solid> = parts.iter().filter(|s| !s.is_empty()).collect();
    let boxes: Vec<Bounds3> = solids.iter().filter_map(|s| s.bounds3()).collect();
    let margin = tol.resolution();
    let tree = crate::mesh::Bvh::new(boxes.clone());
    let mut links = Vec::new();
    for [i, j] in tree.self_pairs_within(margin, &mut work)? {
        let (a, b) = (solids[i as usize], solids[j as usize]);
        if !apart(
            a,
            b,
            &boxes[i as usize],
            &boxes[j as usize],
            margin,
            &mut work,
        )? {
            links.push([i, j]);
        }
    }
    // Each part's group as its lowest part; the groups' members in
    // ascending order, the groups by their lowest.
    let group = crate::boolean::parts(solids.len(), links);
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); solids.len()];
    for (i, &g) in group.iter().enumerate() {
        members[g as usize].push(i);
    }
    let mut results = Vec::new();
    for members in members.into_iter().filter(|m| !m.is_empty()) {
        let mut level: Vec<Solid> = members.iter().map(|&i| solids[i].clone()).collect();
        while level.len() > 1 {
            let mut next = Vec::with_capacity(level.len().div_ceil(2));
            let (pairs, rest) = level.as_chunks::<2>();
            for [a, b] in pairs {
                next.push(boolean(a, b, Op::Union, tol, budget)?);
            }
            next.extend(rest.iter().cloned());
            level = next;
        }
        results.extend(level);
    }
    match results.len() {
        0 => Ok(Solid::empty()),
        1 => Ok(results.pop().expect("one result")),
        _ => {
            let mesh = side_by_side(&results)?;
            work.spend(mesh.tris().len().saturating_mul(TRANSFORM_WORK))?;
            Solid::new_within(mesh, tol, &mut work)
        }
    }
}

/// Whether `a` and `b`, with boxes `ba` and `bb` within `margin` of each
/// other, can go side by side in one mesh: no vertex of either in the
/// other's box, and every two of their patches' hulls more than `margin`
/// apart. One unit of `work` a vertex and a patch pair looked at.
fn apart(
    a: &Solid,
    b: &Solid,
    ba: &Bounds3,
    bb: &Bounds3,
    margin: f64,
    work: &mut Work,
) -> Result<bool, KernelError> {
    let inside =
        |p: &DVec3, bx: &Bounds3| p.cmpge(bx.min - margin).all() && p.cmple(bx.max + margin).all();
    work.spend(
        a.mesh()
            .verts()
            .len()
            .saturating_add(b.mesh().verts().len()),
    )?;
    if a.mesh().verts().iter().any(|p| inside(p, bb))
        || b.mesh().verts().iter().any(|p| inside(p, ba))
    {
        return Ok(false);
    }
    let patches_b: Vec<_> = (0..b.mesh().tris().len())
        .map(|t| b.mesh().patch(t))
        .collect();
    let tree = crate::mesh::Bvh::new(patches_b.iter().map(|p| p.bounds()).collect());
    let patches_a: Vec<_> = (0..a.mesh().tris().len())
        .map(|t| a.mesh().patch(t))
        .collect();
    let ids: Vec<u32> = (0..patches_a.len() as u32).collect();
    let pairs = tree.hits_within(
        &ids,
        |i| patches_a[i as usize].bounds(),
        margin,
        |_, _| true,
        work,
    )?;
    Ok(pairs.iter().all(|&[i, j]| {
        crate::mesh::apart(
            &patches_a[i as usize].hull(),
            &patches_b[j as usize].hull(),
            margin,
        )
    }))
}

/// The meshes of `solids` in one, side by side: each one's vertices,
/// edges, triangles, faces and aliases after the ones before, numbered
/// on. Unchecked; [`KernelError::TooComplex`] past
/// [`MAX_PATCHES`] patches.
fn side_by_side(solids: &[Solid]) -> Result<Mesh, KernelError> {
    let total = solids
        .iter()
        .try_fold(0usize, |n, s| n.checked_add(s.mesh().tris().len()))
        .filter(|&n| n <= MAX_PATCHES)
        .ok_or(KernelError::TooComplex)?;
    let (mut verts, mut edges, mut faces, mut aliases) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut tris = Vec::with_capacity(total);
    for solid in solids {
        let mesh = solid.mesh();
        // Each count is at most three times MAX_PATCHES, as is their sum.
        let (v0, e0, f0) = (verts.len() as u32, edges.len() as u32, faces.len() as u32);
        let h0 = 3 * tris.len() as u32;
        verts.extend_from_slice(mesh.verts());
        edges.extend_from_slice(mesh.edges());
        faces.extend_from_slice(mesh.faces());
        aliases.extend(mesh.aliases().iter().map(|&(f, key)| (f + f0, key)));
        tris.extend(mesh.tris().iter().map(|tri| Tri {
            halfedges: tri.halfedges.map(|h| Halfedge {
                start: h.start + v0,
                pair: h.pair + h0,
                edge: h.edge + e0,
            }),
            face: tri.face + f0,
        }));
    }
    Ok(Mesh::from_parts(verts, edges, tris, faces).with_aliases(aliases))
}

#[cfg(test)]
mod tests;
