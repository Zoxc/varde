//! The label audit on the plane `z = 0` against boxes: regions clear of
//! the other operand, beside its faces and inside it, queried; a wrong
//! label as the ladder's signal; thin plates parallel to a face, with and
//! without transversal squares, and the tie bound; a query whose first
//! direction runs through a vertex; the wedges, fin and ledge slivers
//! within the depth the hysteresis needs; the cap where the hysteresis
//! can't settle; running out of budget; the charges of a square and of a
//! query; the same verdicts whatever the order of the triangles, loops
//! and regions; refused input; and regions read off a face arrangement.

use varde_decide::num::exact::rat;
use varde_decide::num::schedule::PredKind;

use super::*;
use crate::geom::form::{FormKey, PlaneKey};
use crate::geom::lattice::{CELL, Lattice};
use crate::infra::budget::{Budget, Limit};
use crate::infra::error::Cause;
use crate::topo::arrange2d::{Edge, arrange};

/// `ε_aud` for `ε ≈ 10⁻³`: `2⁻¹¹` mm.
const EPS_AUD: f64 = 1.0 / 2048.0;

/// `ρ_tie` for `ε ≈ 10⁻³`: `2⁻¹⁴` mm.
const RHO_TIE: f64 = 1.0 / 16384.0;

const BANDS: Bands = Bands {
    eps_aud: EPS_AUD,
    rho_tie: RHO_TIE,
};

/// The plane `z = 0`, its lattice and base tile.
struct Plane {
    lattice: Lattice,
    tile: Tile,
}

fn plane() -> Plane {
    let key = PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).expect("a plane");
    let lattice = Lattice::new(FormKey::Plane(key)).expect("its lattice");
    let tile = lattice.base_tile(0, 0).expect("its tile");
    Plane { lattice, tile }
}

impl Plane {
    /// The UV of the point `(x, y, 0)`, exact for the fixtures' dyadic
    /// coordinates.
    fn uv(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let (o, [u, v, _]) = (self.lattice.origin(), self.lattice.axes());
        let d = [x - o[0], y - o[1], -o[2]];
        let along = |a: [f64; 3]| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5;
        [along(u), along(v)]
    }

    /// The region bounded by the polygon `pts` (in `x, y`) with this
    /// winding number.
    fn region(&self, pts: &[[f64; 2]], winding: i64) -> AuditRegion {
        AuditRegion {
            loops: vec![pts.iter().map(|&p| self.uv(p)).collect()],
            winding,
        }
    }

    /// A leaf over the box of `pts` (in `x, y`).
    fn leaf(&self, pts: &[[f64; 2]], cover: Cover, partner: u32) -> Leaf {
        let uvs: Vec<[f64; 2]> = pts.iter().map(|&p| self.uv(p)).collect();
        let lo = |k: usize| uvs.iter().map(|p| p[k]).fold(f64::INFINITY, least);
        let hi = |k: usize| uvs.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, most);
        Leaf {
            uv: [[lo(0), lo(1)], [hi(0), hi(1)]],
            cover,
            partner,
        }
    }

    /// The 3D point of the UV `uv`.
    fn point(&self, uv: [f64; 2]) -> [f64; 3] {
        self.tile.eval(uv[0], uv[1]).map(|c| c.v)
    }
}

/// The box `[lo, hi]` as twelve triangles facing out.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Vec<Tri> {
    let c =
        |x: usize, y: usize, z: usize| [[lo[0], hi[0]][x], [lo[1], hi[1]][y], [lo[2], hi[2]][z]];
    let quads = [
        [c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0)],
        [c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)],
        [c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)],
        [c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)],
        [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)],
        [c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)],
    ];
    quads
        .iter()
        .flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]])
        .collect()
}

/// The square `[x, x + s] × [y, y + s]`, counter-clockwise.
fn square(x: f64, y: f64, s: f64) -> [[f64; 2]; 4] {
    [[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
}

/// The audit of `regions` with these leaves and triangles, ribbons none.
fn run_audit(
    p: &Plane,
    regions: &[AuditRegion],
    leaves: &[Leaf],
    solid: &[Tri],
    bands: Bands,
) -> Result<Vec<RegionAudit>, AuditError> {
    let input = AuditInput {
        tile: &p.tile,
        regions,
        leaves,
        ribbons: &[],
        solid,
        before: &[],
        bands,
    };
    audit(&input, &mut Work::new(&Budget::DEFAULT))
}

fn queried(a: &RegionAudit) -> bool {
    matches!(a.verdict, Verdict::Queried { .. })
}

/// The depth the hysteresis needs for a region (module docs): the
/// splits until its first squares' 3D side is below `(ε_aud − t)/2`,
/// `t` the greater of `ε_aud/2` and `ρ_tie`.
fn depth_bound(region: &AuditRegion, bands: Bands) -> u32 {
    let threshold = most(bands.eps_aud / 2.0, bands.rho_tie);
    let need = (bands.eps_aud - threshold) / 2.0;
    let mut s = side(Search::start(&region.loops).level) * CELL;
    let mut k = 0;
    while s >= need {
        s /= 2.0;
        k += 1;
    }
    k
}

#[test]
fn the_cuboid_winds_once_about_its_inside() {
    let solid = cuboid([-1.0, -2.0, -3.0], [4.0, 5.0, 6.0]);
    let mut work = Work::new(&Budget::DEFAULT);
    let w = |p: [f64; 3], work: &mut Work| {
        run(work, "test", &[()], |_, m| winding(&solid, p, T[0], m)).map(|mut v| v.remove(0))
    };
    assert_eq!(w([0.5, 0.5, 0.5], &mut work), Ok(Some(1)));
    assert_eq!(w([9.0, 0.5, 0.5], &mut work), Ok(Some(0)));
    assert_eq!(w([-9.0, -9.0, -9.0], &mut work), Ok(Some(0)));
}

#[test]
fn regions_clear_of_the_other_operand_are_queried() {
    let p = plane();
    let outside = p.region(&square(0.0, 0.0, 8.0), 0);
    let inside = p.region(&square(32.0, 0.0, 8.0), 1);
    let solid: Vec<Tri> = [
        cuboid([16.0, 0.0, -4.0], [24.0, 8.0, 4.0]),
        cuboid([28.0, -4.0, -4.0], [44.0, 12.0, 4.0]),
    ]
    .concat();
    let regions = [outside, inside];
    let audits = run_audit(&p, &regions, &[], &solid, BANDS).expect("audited");
    for (a, r) in audits.iter().zip(&regions) {
        assert!(queried(a), "{a:?}");
        assert!(a.depth <= depth_bound(r, BANDS), "{a:?}");
    }
}

#[test]
fn a_wrong_label_is_the_ladder_signal() {
    let p = plane();
    let solid = cuboid([16.0, 0.0, -4.0], [24.0, 8.0, 4.0]);
    let regions = [
        p.region(&square(0.0, 0.0, 8.0), 0),
        p.region(&square(0.0, 16.0, 8.0), 1),
    ];
    let e = run_audit(&p, &regions, &[], &solid, BANDS).expect_err("a mismatch");
    assert_eq!(
        e,
        AuditError::Mismatch {
            region: 1,
            label: 1,
            query: 0
        }
    );
    assert_eq!(
        e.kernel_error(),
        Some(KernelError::Internal(Internal::Inconsistent(
            Inconsistency::Proxy
        )))
    );
}

#[test]
fn regions_on_both_sides_of_a_transversal_face_are_queried() {
    // The box's face `x = 4` cuts the plane along a Simple arc between
    // the two regions.
    let p = plane();
    let solid = cuboid([4.0, -4.0, -4.0], [12.0, 12.0, 4.0]);
    let left = [[0.0, 0.0], [4.0, 0.0], [4.0, 8.0], [0.0, 8.0]];
    let right = [[4.0, 0.0], [8.0, 0.0], [8.0, 8.0], [4.0, 8.0]];
    let strip = [[3.5, 0.0], [4.5, 8.0]];
    let leaves = [p.leaf(&strip, Cover::Simple, 3)];
    let regions = [p.region(&left, 0), p.region(&right, 1)];
    let audits = run_audit(&p, &regions, &leaves, &solid, BANDS).expect("audited");
    for (a, r) in audits.iter().zip(&regions) {
        assert!(queried(a), "{a:?}");
        assert!(a.depth <= depth_bound(r, BANDS), "{a:?}");
    }
}

/// A plate region of side `2⁻⁸` mm with the box's bottom face `δ` above it.
fn plate(p: &Plane, delta: f64) -> (AuditRegion, Vec<Tri>) {
    let s = 1.0 / 256.0;
    let region = p.region(&square(0.0, 0.0, s), 0);
    (region, cuboid([-1.0, -1.0, delta], [2.0, 2.0, 4.0]))
}

#[test]
fn a_plate_under_a_near_face_is_thin() {
    let p = plane();
    let (region, solid) = plate(&p, EPS_AUD / 4.0);
    let s = 1.0 / 256.0;
    let leaves = [p.leaf(&square(0.0, 0.0, s), Cover::Near, 7)];
    let audits =
        run_audit(&p, std::slice::from_ref(&region), &leaves, &solid, BANDS).expect("audited");
    assert_eq!(
        audits[0].verdict,
        Verdict::Thin {
            partners: vec![Partner {
                face: 7,
                cover: Cover::Near
            }]
        }
    );
    assert!(audits[0].depth <= depth_bound(&region, BANDS));
}

#[test]
fn a_plate_partly_under_a_near_leaf_is_thin_by_it_alone() {
    // The squares of the right half are transversal, and their tie band
    // keeps them from being far: thin all the same, named by the left
    // half's leaf only.
    let p = plane();
    let (region, solid) = plate(&p, EPS_AUD / 4.0);
    let s = 1.0 / 256.0;
    let leaves = [
        p.leaf(&[[0.0, 0.0], [s / 4.0, s]], Cover::Contact, 9),
        p.leaf(&[[3.0 * s / 4.0, 0.0], [s, s]], Cover::Simple, 2),
    ];
    let bands = Bands {
        eps_aud: EPS_AUD,
        rho_tie: EPS_AUD / 2.0,
    };
    let audits = run_audit(&p, &[region], &leaves, &solid, bands).expect("audited");
    assert_eq!(
        audits[0].verdict,
        Verdict::Thin {
            partners: vec![Partner {
                face: 9,
                cover: Cover::Contact
            }]
        }
    );
}

#[test]
fn a_plate_within_the_tie_band_of_transversal_squares_stands_by_it() {
    let p = plane();
    let (region, solid) = plate(&p, EPS_AUD / 4.0);
    let audits = run_audit(&p, &[region], &[], &solid, BANDS).expect("audited");
    assert_eq!(audits[0].verdict, Verdict::TieBound);
}

#[test]
fn a_plate_beyond_half_the_band_is_queried() {
    let p = plane();
    let (region, solid) = plate(&p, 3.0 * EPS_AUD / 4.0);
    let s = 1.0 / 256.0;
    let leaves = [p.leaf(&square(0.0, 0.0, s), Cover::Near, 7)];
    let audits =
        run_audit(&p, std::slice::from_ref(&region), &leaves, &solid, BANDS).expect("audited");
    assert!(queried(&audits[0]), "{:?}", audits[0]);
    assert!(audits[0].depth <= depth_bound(&region, BANDS));
}

#[test]
fn a_ray_through_a_vertex_takes_the_next_direction() {
    let p = plane();
    let region = p.region(&square(0.0, 0.0, 8.0), 0);
    let mut solid = cuboid([16.0, 0.0, -4.0], [24.0, 8.0, 4.0]);
    let first = run_audit(&p, std::slice::from_ref(&region), &[], &solid, BANDS).expect("audited");
    let Verdict::Queried { uv, direction: 0 } = first[0].verdict else {
        panic!("{:?}", first[0]);
    };
    // A tetrahedron with a corner on the ray from that centre along the
    // first direction.
    let at = p.point(uv);
    let d = T[0] * 16.0;
    let v0 = [at[0] + d.x, at[1] + d.y, at[2] + d.z];
    for k in 0..3 {
        assert_eq!(rat(v0[k]) - rat(at[k]), rat(d[k]), "exact");
    }
    let off = |k: usize| {
        let mut v = v0;
        v[k] += 1.0;
        v
    };
    let (v1, v2, v3) = (off(0), off(1), off(2));
    solid.extend([[v0, v2, v1], [v0, v1, v3], [v0, v3, v2], [v1, v2, v3]]);
    let audits = run_audit(&p, &[region], &[], &solid, BANDS).expect("audited");
    assert_eq!(audits[0].verdict, Verdict::Queried { uv, direction: 1 });
}

/// The wedge with its apex at the origin, its side on `y = 0` and its
/// opening of slope `slope`, `len` mm long, against the box below `y = 0`.
fn wedge(p: &Plane, slope: f64, len: f64) -> (AuditRegion, Leaf) {
    let pts = [[0.0, 0.0], [len, 0.0], [len, len * slope]];
    (p.region(&pts, 0), p.leaf(&pts, Cover::Simple, 4))
}

#[test]
fn slivers_stay_within_the_depth_the_hysteresis_needs() {
    let p = plane();
    let below = cuboid([-8.0, -8.0, -8.0], [8.0, 0.0, 8.0]);
    let mut cases: Vec<(&str, AuditRegion, Vec<Leaf>, Vec<Tri>)> = Vec::new();
    // Wedges at about 1°, 0.1° and 0.01° against the face `y = 0`.
    for (name, slope) in [
        ("wedge 1°", 1.0 / 64.0),
        ("wedge 0.1°", 1.0 / 512.0),
        ("wedge 0.01°", 1.0 / 8192.0),
    ] {
        let (r, l) = wedge(&p, slope, 1.0);
        cases.push((name, r, vec![l], below.clone()));
    }
    // A fin `0.2·ε` wide between two boxes, a ledge `0.3·ε` wide beside one.
    let fin_w = 1.0 / 4096.0;
    let above = cuboid([-8.0, fin_w, -8.0], [8.0, 8.0, 8.0]);
    let fin = [[0.0, 0.0], [1.0, 0.0], [1.0, fin_w], [0.0, fin_w]];
    cases.push((
        "fin",
        p.region(&fin, 0),
        vec![p.leaf(&fin, Cover::Simple, 4)],
        [below.clone(), above].concat(),
    ));
    let ledge_w = 3.0 / 8192.0;
    let ledge = [[0.0, 0.0], [1.0, 0.0], [1.0, ledge_w], [0.0, ledge_w]];
    cases.push((
        "ledge",
        p.region(&ledge, 0),
        vec![p.leaf(&ledge, Cover::Simple, 4)],
        below.clone(),
    ));
    for (name, region, leaves, solid) in cases {
        let audits = run_audit(&p, std::slice::from_ref(&region), &leaves, &solid, BANDS)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let a = &audits[0];
        assert!(
            queried(a) || a.verdict == Verdict::TieBound,
            "{name}: {a:?}"
        );
        assert!(a.depth <= depth_bound(&region, BANDS), "{name}: {a:?}");
        assert!(a.depth <= D_AUDIT, "{name}: {a:?}");
    }
}

#[test]
fn the_slowest_plate_at_the_least_tolerance_stays_within_the_bound() {
    // `ε = 10⁻⁵`: a plate at `0.74·ε_aud` is far only past `R < 0.24·ε_aud`
    // and near only below `0.26·ε_aud`.
    let p = plane();
    let eps_aud = 1.0 / 262144.0;
    let bands = Bands {
        eps_aud,
        rho_tie: eps_aud / 8.0,
    };
    let s = 8.0 * eps_aud;
    let region = p.region(&square(0.0, 0.0, s), 0);
    let solid = cuboid([-1.0, -1.0, 0.74 * eps_aud], [2.0, 2.0, 4.0]);
    let leaves = [p.leaf(&square(0.0, 0.0, s), Cover::Near, 7)];
    let audits =
        run_audit(&p, std::slice::from_ref(&region), &leaves, &solid, bands).expect("audited");
    assert!(
        audits[0].depth <= depth_bound(&region, bands),
        "{:?}",
        audits[0]
    );
}

#[test]
fn the_cap_holds_the_depth_the_hysteresis_needs_at_the_extremes() {
    // The largest first square, the whole tile, at the least `ε_aud`
    // (`ε = 10⁻⁵`) with the widest `ρ_tie` (`2.5·ε_tie = 0.625·ε_aud`).
    let whole = AuditRegion {
        loops: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]],
        winding: 0,
    };
    assert_eq!(Search::start(&whole.loops).level, 0);
    let eps_aud = 5e-6;
    let widest = Bands {
        eps_aud,
        rho_tie: 0.625 * eps_aud,
    };
    assert_eq!(depth_bound(&whole, widest), D_AUDIT);
}

#[test]
fn a_region_the_hysteresis_cant_settle_reaches_the_cap() {
    // The tie band is `ε_aud` itself and the apex lies exactly `ε_aud`
    // from the face `x = 0`: the squares at the apex are never near nor
    // far.
    let p = plane();
    let a = EPS_AUD;
    let region = p.region(&[[0.0, -a], [a, 0.0], [0.0, a]], 0);
    let solid = cuboid([-8.0, -8.0, -8.0], [0.0, 8.0, 8.0]);
    let bands = Bands {
        eps_aud: EPS_AUD,
        rho_tie: EPS_AUD,
    };
    let e = run_audit(&p, &[region], &[], &solid, bands).expect_err("capped");
    let AuditError::OutOfBudget(KernelError::OutOfBudget { cause, profile }) = e else {
        panic!("{e:?}");
    };
    assert_eq!(cause, Cause::SizeLimit);
    assert_eq!(profile.limit, Limit::Cap(Cap::AuditDepth));
}

#[test]
fn running_out_of_budget_stops_the_audit() {
    let p = plane();
    let (region, solid) = plate(&p, EPS_AUD / 4.0);
    let input = AuditInput {
        tile: &p.tile,
        regions: &[region],
        leaves: &[],
        ribbons: &[],
        solid: &solid,
        before: &[],
        bands: BANDS,
    };
    let e = audit(&input, &mut Work::new(&Budget::new(40))).expect_err("out of budget");
    assert!(
        matches!(
            e,
            AuditError::OutOfBudget(KernelError::OutOfBudget {
                cause: Cause::Work,
                ..
            })
        ),
        "{e:?}"
    );
}

/// A square's near/far certificate is charged at the audit's row of the
/// unit table, per piece, its parts rounded up once: the square, its
/// eight loop tests (four apart from it, four of the inside test) and the
/// leaves', each triangle of both boundaries and each ribbon, and the
/// inside test's two predicates at the 2D orientation's filter price.
#[test]
fn a_square_is_charged_its_certificate_at_the_audit_s_row() {
    let p = plane();
    let region = p.region(&square(-64.0, -64.0, 128.0), 0);
    let cell = calibration::cell_at(p.uv([1.0, 1.0]), 22);
    let solid = cuboid([-1.0, -1.0, 2.0], [3.0, 3.0, 4.0]);
    let before = cuboid([8.0, 8.0, 1.0], [9.0, 9.0, 2.0]);
    let ribbons = [Ribbon {
        ends: [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]],
        beta: 1e-6,
    }; 3];
    let leaves = [p.leaf(&[[40.0, 40.0], [41.0, 41.0]], Cover::Near, 0); 5];
    let u = UNITS.audit;
    let filters = 2 * UNITS.record.of(PredKind::Orient2).filter;
    for (leaves, ribbons, solid, before) in [
        (&[][..], &[][..], &[][..], &[][..]),
        (&leaves[..], &[][..], &[][..], &[][..]),
        (&[][..], &ribbons[..], &solid[..], &[][..]),
        (&leaves[..], &ribbons[..], &solid[..], &before[..]),
    ] {
        let input = AuditInput {
            tile: &p.tile,
            regions: std::slice::from_ref(&region),
            leaves,
            ribbons,
            solid,
            before,
            bands: BANDS,
        };
        let parts = u.square
            + (8 + leaves.len() as u64) * u.test
            + (solid.len() + before.len()) as u64 * u.triangle
            + ribbons.len() as u64 * u.ribbon
            + filters;
        assert_eq!(
            calibration::squares(&input, 0, &[cell]),
            Some(parts.div_ceil(UNIT_PARTS)),
            "{} leaves, {} ribbons, {} triangles",
            leaves.len(),
            ribbons.len(),
            solid.len() + before.len()
        );
    }
}

/// A query at a far square's centre is charged its predicates alone: a
/// solid it misses costs no square's row, and twice the solid's
/// triangles cost twice the predicates, within the one unit rounded up.
#[test]
fn a_query_is_charged_its_predicates_alone() {
    let solid = cuboid([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]);
    assert_eq!(calibration::query(&[], [5.0, 5.0, 0.0], 0), (Some(0), 0));
    let (w, once) = calibration::query(&solid, [0.25, 0.125, 0.0], 0);
    assert_eq!(w, Some(1));
    let twice: Vec<Tri> = [solid.clone(), solid].concat();
    let (w, both) = calibration::query(&twice, [0.25, 0.125, 0.0], 0);
    assert_eq!(w, Some(2));
    assert!((2 * once - 1..=2 * once).contains(&both), "{once} {both}");
    // Each triangle's three edges at least, each at the 3D
    // determinant's filter price.
    let filter = UNITS.record.of(PredKind::Orient3).filter;
    assert!(once * UNIT_PARTS >= 12 * 3 * filter, "{once}");
}

#[test]
fn ribbons_widen_the_tie_band_of_the_squares_they_reach() {
    // A strip `0.75·ε_aud` under a transversal face is far past `ρ_tie`
    // before it is near; a ribbon along it whose seam bound reaches past
    // the face keeps every square from being far.
    let p = plane();
    let (s, w) = (1.0 / 256.0, EPS_AUD / 2.0);
    let region = p.region(&[[0.0, 0.0], [s, 0.0], [s, w], [0.0, w]], 0);
    let solid = cuboid([-1.0, -1.0, 3.0 * EPS_AUD / 4.0], [2.0, 2.0, 4.0]);
    let ribbons = [Ribbon {
        ends: [[0.0, w / 2.0, 0.0], [s, w / 2.0, 0.0]],
        beta: 7.0 * EPS_AUD / 8.0,
    }];
    let input = |ribbons| AuditInput {
        tile: &p.tile,
        regions: std::slice::from_ref(&region),
        leaves: &[],
        ribbons,
        solid: &solid,
        before: &[],
        bands: BANDS,
    };
    let bare = audit(&input(&[]), &mut Work::new(&Budget::DEFAULT)).expect("audited");
    assert!(queried(&bare[0]), "{:?}", bare[0]);
    let banded = audit(&input(&ribbons), &mut Work::new(&Budget::DEFAULT)).expect("audited");
    assert_eq!(banded[0].verdict, Verdict::TieBound);
}

#[test]
fn the_boundary_before_the_snap_counts_for_distances() {
    let p = plane();
    let region = p.region(&square(0.0, 0.0, 1.0 / 256.0), 0);
    let solid = cuboid([16.0, 0.0, -4.0], [24.0, 8.0, 4.0]);
    let before = cuboid([-1.0, -1.0, EPS_AUD / 4.0], [2.0, 2.0, 4.0]);
    let input = AuditInput {
        tile: &p.tile,
        regions: std::slice::from_ref(&region),
        leaves: &[],
        ribbons: &[],
        solid: &solid,
        before: &before,
        bands: BANDS,
    };
    let audits = audit(&input, &mut Work::new(&Budget::DEFAULT)).expect("audited");
    assert_eq!(audits[0].verdict, Verdict::TieBound);
}

#[test]
fn verdicts_dont_depend_on_the_order_of_the_input() {
    let p = plane();
    let solid = cuboid([4.0, -4.0, -4.0], [12.0, 12.0, 4.0]);
    let left = [[0.0, 0.0], [4.0, 0.0], [4.0, 8.0], [0.0, 8.0]];
    let right = [[4.0, 0.0], [8.0, 0.0], [8.0, 8.0], [4.0, 8.0]];
    // A second box far off, so the triangles' order has more to change.
    let far = cuboid([64.0, -1.0, -1.0], [66.0, 2.0, 4.0]);
    let all: Vec<Tri> = [solid, far].concat();
    let regions = vec![p.region(&left, 0), p.region(&right, 1)];
    let audits = run_audit(&p, &regions, &[], &all, BANDS).expect("audited");
    // Triangles reversed and each turned to start elsewhere; loops
    // rotated; regions swapped.
    let turned: Vec<Tri> = all.iter().rev().map(|&[a, b, c]| [b, c, a]).collect();
    let rotated: Vec<AuditRegion> = regions
        .iter()
        .rev()
        .map(|r| AuditRegion {
            loops: r
                .loops
                .iter()
                .map(|l| {
                    let mut l = l.clone();
                    l.rotate_left(1);
                    l
                })
                .collect(),
            winding: r.winding,
        })
        .collect();
    let again = run_audit(&p, &rotated, &[], &turned, BANDS).expect("audited");
    assert_eq!(audits[0], again[1]);
    assert_eq!(audits[1], again[0]);
}

#[test]
fn input_the_audit_doesnt_take_is_refused() {
    let p = plane();
    let solid = cuboid([16.0, 0.0, -4.0], [24.0, 8.0, 4.0]);
    let good = p.region(&square(0.0, 0.0, 8.0), 0);
    let one_point = AuditRegion {
        loops: vec![vec![p.uv([0.0, 0.0])]],
        winding: 0,
    };
    assert_eq!(
        run_audit(&p, &[good.clone(), one_point], &[], &solid, BANDS),
        Err(AuditError::Input(InputError::Region(1)))
    );
    let no_band = Bands {
        eps_aud: 0.0,
        rho_tie: 0.0,
    };
    assert_eq!(
        run_audit(&p, std::slice::from_ref(&good), &[], &solid, no_band),
        Err(AuditError::Input(InputError::Bands))
    );
    let mut bad = solid.clone();
    bad[3][1][2] = f64::NAN;
    assert_eq!(
        run_audit(&p, std::slice::from_ref(&good), &[], &bad, BANDS),
        Err(AuditError::Input(InputError::Triangle(3)))
    );
    let inverted = Leaf {
        uv: [[0.6, 0.5], [0.5, 0.6]],
        cover: Cover::Near,
        partner: 0,
    };
    assert_eq!(
        run_audit(&p, &[good], &[inverted], &solid, BANDS),
        Err(AuditError::Input(InputError::Leaf(0)))
    );
}

#[test]
fn regions_are_read_off_a_face_arrangement() {
    // A square with a square hole: the ring between them and the hole.
    let p = plane();
    let outer = square(0.0, 0.0, 8.0);
    let hole = square(2.0, 2.0, 4.0);
    let mut verts: Vec<[f64; 2]> = outer.iter().map(|&q| p.uv(q)).collect();
    verts.extend(hole.iter().map(|&q| p.uv(q)));
    // The plane's `u` runs along `−y`: the loops wind clockwise in UV.
    let ring = |first: u32| (0..4).map(move |k| [first + k, first + (k + 1) % 4]);
    let edges: Vec<Edge> = ring(0)
        .chain(ring(4))
        .map(|ends| Edge { ends, wind: 1 })
        .collect();
    let arr = arrange(&verts, &edges, &mut Work::new(&Budget::DEFAULT)).expect("arranged");
    assert_eq!(AuditRegion::of(&arr, &verts, 0), None);
    let regions: Vec<AuditRegion> = (1..arr.regions().len() as u32)
        .map(|r| AuditRegion::of(&arr, &verts, r).expect("bounded"))
        .collect();
    let ring_region = regions
        .iter()
        .find(|r| r.loops.len() == 2)
        .expect("the ring");
    let hole_region = regions
        .iter()
        .find(|r| r.loops.len() == 1)
        .expect("the hole");
    assert_eq!(ring_region.loops[0].len(), 4);
    assert_eq!(ring_region.loops[1].len(), 4);
    // Labelled by the arrangement's windings, both audit: the ring and
    // the hole lie clear of a box beside them.
    let solid = cuboid([16.0, 0.0, -4.0], [24.0, 8.0, 4.0]);
    let labelled: Vec<AuditRegion> = [ring_region, hole_region]
        .into_iter()
        .map(|r| AuditRegion {
            loops: r.loops.clone(),
            winding: 0,
        })
        .collect();
    let audits = run_audit(&p, &labelled, &[], &solid, BANDS).expect("audited");
    assert!(audits.iter().all(queried), "{audits:?}");
    let Verdict::Queried { uv, .. } = audits[1].verdict else {
        unreachable!()
    };
    // The hole's query lies in the hole.
    let q = p.point(uv);
    assert!(
        (2.0..=6.0).contains(&q[0]) && (2.0..=6.0).contains(&q[1]),
        "{q:?}"
    );
}

/// Audits of several regions on budgets running out anywhere along them,
/// and the whole: the same verdicts, or the same error, cause and profile,
/// with the items after the deciding one stopped early or run on.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn audits_running_out_are_the_same_without_early_stop() {
    use crate::infra::budget::assert_early_stop_changes_nothing;
    let p = plane();
    let regions = [
        p.region(&square(0.0, 0.0, 8.0), 0),
        p.region(&square(32.0, 0.0, 8.0), 1),
        p.region(&square(0.0, 16.0, 8.0), 0),
    ];
    let solid: Vec<Tri> = [
        cuboid([16.0, 0.0, -4.0], [24.0, 8.0, 4.0]),
        cuboid([28.0, -4.0, -4.0], [44.0, 12.0, 4.0]),
    ]
    .concat();
    let input = AuditInput {
        tile: &p.tile,
        regions: &regions,
        leaves: &[],
        ribbons: &[],
        solid: &solid,
        before: &[],
        bands: BANDS,
    };
    let run = |budget| {
        let mut work = Work::new(&Budget::new(budget));
        let out = audit(&input, &mut work);
        (out, work.used())
    };
    let (out, used) = run(crate::MAX_WORK);
    assert!(out.is_ok(), "{out:?}");
    for k in 0..=12 {
        let (out, _) = assert_early_stop_changes_nothing(|| run(used * k / 12));
        assert_eq!(out.is_ok(), k == 12, "{k}");
    }
}
