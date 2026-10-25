//! Primitives: the box, the cylinder, the cone (or frustum), the sphere
//! and the torus, built as exact forms on their lattices, named and
//! tagged, and checked ([`primitive`]).
//!
//! **Placement.** Each primitive stands in the world frame, its axis
//! along `+z`: a box between two corners, a cylinder or cone on the
//! circle about its base centre rising along `+z`, a sphere or torus
//! about its centre. A [`Motion`](crate) moves it elsewhere.
//!
//! **Faces and keys.** Every face's key is `Primitive(k)` of the caller's
//! [`OpCtx`], `k` the face's part of the primitive:
//! - box: 0 the bottom (`−z`), 1 the top (`+z`), 2 to 5 the sides
//!   counter-clockwise seen from `+z` from the one facing `−y` (`−y`,
//!   `+x`, `+y`, `−x`);
//! - cylinder and cone: 0 the bottom cap, 1 the top cap (none at a
//!   cone's apex), 2 the wall;
//! - sphere and torus: 0, the whole surface.
//!
//! A curved part is cut into the faces of its form's canonical surfaces
//! ([`crate::geom::forms`]): its form's 0° and 180° lattice lines cut it
//! into two faces of 180°, and a ring torus's tube is cut again at its
//! highest and lowest circles into its outer and inner halves. The faces
//! of one part share its key; the edges between them are
//! [`SameForm`](EdgeKind::SameForm), tagged [`EdgeTag::Seam`], of
//! continuity `Curvature`. The other edges are 90° convex creases. A
//! sphere's poles and a spindle torus's apexes are tagged
//! [`VertTag::Pole`] (`N` towards `+z`), a cone's apex [`VertTag::Apex`],
//! each face reaching one closing there at its own singular edge.
//!
//! **Exact geometry.** Every key is the form's from the given
//! dimensions ([`crate::geom::form`]), its surfaces the lattice's tiles.
//! The circles a sphere's or torus's faces meet at are their tiles' rows,
//! bit for bit, as are their meridians; a cylinder's or cone's circles
//! are the quarter arcs of its lattice's angle cells at the circle's
//! height (`1, √½, 1`), each control point the exact point rounded once,
//! and its rulings the lines between their ends. A circle's p-curves are
//! iso lines on the walls and the circle mapped to the cap plane's UV; a
//! box's edges are lines with linear p-curves. A position in a face's UV
//! is the exact value rounded once. The seam bounds certify the rest
//! ([`Builder::seam_bounds`], towards [`seam_target`]), so a ribbon is
//! the rounding of the geometry. An edge above `ε_seam = ε/4` is listed
//! among the status's warnings ([`Accuracy`]): a cone with an apex far
//! enough off has one, its rows' rounding at the apex's distance.
//!
//! **Input** (decided exactly on the given values). A value that isn't
//! finite, or a point of the primitive's box beyond `MAX_COORD`, is
//! [`Storage::OutOfRange`]. A size, height, diameter, or a ring torus's
//! hole or a spindle torus's apex distance not above
//! `2·(1 + 2⁻⁸)·ε_exc` (`ε_exc = ε/8`, extrude's least extent), and radii
//! that make no form of the kind (a cone's equal radii, a torus's equal
//! radii, a spindle too close to a horn torus for its tiles' weights),
//! are [`FeatureKind::Dimension`].
//!
//! **Shells and regions.** One lump: the faces' inner sides its outer
//! shell, their outer sides an island of the unbounded region.
//!
//! [`Accuracy`]: crate::WarningKind::Accuracy

use malachite::Rational;
use malachite::base::num::arithmetic::traits::Abs;
use varde_decide::bezier::Dir;
use varde_decide::num::exact::rat as q;
use varde_decide::num::storage::{OperandScan, eps_min, nearest};

use crate::geom::curve::{Curve3D, Span3};
use crate::geom::form::{ConeKey, CylinderKey, FormError, PlaneKey, SphereKey, TorusKey};
use crate::geom::forms::{cyl, torus as ring};
use crate::geom::lattice::{CELL, Lattice, LatticeError, Level, Rect};
use crate::geom::pcurve::{PCurve, PSpan};
use crate::geom::surface::Surface;
use crate::infra::budget::{Budget, Work};
use crate::infra::error::{FeatureKind, Inconsistency, Internal, KernelError, Storage};
use crate::infra::ops::{OpCtx, Status, Tolerance, Warning, WarningKind};
use crate::topo::brep::{
    BodyKind, Brep, Continuity, Convexity, EdgeId, EdgeKind, EdgeTag, ExactRows, FaceId, FaceKey,
    FacePart, PoleEnd, RegionId, RegionKind, ShellId, ShellKind, Side, VertId, VertTag,
};
use crate::topo::build::{BuildError, Builder, Use};
use crate::topo::check::{Outcome, Role, Scope};

#[cfg(test)]
mod tests;

/// A primitive and its dimensions (mm), in the world frame (module
/// docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    /// The box between `min` and `min + size`.
    Cuboid {
        /// Its least corner.
        min: [f64; 3],
        /// Its extent along each axis.
        size: [f64; 3],
    },
    /// The cylinder on the circle of `radius` about `base`, rising
    /// `height` along `+z`.
    Cylinder {
        /// The centre of its bottom.
        base: [f64; 3],
        /// Its radius.
        radius: f64,
        /// Its height.
        height: f64,
    },
    /// The cone or frustum on the circle of `radii[0]` about `base`,
    /// rising `height` along `+z` to the circle of `radii[1]`; a radius
    /// 0 is an apex.
    Cone {
        /// The centre of its bottom.
        base: [f64; 3],
        /// Its bottom and top radii.
        radii: [f64; 2],
        /// Its height.
        height: f64,
    },
    /// The sphere of `radius` about `centre`.
    Sphere {
        /// Its centre.
        centre: [f64; 3],
        /// Its radius.
        radius: f64,
    },
    /// The torus about the line through `centre` along `+z`: its tube of
    /// radius `minor` about the circle of radius `major` through
    /// `centre`. A ring torus for `major > minor`, a spindle (its outer
    /// part) for `major < minor`.
    Torus {
        /// Its centre.
        centre: [f64; 3],
        /// The radius of its tube's centre circle.
        major: f64,
        /// The radius of its tube.
        minor: f64,
    },
}

/// The bottom cap's part number.
pub const BOTTOM: u32 = 0;
/// The top cap's part number.
pub const TOP: u32 = 1;
/// A cylinder's or cone's wall's part number, and a box's first side's.
pub const WALL: u32 = 2;
/// A sphere's or torus's part number.
pub const WHOLE: u32 = 0;

/// The primitive `p`, its faces keyed for `ctx`, on tolerance `eps` and
/// a budget of `budget`, checked as a constructed body (module docs).
/// Its check's failure is its outcome for a constructed body
/// ([`crate::topo::check::CheckError::outcome`]).
pub fn primitive(
    p: &Primitive,
    ctx: OpCtx,
    eps: Tolerance,
    budget: &Budget,
) -> Result<Brep, KernelError> {
    let mut work = Work::new(budget);
    let body = build(p, ctx, eps, &mut work)?;
    match body.check_in(Scope::Full, None, &mut work, 0) {
        Ok(()) => Ok(body),
        Err(e) => Err(match e.outcome(Role::Constructed) {
            Outcome::Error(e) => e,
            Outcome::Accuracy | Outcome::LadderSignal => defect(),
        }),
    }
}

/// The seam bounds' target at a primitive's largest coordinate magnitude
/// `m`: the least tolerance storable there, so that a ribbon is its
/// geometry's rounding (none where that is exact) and its measures hold
/// to 1e-12.
pub fn seam_target(m: f64) -> f64 {
    eps_min(m).unwrap_or(f64::MIN_POSITIVE)
}

/// The primitive `p`, unchecked (module docs), its seam bounds charged
/// to `work`.
pub(crate) fn build(
    p: &Primitive,
    ctx: OpCtx,
    eps: Tolerance,
    work: &mut Work,
) -> Result<Brep, KernelError> {
    let [lo, hi] = extent(p)?;
    let mut scan = OperandScan::new();
    scan.bounded(&lo)?;
    scan.bounded(&hi)?;
    let start = scan.start(eps.eps())?;
    let least = least_extent(eps);
    let mut m = Make {
        b: Builder::new(BodyKind::Solid, Status::new(eps, &start)),
        ctx,
        target: seam_target(start.max_coord),
        accuracy: eps.eps() / 4.0,
        work,
        faces: Vec::new(),
    };
    match *p {
        Primitive::Cuboid { min, size: _ } => m.cuboid(min, hi, &least)?,
        Primitive::Cylinder {
            base,
            radius,
            height: _,
        } => {
            above(&(q(2.0) * q(radius)), &least)?;
            let top = [base[0], base[1], hi[2]];
            m.cylinder(base, top, radius, &least)?;
        }
        Primitive::Cone {
            base,
            radii,
            height: _,
        } => {
            let top = [base[0], base[1], hi[2]];
            m.cone(base, top, radii, &least)?;
        }
        Primitive::Sphere { centre, radius } => {
            above(&(q(2.0) * q(radius)), &least)?;
            m.sphere(centre, radius)?;
        }
        Primitive::Torus {
            centre,
            major,
            minor,
        } => m.torus(centre, major, minor, &least)?,
    }
    m.finish()
}

/// The primitive's box `[lo, hi]` from its finite dimensions, as stored
/// (a box's far corner and a cylinder's or cone's top rounded once);
/// `OutOfRange` for a value that isn't finite.
fn extent(p: &Primitive) -> Result<[[f64; 3]; 2], KernelError> {
    let out = || KernelError::Unrepresentable(Storage::OutOfRange);
    let finite = |xs: &[f64]| xs.iter().all(|x| x.is_finite());
    let around = |c: [f64; 3], r: [f64; 3]| {
        [
            [0, 1, 2].map(|k| c[k] - r[k]),
            [0, 1, 2].map(|k| c[k] + r[k]),
        ]
    };
    let upright = |base: [f64; 3], r: f64, h: f64| {
        let [lo, hi] = around(base, [r, r, 0.0]);
        [lo, [hi[0], hi[1], base[2] + h]]
    };
    let b = match *p {
        Primitive::Cuboid { min, size } => {
            if !(finite(&min) && finite(&size)) {
                return Err(out());
            }
            [min, [0, 1, 2].map(|k| min[k] + size[k])]
        }
        Primitive::Cylinder {
            base,
            radius,
            height,
        } => {
            if !(finite(&base) && finite(&[radius, height])) {
                return Err(out());
            }
            upright(base, radius.abs(), height)
        }
        Primitive::Cone {
            base,
            radii,
            height,
        } => {
            if !(finite(&base) && finite(&radii) && height.is_finite()) {
                return Err(out());
            }
            let (a, b) = (radii[0].abs(), radii[1].abs());
            let r = if a > b { a } else { b };
            upright(base, r, height)
        }
        Primitive::Sphere { centre, radius } => {
            if !(finite(&centre) && radius.is_finite()) {
                return Err(out());
            }
            around(centre, [radius.abs(); 3])
        }
        Primitive::Torus {
            centre,
            major,
            minor,
        } => {
            if !(finite(&centre) && finite(&[major, minor])) {
                return Err(out());
            }
            let r = major.abs() + minor.abs();
            around(centre, [r, r, minor.abs()])
        }
    };
    if b.iter().flatten().all(|x| x.is_finite()) {
        Ok(b)
    } else {
        Err(out())
    }
}

/// `2·(1 + 2⁻⁸)·ε_exc` with `ε_exc = ε/8`: the least extent a primitive
/// takes, exactly (module docs).
fn least_extent(eps: Tolerance) -> Rational {
    q(eps.eps()) * Rational::from_unsigneds(257u32, 1024u32)
}

/// `Ok` if `d` is above `least`, else `Dimension`.
fn above(d: &Rational, least: &Rational) -> Result<(), KernelError> {
    if d > least { Ok(()) } else { Err(dimension()) }
}

fn dimension() -> KernelError {
    KernelError::Feature(FeatureKind::Dimension)
}

/// A primitive's own defect: a construction the validated input should
/// always allow failed.
pub(super) fn defect() -> KernelError {
    KernelError::Internal(Internal::Inconsistent(Inconsistency::ResultDefect))
}

/// A key the validated input should always give: `OutOfRange` where a
/// parameter rounds past the range, else the primitive's defect.
pub(super) fn form<K>(k: Result<K, FormError>) -> Result<K, KernelError> {
    k.map_err(|e| match e {
        FormError::Range | FormError::NotFinite => {
            KernelError::Unrepresentable(Storage::OutOfRange)
        }
        _ => defect(),
    })
}

/// A lattice or surface: `OutOfRange` where a value rounds past the
/// range, else the primitive's defect.
pub(super) fn lattice<T>(r: Result<T, LatticeError>) -> Result<T, KernelError> {
    r.map_err(|e| match e {
        LatticeError::Range => KernelError::Unrepresentable(Storage::OutOfRange),
        _ => defect(),
    })
}

/// A curve the construction should always give.
fn curve<T, E>(r: Result<T, E>) -> Result<T, KernelError> {
    r.map_err(|_| defect())
}

/// A builder's refusal: running out of budget as it is, else the
/// primitive's defect.
pub(super) fn built(e: BuildError) -> KernelError {
    match e {
        BuildError::OutOfBudget(e) => e,
        _ => defect(),
    }
}

/// The direction `k` quarter turns from `e₁` towards `e₂`.
fn dir(e: &[[f64; 3]; 3], k: i64) -> [Rational; 3] {
    let (d, s) = match k.rem_euclid(4) {
        0 => (e[0], 1.0),
        1 => (e[1], 1.0),
        2 => (e[0], -1.0),
        _ => (e[1], -1.0),
    };
    d.map(|x| q(x * s))
}

/// `c + r·d`, rounded once per coordinate.
fn at(c: &[f64; 3], r: &Rational, d: &[Rational; 3]) -> [f64; 3] {
    [0, 1, 2].map(|k| nearest(&(q(c[k]) + r * &d[k])))
}

/// The circle of radius `r` about `c` in the frame `e`, from quarter
/// turn `from` to `from + 2` (counter-clockwise about `e₃`): two quarter
/// arcs, each control point rounded once, knots `0, 1, 2`.
fn half_circle(c: &[f64; 3], e: &[[f64; 3]; 3], r: f64, from: i64) -> Curve3D {
    let r = q(r);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let spans = (from..from + 2)
        .map(|k| {
            let (a, b) = (dir(e, k), dir(e, k + 1));
            let m = [0, 1, 2].map(|i| &a[i] + &b[i]);
            let p = |d: &[Rational; 3], w: f64| {
                let x = at(c, &r, d);
                [x[0], x[1], x[2], w]
            };
            [p(&a, 1.0), p(&m, w), p(&b, 1.0)]
        })
        .collect();
    Curve3D::new(spans, vec![0.0, 1.0, 2.0]).expect("two quarter arcs join")
}

/// The line from `a` to `b`, its middle control point rounded, knots
/// `0, 1`.
pub(super) fn line(a: [f64; 3], b: [f64; 3]) -> Curve3D {
    let m = [0, 1, 2].map(|k| nearest(&((q(a[k]) + q(b[k])) / Rational::from(2u32))));
    let pt = |p: [f64; 3]| [p[0], p[1], p[2], 1.0];
    Curve3D::new(vec![[pt(a), pt(m), pt(b)]], vec![0.0, 1.0]).expect("a line")
}

/// The curve of the rows `rows`, knots `0..n`.
fn chain(rows: Vec<Span3>) -> Curve3D {
    let knots = (0..=rows.len()).map(|k| k as f64).collect();
    Curve3D::new(rows, knots).expect("tile rows join")
}

/// The point `p`: a curve of two spans at it, knots `0, 1, 2`.
fn point(p: [f64; 3]) -> Curve3D {
    let x = [p[0], p[1], p[2], 1.0];
    Curve3D::new(vec![[x; 3], [x; 3]], vec![0.0, 1.0, 2.0]).expect("a point")
}

/// The curve run the other way, its knots (integers `0..n`) kept.
fn reversed(c: &Curve3D) -> Curve3D {
    let mut spans = c.spans().to_vec();
    spans.reverse();
    for s in &mut spans {
        s.reverse();
    }
    Curve3D::new(spans, c.knots().to_vec()).expect("a reversed chain")
}

/// The p-curve run the other way, over the same knots, which are
/// symmetric about their middle (integers from 0).
fn reversed_p(p: &PCurve) -> PCurve {
    let spans = p.spans().iter().rev().map(PSpan::reversed).collect();
    PCurve::from_spans(spans, p.knots().to_vec()).expect("a reversed chain")
}

/// The iso p-curve with `fixed` at `at`, the other coordinate running
/// from `a` to `b` linearly over `[0, t]`.
fn iso(fixed: Dir, at: f64, a: f64, b: f64, t: f64) -> PCurve {
    let span = PSpan::Iso {
        fixed,
        at,
        m: [[a, 1.0], [b, 1.0]],
    };
    PCurve::from_spans(vec![span], vec![0.0, t]).expect("an iso span")
}

/// A plane's global UV of `x`: `(x − o)·axis/CELL + ½` per axis, exact
/// and rounded once.
pub(super) fn plane_uv(l: &Lattice, x: &[f64; 3]) -> [f64; 2] {
    let (o, axes) = (l.origin(), l.axes());
    let half = Rational::from_signeds(1i32, 2i32);
    let cell = q(CELL);
    [axes[0], axes[1]].map(|a| {
        let along: Rational = (0..3).map(|k| (q(x[k]) - q(o[k])) * q(a[k])).sum();
        nearest(&(along / &cell + &half))
    })
}

/// The curve `c` on a plane: each control point mapped to its UV, its
/// weight kept, as Q spans over `c`'s knots.
fn on_plane(l: &Lattice, c: &Curve3D) -> PCurve {
    let spans = c
        .spans()
        .iter()
        .map(|s| {
            s.map(|h| {
                let [u, v] = plane_uv(l, &[h[0], h[1], h[2]]);
                [u, v, h[3]]
            })
        })
        .collect();
    PCurve::new(spans, c.knots().to_vec()).expect("a mapped chain")
}

/// The dot product of `a` and `b`, exactly.
fn dot(a: &[f64; 3], b: &[f64; 3]) -> Rational {
    (0..3).map(|k| q(a[k]) * q(b[k])).sum()
}

/// What an edge is to its two faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sort {
    /// Between faces of different forms: a 90° convex crease.
    Crease,
    /// Between faces of one key and form.
    Seam,
    /// A pole's or apex's side.
    Singular,
}

/// An edge as built: its id, its curve in the direction it was given,
/// and whether it is stored the other way (its ends' ids decreasing).
#[derive(Clone, Debug)]
struct E {
    id: EdgeId,
    curve: Curve3D,
    flip: bool,
}

/// A use of an edge by a loop: along its given direction or against,
/// its p-curve over the given direction.
struct Co<'e> {
    e: &'e E,
    along: bool,
    pcurve: PCurve,
}

/// One end of a curved wall: a circle cut in two at quarter turns 0 and
/// 2 (its halves from 0 to 2 and from 2 to 4), or a pole or apex, at
/// the row `v` of the wall's UV.
enum End<'e> {
    Circle { v: f64, halves: [&'e E; 2] },
    Pole { v: f64, vert: VertId, pos: [f64; 3] },
}

impl End<'_> {
    fn v(&self) -> f64 {
        match self {
            End::Circle { v, .. } | End::Pole { v, .. } => *v,
        }
    }
}

/// A primitive being built.
struct Make<'w> {
    b: Builder,
    ctx: OpCtx,
    target: f64,
    /// `ε_seam = ε/4`: an edge above it is an accuracy item.
    accuracy: f64,
    work: &'w mut Work,
    faces: Vec<FaceId>,
}

impl Make<'_> {
    fn vert(&mut self, pos: [f64; 3]) -> Result<VertId, KernelError> {
        self.b.add_vert(pos).map_err(built)
    }

    /// An edge on `curve` from `from` to `to`, of sort `sort`.
    fn edge(
        &mut self,
        curve: Curve3D,
        from: VertId,
        to: VertId,
        sort: Sort,
    ) -> Result<E, KernelError> {
        let flip = from > to;
        let stored = if flip {
            reversed(&curve)
        } else {
            curve.clone()
        };
        let k = stored.knots();
        let range = [k[0], k[k.len() - 1]];
        let c = self.b.add_curve(stored).map_err(built)?;
        let id = self
            .b
            .add_edge(c, range, if flip { [to, from] } else { [from, to] })
            .map_err(built)?;
        let (kind, tag, cont) = match sort {
            Sort::Crease => (
                EdgeKind::Regular,
                None,
                Continuity::Crease(Convexity::Convex),
            ),
            Sort::Seam => (
                EdgeKind::SameForm { touch: false },
                Some(EdgeTag::Seam),
                Continuity::Curvature,
            ),
            Sort::Singular => (EdgeKind::Singular, None, Continuity::default()),
        };
        self.b.set_edge_kind(id, kind);
        self.b.set_edge_tag(id, tag);
        self.b.set_cont(id, cont);
        Ok(E { id, curve, flip })
    }

    /// A singular edge at the pole `vert` at `pos`.
    fn singular(&mut self, vert: VertId, pos: [f64; 3]) -> Result<E, KernelError> {
        self.edge(point(pos), vert, vert, Sort::Singular)
    }

    /// A face of part `part` on `surface`, `flipped` if its outward
    /// normal is against the surface's, with the loop `uses`.
    fn face(
        &mut self,
        surface: Surface,
        flipped: bool,
        part: u32,
        uses: &[Co<'_>],
    ) -> Result<(), KernelError> {
        let s = self
            .b
            .add_surface(surface, None, ExactRows::default())
            .map_err(built)?;
        let key = FaceKey::new(self.ctx, FacePart::Primitive(part));
        let f = self.b.add_face(s, flipped, key).map_err(built)?;
        let mut pairs = Vec::with_capacity(uses.len());
        for u in uses {
            let p = if u.e.flip {
                reversed_p(&u.pcurve)
            } else {
                u.pcurve.clone()
            };
            let p = curve(p.split_at_tile_lines())?;
            pairs.push((u.e.id, self.b.add_pcurve(p).map_err(built)?));
        }
        let bounds = self
            .b
            .seam_bounds(s, &pairs, self.target, self.work)
            .map_err(built)?;
        let uses: Vec<Use> = uses
            .iter()
            .zip(pairs)
            .zip(bounds)
            .map(|((u, (edge, pcurve)), seam_bound)| Use {
                edge,
                reversed: u.along == u.e.flip,
                pcurve,
                seam_bound,
            })
            .collect();
        self.b.add_loop(f, true, &uses).map_err(built)?;
        self.faces.push(f);
        Ok(())
    }

    /// The two faces of part `part` on the 180° surfaces `surfaces`
    /// (quarter turns 0 to 2 and 2 to 4) between the ends `lo` and `hi`
    /// (`lo` at the lesser `v`), joined at the meridians `meridians`
    /// (quarter turns 0 and 2, given from `lo` to `hi`). Each face's loop
    /// runs counter-clockwise in its UV.
    fn wall<'e>(
        &mut self,
        surfaces: [Surface; 2],
        lo: &End<'e>,
        hi: &End<'e>,
        meridians: [&'e E; 2],
        part: u32,
    ) -> Result<(), KernelError> {
        let (v0, v1) = (lo.v(), hi.v());
        let mt = |e: &E| *e.curve.knots().last().expect("knots");
        for (h, surface) in surfaces.into_iter().enumerate() {
            let (u0, u1) = ((2 * h) as f64, (2 * h + 2) as f64);
            let mut poles = Vec::new();
            for end in [lo, hi] {
                if let End::Pole { vert, pos, .. } = end {
                    poles.push(self.singular(*vert, *pos)?);
                }
            }
            let mut poles = poles.iter();
            let mut row = |end: &End<'e>, along: bool| -> Co<'_> {
                let (e, v) = match end {
                    End::Circle { v, halves } => (halves[h], *v),
                    End::Pole { v, .. } => (poles.next().expect("a pole edge"), *v),
                };
                Co {
                    e,
                    along,
                    pcurve: iso(Dir::V, v, u0, u1, 2.0),
                }
            };
            let (first, third) = (row(lo, true), row(hi, false));
            let (right, left) = (meridians[(h + 1) % 2], meridians[h]);
            let uses = [
                first,
                Co {
                    e: right,
                    along: true,
                    pcurve: iso(Dir::U, u1, v0, v1, mt(right)),
                },
                third,
                Co {
                    e: left,
                    along: false,
                    pcurve: iso(Dir::U, u0, v0, v1, mt(left)),
                },
            ];
            self.face(surface, false, part, &uses)?;
        }
        Ok(())
    }

    /// A planar cap of part `part` through the centre `c`, perpendicular
    /// to the walls' axis `a`, its outward normal `n` (`±a`), bounded by
    /// the circle `halves` (counter-clockwise about `a`).
    fn cap(
        &mut self,
        c: [f64; 3],
        a: [f64; 3],
        n: [f64; 3],
        halves: [&E; 2],
        part: u32,
    ) -> Result<(), KernelError> {
        let key = form(PlaneKey::new(a, c))?;
        let l = lattice(Lattice::new(key.into()))?;
        let surface = lattice(Surface::new(l.clone(), lattice(l.half(0))?))?;
        let flipped = dot(&key.normal(), &n) < 0;
        let along = dot(&a, &n) > 0;
        let order = if along { [0, 1] } else { [1, 0] };
        let uses: Vec<Co<'_>> = order
            .iter()
            .map(|&h| Co {
                e: halves[h],
                along,
                pcurve: on_plane(&l, &halves[h].curve),
            })
            .collect();
        self.face(surface, flipped, part, &uses)
    }

    /// The box from `lo` to `hi` (stored corners).
    fn cuboid(&mut self, lo: [f64; 3], hi: [f64; 3], least: &Rational) -> Result<(), KernelError> {
        for k in 0..3 {
            above(&(q(hi[k]) - q(lo[k])), least)?;
        }
        // Corner `i` has `x` from bit 0, `y` from bit 1, `z` from bit 2.
        let corner = |i: usize| [0, 1, 2].map(|k| if i >> k & 1 == 0 { lo[k] } else { hi[k] });
        let verts = (0..8)
            .map(|i| self.vert(corner(i)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut edges = Vec::new();
        for i in 0..8 {
            for k in 0..3 {
                if i >> k & 1 == 0 {
                    let j = i | 1 << k;
                    let e =
                        self.edge(line(corner(i), corner(j)), verts[i], verts[j], Sort::Crease)?;
                    edges.push(((i, j), e));
                }
            }
        }
        // Each side counter-clockwise about its outward normal.
        let sides: [([f64; 3], [usize; 4]); 6] = [
            ([0.0, 0.0, -1.0], [0, 2, 3, 1]),
            ([0.0, 0.0, 1.0], [4, 5, 7, 6]),
            ([0.0, -1.0, 0.0], [0, 1, 5, 4]),
            ([1.0, 0.0, 0.0], [1, 3, 7, 5]),
            ([0.0, 1.0, 0.0], [2, 6, 7, 3]),
            ([-1.0, 0.0, 0.0], [0, 4, 6, 2]),
        ];
        for (part, (n, ring)) in sides.into_iter().enumerate() {
            let key = form(PlaneKey::new(n, corner(ring[0])))?;
            let l = lattice(Lattice::new(key.into()))?;
            let surface = lattice(Surface::new(l.clone(), lattice(l.half(0))?))?;
            let flipped = dot(&key.normal(), &n) < 0;
            let uses: Vec<Co<'_>> = (0..4)
                .map(|k| {
                    let (a, b) = (ring[k], ring[(k + 1) % 4]);
                    let (i, j) = (a.min(b), a.max(b));
                    let e = &edges
                        .iter()
                        .find(|(ij, _)| *ij == (i, j))
                        .expect("an edge")
                        .1;
                    let [p, r] = [corner(i), corner(j)].map(|x| plane_uv(&l, &x));
                    let pcurve = PCurve::from_spans(
                        vec![PSpan::Linear([[p[0], p[1], 1.0], [r[0], r[1], 1.0]])],
                        vec![0.0, 1.0],
                    )
                    .expect("a segment");
                    Co {
                        e,
                        along: a < b,
                        pcurve,
                    }
                })
                .collect();
            self.face(surface, flipped, part as u32, &uses)?;
        }
        Ok(())
    }

    /// The cylinder from the circle about `bottom` to the one about `top`
    /// (stored centres), of radius `r`.
    fn cylinder(
        &mut self,
        bottom: [f64; 3],
        top: [f64; 3],
        r: f64,
        least: &Rational,
    ) -> Result<(), KernelError> {
        above(&(q(top[2]) - q(bottom[2])), least)?;
        let key = form(CylinderKey::new(bottom, [0.0, 0.0, 1.0], r))?;
        let surfaces = [0, 2].map(|first| lattice(cyl::cylinder(key, first)));
        let [s0, s2] = surfaces;
        let surfaces = [s0?, s2?];
        let l = surfaces[0].lattice();
        let e = l.axes();
        let foot = key.foot();
        let v = |c: &[f64; 3]| {
            let along: Rational = (0..3).map(|k| (q(c[k]) - q(foot[k])) * q(e[2][k])).sum();
            nearest(&(along / q(CELL) + Rational::from_signeds(1i32, 2i32)))
        };
        let (v0, v1) = (v(&bottom), v(&top));
        self.revolved(surfaces, [bottom, top], [r, r], [v0, v1], e, [true, true])
    }

    /// The wall of revolution between the circles about `centres` (along
    /// the axis `e₃` of the frame `e`, at rows `vs` of the wall's UV,
    /// increasing) of `radii` (0 for an apex), on the 180° `surfaces`,
    /// and the caps on the circles where `caps` says: the bottom cap
    /// facing `−z`, the top `+z`.
    fn revolved(
        &mut self,
        surfaces: [Surface; 2],
        centres: [[f64; 3]; 2],
        radii: [f64; 2],
        vs: [f64; 2],
        e: [[f64; 3]; 3],
        caps: [bool; 2],
    ) -> Result<(), KernelError> {
        // The circles' ends (quarter turns 0 and 2), or the apex.
        let mut ends = Vec::new();
        for (c, r) in centres.iter().zip(radii) {
            if r == 0.0 {
                let pos = surfaces[0]
                    .row(Dir::U, 0, 0)
                    .map(|row| [row[0][0], row[0][1], row[0][2]])
                    .ok_or_else(defect)?;
                let vert = self.b.add_pole(pos, VertTag::Apex).map_err(built)?;
                ends.push((None, [vert, vert], [pos, pos]));
            } else {
                let halves = [0, 2].map(|k| half_circle(c, &e, r, k));
                let p = |c: &Curve3D| {
                    let x = c.spans()[0][0];
                    [x[0], x[1], x[2]]
                };
                let pos = [p(&halves[0]), p(&halves[1])];
                let verts = [self.vert(pos[0])?, self.vert(pos[1])?];
                ends.push((Some(halves), verts, pos));
            }
        }
        let mut circles = Vec::new();
        for (halves, verts, _) in &ends {
            circles.push(match halves {
                Some([h0, h2]) => Some([
                    self.edge(h0.clone(), verts[0], verts[1], Sort::Crease)?,
                    self.edge(h2.clone(), verts[1], verts[0], Sort::Crease)?,
                ]),
                None => None,
            });
        }
        let meridians = [0, 1].map(|k| {
            let (a, b) = (ends[0].2[k], ends[1].2[k]);
            (line(a, b), ends[0].1[k], ends[1].1[k])
        });
        let mut ms = Vec::new();
        for (c, from, to) in meridians {
            ms.push(self.edge(c, from, to, Sort::Seam)?);
        }
        let end = |i: usize| match &circles[i] {
            Some([a, b]) => End::Circle {
                v: vs[i],
                halves: [a, b],
            },
            None => End::Pole {
                v: vs[i],
                vert: ends[i].1[0],
                pos: ends[i].2[0],
            },
        };
        let (lo, hi) = (end(0), end(1));
        self.wall(surfaces, &lo, &hi, [&ms[0], &ms[1]], WALL)?;
        // The caps, by the height of their centres: bottom first.
        let mut order = [0, 1];
        if centres[0][2] > centres[1][2] {
            order.swap(0, 1);
        }
        for (part, i) in [BOTTOM, TOP].into_iter().zip(order) {
            if let (true, Some([a, b])) = (caps[i], &circles[i]) {
                let n = [0.0, 0.0, if part == BOTTOM { -1.0 } else { 1.0 }];
                self.cap(centres[i], e[2], n, [a, b], part)?;
            }
        }
        Ok(())
    }

    /// The cone or frustum from the circle about `bottom` (radius
    /// `radii[0]`) to the one about `top` (stored centres).
    fn cone(
        &mut self,
        bottom: [f64; 3],
        top: [f64; 3],
        radii: [f64; 2],
        least: &Rational,
    ) -> Result<(), KernelError> {
        let [r0, r1] = radii;
        let h = q(top[2]) - q(bottom[2]);
        above(&h, least)?;
        if !(r0 >= 0.0 && r1 >= 0.0) || r0 == r1 {
            return Err(dimension());
        }
        for r in radii {
            if r != 0.0 {
                above(&(q(2.0) * q(r)), least)?;
            }
        }
        // The apex on the axis where the radius reaches 0, rounded once;
        // the axis from it into the cone.
        let (near, far) = if r0 > r1 { (1, 0) } else { (0, 1) };
        let centres = [bottom, top];
        let (rn, rf) = (q(radii[near]), q(radii[far]));
        let zn = q(centres[near][2]);
        let zf = q(centres[far][2]);
        let apex_z = nearest(&(&zn - (&zf - &zn) * &rn / (&rf - &rn)));
        let apex = [bottom[0], bottom[1], apex_z];
        let down = if far == 0 { -1.0 } else { 1.0 };
        let axis = [0.0, 0.0, down];
        let along = (q(top[2]) - q(bottom[2])).abs();
        let across = (&rf - &rn).abs();
        let key = form(ConeKey::new(apex, axis, nearest(&along), nearest(&across)))?;
        let l = lattice(Lattice::new(key.into()))?;
        // A circle's slant row: its distance from the apex along the axis
        // over the key's cosine, in cells.
        let slant = |c: &[f64; 3]| {
            let t = (q(c[2]) - q(key.apex()[2])) * q(down);
            nearest(&(t / (q(key.cos()) * q(CELL))))
        };
        let vs = [
            if radii[near] == 0.0 {
                0.0
            } else {
                slant(&centres[near])
            },
            slant(&centres[far]),
        ];
        if vs[0] >= vs[1] {
            return Err(dimension());
        }
        // Below `2⁵³`: the rows are positions' distances over `CELL`.
        let (j0, j1) = (vs[0].floor() as i64, vs[1].floor() as i64 + 1);
        let surfaces = [0i64, 2].map(|first| {
            lattice(Surface::new(
                l.clone(),
                Rect {
                    level: Level::BASE,
                    u: first..first + 2,
                    v: j0..j1,
                },
            ))
        });
        let [s0, s2] = surfaces;
        let e = l.axes();
        let caps = [radii[near] != 0.0, true];
        self.revolved(
            [s0?, s2?],
            [centres[near], centres[far]],
            [radii[near], radii[far]],
            vs,
            e,
            caps,
        )
    }

    /// The sphere of radius `r` about `c`: two faces pole to pole.
    fn sphere(&mut self, c: [f64; 3], r: f64) -> Result<(), KernelError> {
        let key = form(SphereKey::new(c, r, [0.0, 0.0, 1.0]))?;
        let surfaces = [0, 2].map(|first| lattice(crate::geom::forms::sphere::sphere(key, first)));
        let [s0, s2] = surfaces;
        let surfaces = [s0?, s2?];
        self.poled(surfaces, 1, WHOLE)
    }

    /// The two faces of part `part` on `surfaces` (quarter turns 0 to 2
    /// and 2 to 4) from the south pole at row `−n` to the north pole at
    /// row `n`, along meridians of `2n` tile rows.
    fn poled(&mut self, surfaces: [Surface; 2], n: i64, part: u32) -> Result<(), KernelError> {
        let s = &surfaces[0];
        let pole = |v: i64, cell: i64| -> Result<[f64; 3], KernelError> {
            let row = s.row(Dir::U, v, cell).ok_or_else(defect)?;
            Ok([row[0][0], row[0][1], row[0][2]])
        };
        let (south, north) = (pole(-n, 0)?, pole(n, 0)?);
        let vs = self
            .b
            .add_pole(south, VertTag::Pole(PoleEnd::S))
            .map_err(built)?;
        let vn = self
            .b
            .add_pole(north, VertTag::Pole(PoleEnd::N))
            .map_err(built)?;
        let mut ms = Vec::new();
        for u in [0, 2] {
            let rows = (-n..n)
                .map(|cell| s.row(Dir::V, u, cell).ok_or_else(defect))
                .collect::<Result<Vec<_>, _>>()?;
            ms.push(self.edge(chain(rows), vs, vn, Sort::Seam)?);
        }
        let lo = End::Pole {
            v: -n as f64,
            vert: vs,
            pos: south,
        };
        let hi = End::Pole {
            v: n as f64,
            vert: vn,
            pos: north,
        };
        self.wall(surfaces, &lo, &hi, [&ms[0], &ms[1]], part)
    }

    /// The torus about `c` (module docs): a ring's four faces, a
    /// spindle's two.
    fn torus(
        &mut self,
        c: [f64; 3],
        major: f64,
        minor: f64,
        least: &Rational,
    ) -> Result<(), KernelError> {
        let (rr, r) = (q(major), q(minor));
        if !(major > 0.0 && minor > 0.0) || major == minor {
            return Err(dimension());
        }
        above(&(q(2.0) * &r), least)?;
        if major > minor {
            above(&(q(2.0) * (&rr - &r)), least)?;
        } else {
            // The apexes `2·√(r² − R²)` apart.
            let d2 = Rational::from(4u32) * (&r * &r - &rr * &rr);
            if d2 <= least * least {
                return Err(dimension());
            }
        }
        let key = form(TorusKey::new(c, [0.0, 0.0, 1.0], major, minor))?;
        let outer = [0, 2].map(|first| ring::torus(key, first));
        let tiles = |e: &LatticeError| matches!(e, LatticeError::Tile(_));
        if outer.iter().any(|s| s.as_ref().is_err_and(tiles)) {
            // A spindle too close to a horn torus for its tiles' weights.
            return Err(dimension());
        }
        let [o0, o2] = outer;
        let outer = [lattice(o0)?, lattice(o2)?];
        if major < minor {
            // The apple from apex to apex at tube level 1: rows −2 to 2.
            return self.poled(outer, 2, WHOLE);
        }
        let [i0, i2] = [0, 2].map(|first| lattice(ring::torus_inner(key, first)));
        let inner = [i0?, i2?];
        let s = &outer[0];
        let row = |along: Dir, at: i64, cell: i64| s.row(along, at, cell).ok_or_else(defect);
        let corner = |u: i64, v: i64| -> Result<[f64; 3], KernelError> {
            let r = row(Dir::V, u, v.min(0))?;
            let x = if v < 0 { r[0] } else { r[2] };
            Ok([x[0], x[1], x[2]])
        };
        // Vertices at quarter turns 0 and 2 on the lowest and highest
        // circles.
        let mut verts = [[VertId::NONE; 2]; 2];
        for (i, v) in [-1, 1].into_iter().enumerate() {
            for (k, u) in [0, 2].into_iter().enumerate() {
                verts[i][k] = self.vert(corner(u, v)?)?;
            }
        }
        // The circles' halves: tile rows of the outer surfaces.
        let mut circles = Vec::new();
        for (i, v) in [-1i64, 1].into_iter().enumerate() {
            let mut halves = Vec::new();
            for (k, surface) in outer.iter().enumerate() {
                let first = 2 * k as i64;
                let rows = (first..first + 2)
                    .map(|cell| surface.row(Dir::U, v, cell).ok_or_else(defect))
                    .collect::<Result<Vec<_>, _>>()?;
                let (from, to) = (verts[i][k], verts[i][(k + 1) % 2]);
                halves.push(self.edge(chain(rows), from, to, Sort::Seam)?);
            }
            circles.push(halves);
        }
        // The meridians: the outer halves from the lowest circle to the
        // highest, the inner from the highest to the lowest.
        let mut outer_m = Vec::new();
        let mut inner_m = Vec::new();
        for (k, u) in [0i64, 2].into_iter().enumerate() {
            let rows = vec![row(Dir::V, u, -1)?, row(Dir::V, u, 0)?];
            outer_m.push(self.edge(chain(rows), verts[0][k], verts[1][k], Sort::Seam)?);
            let s = &inner[0];
            let rows = (1..3)
                .map(|cell| s.row(Dir::V, u, cell).ok_or_else(defect))
                .collect::<Result<Vec<_>, _>>()?;
            inner_m.push(self.edge(chain(rows), verts[1][k], verts[0][k], Sort::Seam)?);
        }
        let circle = |i: usize, v: f64| End::Circle {
            v,
            halves: [&circles[i][0], &circles[i][1]],
        };
        let outer_ms = [&outer_m[0], &outer_m[1]];
        self.wall(outer, &circle(0, -1.0), &circle(1, 1.0), outer_ms, WHOLE)?;
        let inner_ms = [&inner_m[0], &inner_m[1]];
        self.wall(inner, &circle(1, 1.0), &circle(0, 3.0), inner_ms, WHOLE)
    }

    /// The faces' sides joined into the one lump's shells and regions;
    /// the edges above `ε_seam` listed as accuracy warnings, in
    /// edge order; the body finished.
    fn finish(mut self) -> Result<Brep, KernelError> {
        let side = |s| self.faces.iter().map(|&f| (f, s)).collect::<Vec<_>>();
        let (outs, ins) = (side(Side::Out), side(Side::In));
        let island = self.b.add_shell(ShellKind::Island, outs).map_err(built)?;
        let outer = self.b.add_shell(ShellKind::Outer, ins).map_err(built)?;
        self.b
            .add_region(
                RegionKind::Empty,
                ShellId::NONE,
                vec![island],
                RegionId::NONE,
            )
            .map_err(built)?;
        self.b
            .add_region(RegionKind::Material, outer, Vec::new(), RegionId::NONE)
            .map_err(built)?;
        let tols: Vec<f64> = (0..self.b.brep().edge_count())
            .map(|i| self.b.brep().edge_tol(EdgeId(i as u32)))
            .collect();
        for tol in tols {
            if tol > self.accuracy {
                let w = Warning::new(WarningKind::Accuracy, tol).ok_or_else(defect)?;
                self.b.status_mut().op_mut().warn(w);
            }
        }
        Ok(self.b.finish().map_err(built)?.0)
    }
}
