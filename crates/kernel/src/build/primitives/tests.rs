//! The primitives: their check, their volumes, areas and centroids
//! against the closed forms, their faces' keys and their edges' and
//! vertices' kinds and tags, and the input they refuse.

use std::f64::consts::PI;

use varde_decide::num::bounded::Bounded;
use varde_decide::num::trig;

use super::*;
use crate::geom::form::FormKey;
use crate::infra::ops::{FeatureId, InstanceId};
use crate::measure::mass::{MassProps, mass_props};
use crate::names::{edge_name, vert_names};
use crate::topo::brep::{CoedgeId, VertKind};
use crate::topo::check::{CheckError, Finding, Inv};

const FEATURE: FeatureId = FeatureId(7);

fn ctx() -> OpCtx {
    OpCtx {
        feature: FEATURE,
        instance: InstanceId(3),
    }
}

fn build_with(p: &Primitive, eps: Tolerance) -> Result<Brep, KernelError> {
    build(p, ctx(), eps, &mut Work::new(&Budget::DEFAULT))
}

fn built(p: &Primitive) -> Brep {
    build_with(p, Tolerance::DEFAULT).unwrap_or_else(|e| panic!("{p:?}: {e:?}"))
}

/// The check passes but for the parts this build doesn't certify yet:
/// curved trims (4) and faces apart (9). With 4 left out, so is every
/// invariant before 9; with 9 left out, every invariant but 4.
#[track_caller]
fn passes_check(b: &Brep) {
    let budget = Budget::DEFAULT;
    for r in [
        b.check(&budget),
        b.check_except(Inv::Trims, &budget),
        b.check_except(Inv::Embedding, &budget),
    ] {
        match r {
            Ok(()) => {}
            Err(CheckError::Failed(f))
                if f.finding == Finding::Unbuilt
                    && (f.inv == Inv::Trims || f.inv == Inv::Embedding) => {}
            Err(e) => panic!("check: {e:?}"),
        }
    }
}

/// The check passes every invariant before `inv`, but for curved trims
/// (4): with 4 left out, it fails, if at all, at `inv` or after.
#[track_caller]
fn passes_before(b: &Brep, inv: Inv) {
    let budget = Budget::DEFAULT;
    for r in [b.check(&budget), b.check_except(Inv::Trims, &budget)] {
        match r {
            Ok(()) => {}
            Err(CheckError::Failed(f)) if f.finding == Finding::Unbuilt && f.inv == Inv::Trims => {}
            Err(e) if e.inv().is_some_and(|at| at >= inv) => {}
            Err(e) => panic!("check: {e:?}"),
        }
    }
}

fn props(b: &Brep) -> MassProps {
    mass_props(b, &Budget::DEFAULT).unwrap()
}

/// `x` holds `exact` and is within `rel` of it relative to `scale`.
#[track_caller]
fn assert_holds(x: Bounded<f64>, exact: f64, scale: f64, rel: f64) {
    let d = (x.v - exact).abs();
    assert!(d <= x.err() + 1e-15 * scale, "{x:?} misses {exact}");
    assert!(
        x.err() <= rel * scale,
        "{x:?}: error above {rel} of {scale}"
    );
}

/// The volume, area and centroid hold the closed forms to 1e-12.
#[track_caller]
fn measures(b: &Brep, volume: f64, area: f64, centroid: [f64; 3], scale: f64) {
    let m = props(b);
    assert_holds(m.volume, volume, volume, 1e-12);
    assert_holds(m.area, area, area, 1e-12);
    let c = m.centroid.expect("a centroid");
    for k in 0..3 {
        assert_holds(c[k], centroid[k], scale, 1e-12);
    }
}

fn cuboid(min: [f64; 3], size: [f64; 3]) -> Primitive {
    Primitive::Cuboid { min, size }
}

fn cylinder(base: [f64; 3], radius: f64, height: f64) -> Primitive {
    Primitive::Cylinder {
        base,
        radius,
        height,
    }
}

fn cone(base: [f64; 3], radii: [f64; 2], height: f64) -> Primitive {
    Primitive::Cone {
        base,
        radii,
        height,
    }
}

fn sphere(centre: [f64; 3], radius: f64) -> Primitive {
    Primitive::Sphere { centre, radius }
}

fn torus(centre: [f64; 3], major: f64, minor: f64) -> Primitive {
    Primitive::Torus {
        centre,
        major,
        minor,
    }
}

/// One of each kind, with their face, edge and vertex counts.
fn all() -> Vec<(Primitive, [usize; 3])> {
    vec![
        (cuboid([1.0, -2.0, 0.5], [3.0, 5.0, 6.75]), [6, 12, 8]),
        (cylinder([1.5, -2.0, 3.0], 4.0, 9.5), [4, 6, 4]),
        (cone([1.0, 2.0, -3.0], [5.0, 0.0], 7.0), [3, 6, 3]),
        (cone([1.0, 2.0, -3.0], [0.0, 5.0], 7.0), [3, 6, 3]),
        (cone([-4.0, 0.25, 2.0], [3.0, 1.5], 2.5), [4, 6, 4]),
        (cone([-4.0, 0.25, 2.0], [1.5, 3.0], 2.5), [4, 6, 4]),
        (sphere([10.0, -20.0, 5.0], 7.0), [2, 6, 2]),
        (torus([3.0, 1.0, -2.0], 6.0, 2.5), [4, 8, 4]),
        (torus([3.0, 1.0, -2.0], 2.0, 3.0), [2, 6, 2]),
    ]
}

#[test]
fn every_primitive_passes_the_check_built() {
    for (p, [faces, edges, verts]) in all() {
        let b = built(&p);
        assert_eq!(
            [b.face_count(), b.edge_count(), b.vert_count()],
            [faces, edges, verts],
            "{p:?}"
        );
        passes_check(&b);
        assert!(!b.status().op().approximate(), "{p:?}");
    }
}

#[test]
fn the_entry_checks_what_it_builds() {
    for (p, _) in all() {
        let b = built(&p);
        let checked = b
            .check(&Budget::DEFAULT)
            .map_err(|e| match e.outcome(Role::Constructed) {
                Outcome::Error(e) => e,
                o => panic!("{o:?}"),
            });
        let entry = primitive(&p, ctx(), Tolerance::DEFAULT, &Budget::DEFAULT);
        match (entry, checked) {
            (Ok(_), Ok(())) => {}
            (Err(a), Err(b)) => assert_eq!(a, b, "{p:?}"),
            (a, b) => panic!("{p:?}: {a:?} against {b:?}"),
        }
    }
}

#[test]
fn a_box_has_its_volume_area_and_centroid() {
    let (lo, d) = ([1.0, -2.0, 0.5], [3.0, 5.0, 6.75]);
    let b = built(&cuboid(lo, d));
    let v = d[0] * d[1] * d[2];
    let a = 2.0 * (d[0] * d[1] + d[1] * d[2] + d[2] * d[0]);
    let c = [0, 1, 2].map(|k| lo[k] + d[k] * 0.5);
    measures(&b, v, a, c, 10.0);
}

#[test]
fn a_cylinder_has_its_volume_area_and_centroid() {
    let (base, r, h) = ([1.5, -2.0, 3.0], 4.0, 9.5);
    let b = built(&cylinder(base, r, h));
    let c = [base[0], base[1], base[2] + h * 0.5];
    measures(&b, PI * r * r * h, 2.0 * PI * r * (r + h), c, 10.0);
}

/// A frustum's volume, lateral area and centroid height over its base.
fn frustum(r0: f64, r1: f64, h: f64) -> (f64, f64, f64) {
    let v = PI * h * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0;
    let slant = (h * h + (r0 - r1) * (r0 - r1)).sqrt();
    let side = PI * (r0 + r1) * slant;
    let zc = h * (r0 * r0 + 2.0 * r0 * r1 + 3.0 * r1 * r1) / (4.0 * (r0 * r0 + r0 * r1 + r1 * r1));
    (v, side + PI * (r0 * r0 + r1 * r1), zc)
}

#[test]
fn cones_and_frustums_have_their_volume_area_and_centroid() {
    for (base, radii, h) in [
        ([1.0, 2.0, -3.0], [5.0, 0.0], 7.0),
        ([1.0, 2.0, -3.0], [0.0, 5.0], 7.0),
        ([-4.0, 0.25, 2.0], [3.0, 1.5], 2.5),
        ([-4.0, 0.25, 2.0], [1.5, 3.0], 2.5),
        ([0.0; 3], [0.75, 1.0], 100.0),
    ] {
        let b = built(&cone(base, radii, h));
        let (v, a, zc) = frustum(radii[0], radii[1], h);
        let c = [base[0], base[1], base[2] + zc];
        measures(&b, v, a, c, if h > 10.0 { h } else { 10.0 });
    }
}

#[test]
fn a_sphere_has_its_volume_area_and_centroid() {
    let (c, r) = ([10.0, -20.0, 5.0], 7.0);
    let b = built(&sphere(c, r));
    measures(&b, 4.0 / 3.0 * PI * r * r * r, 4.0 * PI * r * r, c, r);
}

#[test]
fn a_ring_torus_has_its_volume_area_and_centroid() {
    let (c, rr, r) = ([3.0, 1.0, -2.0], 6.0, 2.5);
    let b = built(&torus(c, rr, r));
    measures(
        &b,
        2.0 * PI * PI * rr * r * r,
        4.0 * PI * PI * rr * r,
        c,
        rr,
    );
}

/// A spindle torus's apple, by Pappus: the tube's disc on the axis's
/// near side, `ρ ≥ 0`, turned about the axis.
#[test]
fn a_spindle_torus_has_its_volume_area_and_centroid() {
    let (c, rr, r) = ([3.0, 1.0, -2.0], 2.0, 3.0);
    let b = built(&torus(c, rr, r));
    // The disc's first moment about the axis, less its part past it
    // (`x = ρ − R` from `−r` to `−R`): ∫ 2(R + x)·√(r² − x²) dx.
    let f = |x: f64| {
        let s = (r * r - x * x).sqrt();
        rr * (x * s + r * r * trig::asin(x / r)) - 2.0 / 3.0 * s * s * s
    };
    let moment = rr * PI * r * r - (f(-rr) - f(-r));
    let v = 2.0 * PI * moment;
    // The arc `ρ ≥ 0` (tube angles within φ*, cos φ* = −R/r) turned:
    // 2π·∫ (R + r·cos φ)·r dφ.
    let phi = trig::acos(-rr / r);
    let a = 2.0 * PI * r * (2.0 * rr * phi + 2.0 * r * trig::sin(phi));
    measures(&b, v, a, c, r);
}

#[test]
fn faces_are_keyed_by_their_part() {
    let keys = |p: &Primitive| {
        let b = built(p);
        let mut k: Vec<u32> = (0..b.face_count())
            .map(|f| match b.key(FaceId(f as u32)) {
                k if k.ctx() == ctx() => match k.part {
                    FacePart::Primitive(n) => n,
                    other => panic!("{other:?}"),
                },
                k => panic!("{k:?}"),
            })
            .collect();
        k.sort();
        k
    };
    assert_eq!(keys(&cuboid([0.0; 3], [1.0; 3])), [0, 1, 2, 3, 4, 5]);
    assert_eq!(keys(&cylinder([0.0; 3], 1.0, 1.0)), [0, 1, 2, 2]);
    assert_eq!(keys(&cone([0.0; 3], [1.0, 0.0], 1.0)), [0, 2, 2]);
    assert_eq!(keys(&cone([0.0; 3], [0.0, 1.0], 1.0)), [1, 2, 2]);
    assert_eq!(keys(&cone([0.0; 3], [1.0, 2.0], 1.0)), [0, 1, 2, 2]);
    assert_eq!(keys(&sphere([0.0; 3], 1.0)), [0, 0]);
    assert_eq!(keys(&torus([0.0; 3], 2.0, 1.0)), [0, 0, 0, 0]);
    // A box's sides by their outward normals.
    let b = built(&cuboid([0.0; 3], [1.0; 3]));
    let normals = [
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
    ];
    for f in 0..6 {
        let f = FaceId(f);
        let FacePart::Primitive(k) = b.key(f).part else {
            panic!()
        };
        let FormKey::Plane(plane) = b.face_surface(f).lattice().key() else {
            panic!()
        };
        let n = plane.normal();
        let s = if b.flipped(f) { -1.0 } else { 1.0 };
        assert_eq!(n.map(|x| x * s), normals[k as usize], "face {k}");
    }
}

/// The edges between faces of one part are tagged seams of one form;
/// the others creases between parts; poles and apexes tagged, each face
/// closing there at a singular edge of its own.
#[test]
fn edges_and_vertices_have_their_kinds_and_tags() {
    for (p, _) in all() {
        let b = built(&p);
        for e in (0..b.edge_count()).map(|e| EdgeId(e as u32)) {
            let c = b.edge_coedge(e);
            let pair = b.pair(c);
            match b.edge_kind(e) {
                EdgeKind::Singular => {
                    assert_eq!(pair, CoedgeId::NONE, "{p:?}");
                    assert_eq!(b.edge_tag(e), None);
                    let [v, w] = b.ends(e);
                    assert_eq!(v, w);
                    assert_eq!(b.vert_kind(v), VertKind::Pole);
                }
                kind => {
                    let keys = [b.key(b.face(c)), b.key(b.face(pair))];
                    let same = keys[0] == keys[1];
                    let name = edge_name(&b, e).expect("a name");
                    assert_eq!(name.between.is_one_key(), same);
                    if same {
                        assert_eq!(kind, EdgeKind::SameForm { touch: false }, "{p:?}");
                        assert_eq!(b.edge_tag(e), Some(EdgeTag::Seam));
                        assert_eq!(b.cont(e), Continuity::Curvature);
                    } else {
                        assert_eq!(kind, EdgeKind::Regular, "{p:?}");
                        assert_eq!(b.edge_tag(e), None);
                        assert_eq!(b.cont(e), Continuity::Crease(Convexity::Convex));
                    }
                }
            }
        }
        let names = vert_names(&b);
        for v in (0..b.vert_count()).map(|v| VertId(v as u32)) {
            let tag = b.vert_tag(v);
            assert_eq!(names[v.index()].tag, tag);
            match b.vert_kind(v) {
                VertKind::Pole => assert!(tag.is_some(), "{p:?}"),
                VertKind::Regular => assert_eq!(tag, None, "{p:?}"),
            }
        }
    }
}

#[test]
fn poles_are_named_by_the_axis() {
    let tags = |p: &Primitive| {
        let b = built(p);
        let mut t: Vec<(VertTag, f64)> = (0..b.vert_count())
            .filter_map(|v| {
                let v = VertId(v as u32);
                Some((b.vert_tag(v)?, b.pos(v)[2]))
            })
            .collect();
        t.sort_by(|a, b| a.1.total_cmp(&b.1));
        t.into_iter().map(|(t, _)| t).collect::<Vec<_>>()
    };
    let (n, s) = (VertTag::Pole(PoleEnd::N), VertTag::Pole(PoleEnd::S));
    assert_eq!(tags(&sphere([1.0, 2.0, 3.0], 4.0)), [s, n]);
    assert_eq!(tags(&torus([1.0, 2.0, 3.0], 1.0, 4.0)), [s, n]);
    assert_eq!(tags(&cone([0.0; 3], [2.0, 0.0], 3.0)), [VertTag::Apex]);
    // The apex is at the height the radii meet.
    let b = built(&cone([0.0, 0.0, 1.0], [0.0, 2.0], 3.0));
    let apex = (0..b.vert_count() as u32)
        .map(VertId)
        .find(|&v| b.vert_tag(v).is_some())
        .unwrap();
    assert_eq!(b.pos(apex), [0.0, 0.0, 1.0]);
}

/// A primitive is the same bits on every build, and its faces' keys
/// follow the caller's feature and copy, not the build.
#[test]
fn builds_are_deterministic() {
    for (p, _) in all() {
        let (a, b) = (built(&p), built(&p));
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "{p:?}");
    }
}

#[test]
fn dimensions_that_make_no_solid_are_refused() {
    let eps = Tolerance::DEFAULT;
    // `2·(1 + 2⁻⁸)·ε/8`, the least extent, is just above ε/4.
    let thin = eps.eps() / 4.0;
    let thick = eps.eps() / 4.0 * (1.0 + 1.0 / 128.0);
    let dim = Err(KernelError::Feature(FeatureKind::Dimension));
    for p in [
        cuboid([0.0; 3], [1.0, 0.0, 1.0]),
        cuboid([0.0; 3], [1.0, -1.0, 1.0]),
        cuboid([0.0; 3], [1.0, 1.0, thin]),
        cylinder([0.0; 3], 0.0, 1.0),
        cylinder([0.0; 3], -1.0, 1.0),
        cylinder([0.0; 3], 1.0, -1.0),
        cylinder([0.0; 3], thin / 2.0, 1.0),
        cylinder([0.0; 3], 1.0, thin),
        cone([0.0; 3], [1.0, 1.0], 1.0),
        cone([0.0; 3], [0.0, 0.0], 1.0),
        cone([0.0; 3], [1.0, -1.0], 1.0),
        cone([0.0; 3], [1.0, thin / 2.0], 1.0),
        cone([0.0; 3], [1.0, 0.0], 0.0),
        sphere([0.0; 3], 0.0),
        sphere([0.0; 3], thin / 2.0),
        torus([0.0; 3], 1.0, 1.0),
        torus([0.0; 3], 1.0, 0.0),
        torus([0.0; 3], 0.0, 1.0),
        torus([0.0; 3], 1.0 + thin / 2.0, 1.0),
        // A spindle whose apexes nearly meet, and one too close to a
        // horn torus for its tiles' weights.
        torus([0.0; 3], 1.0, 1.0 + thin * thin),
        torus([0.0; 3], 1.0, 1.0 + 1.0 / 4096.0),
    ] {
        assert_eq!(build_with(&p, eps).map(|_| ()), dim, "{p:?}");
    }
    for p in [
        cuboid([0.0; 3], [1.0, 1.0, thick]),
        cylinder([0.0; 3], thick / 2.0, 1.0),
        cylinder([0.0; 3], 1.0, thick),
        sphere([0.0; 3], thick / 2.0),
        torus([0.0; 3], 1.0 + thick / 2.0, 1.0),
    ] {
        let b = build_with(&p, eps).unwrap_or_else(|e| panic!("{p:?}: {e:?}"));
        // The embedding's certificates on a disc this thin may run out of
        // precision; every invariant before them holds.
        passes_before(&b, Inv::Embedding);
    }
}

#[test]
fn values_out_of_range_are_unrepresentable() {
    let out = Err(KernelError::Unrepresentable(Storage::OutOfRange));
    for p in [
        cuboid([f64::NAN, 0.0, 0.0], [1.0; 3]),
        cuboid([0.0; 3], [f64::INFINITY, 1.0, 1.0]),
        cuboid([999_999.5, 0.0, 0.0], [1.0; 3]),
        cylinder([0.0; 3], 1.0, f64::MAX),
        cylinder([0.0; 3], 2e6, 1.0),
        cone([0.0; 3], [f64::NAN, 0.0], 1.0),
        sphere([1e6, 0.0, 0.0], 1.0),
        torus([0.0; 3], 1e6, 1.0),
        torus([0.0; 3], 1.0, f64::INFINITY),
    ] {
        assert_eq!(build_with(&p, Tolerance::DEFAULT).map(|_| ()), out, "{p:?}");
    }
}

/// A slight taper: the apex far off, its rows' rounding at the apex's
/// distance; an edge's ribbon above `ε/4` is an accuracy warning, never
/// silent.
#[test]
fn a_cone_with_a_far_apex_warns_where_its_seams_pass_eps_seam() {
    let (r0, h) = (50.0, 100.0);
    for taper in [1e-3_f64, 1e-5, 1e-7] {
        let r1 = r0 - h * trig::tan(taper.to_radians());
        for eps in [Tolerance::DEFAULT, Tolerance::new(Tolerance::MIN).unwrap()] {
            let b = build_with(&cone([0.0; 3], [r0, r1], h), eps).unwrap();
            passes_check(&b);
            let most = (0..b.edge_count() as u32)
                .map(|e| b.edge_tol(EdgeId(e)))
                .fold(0.0, |a, t| if t > a { t } else { a });
            let status = b.status().op();
            assert_eq!(
                status.approximate(),
                most > eps.eps() / 4.0,
                "taper {taper}°"
            );
            if status.approximate() {
                assert_eq!(status.max_deviation(), most);
            }
            let (v, _, _) = frustum(r0, r1, h);
            let m = props(&b);
            assert!((m.volume.v - v).abs() <= m.volume.err() + 1e-12 * v);
        }
    }
}
