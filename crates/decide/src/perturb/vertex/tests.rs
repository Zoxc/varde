//! The vertex rules on exact fixtures: groups against the components of
//! the short edges, worked out apart; flush-patch directions against the
//! patches' normals (agreement, the max–min optimum, the cube's
//! symmetries, the opposed and saddle cases), curved at every level; the
//! saddle partner's certificate against a brute force on cones of `A`
//! made of closed octants (so membership is exact) under integer maps,
//! with the partner's cone inside an empty octant, crossing a face or
//! swallowed by `A`; and the answers of records reading these directions
//! against `A` moved by `δ·(σ_A·d + η·T₂ + η²·T₃ + η³·T₄)`.

use malachite::Rational as Q;

use super::*;
use crate::bezier::Scalar;
use crate::ids::VertId;
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;
use crate::num::decide::{Row, Sign, TieKey};
use crate::num::exact::ORDERS;
use crate::perturb::dir::{agrees, max_min};
use crate::perturb::{Moving, PlaneHeight, Rate, T, UP, decide_flat, rates_a};
use crate::records::keys::{Query, RecordKey, VvKey};

struct Rng(varde_testkit::rng::Rng);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    /// A small integer in `-n..=n`.
    fn int(&mut self, n: u64) -> f64 {
        self.below(2 * n + 1) as f64 - n as f64
    }

    fn ints(&mut self, n: u64) -> DVec3 {
        DVec3::new(self.int(n), self.int(n), self.int(n))
    }

    fn nonzero(&mut self, n: u64) -> DVec3 {
        loop {
            let v = self.ints(n);
            if v != DVec3::ZERO {
                return v;
            }
        }
    }
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn qv(v: DVec3) -> Q3 {
    [q(v.x), q(v.y), q(v.z)]
}

/// The groups by a search over the short edges, each vertex's group's
/// least member.
fn components(verts: &[Blended], edges: &[[usize; 2]]) -> Vec<usize> {
    let short = |&[a, b]: &[usize; 2]| {
        let v = qv(verts[b].at - verts[a].at);
        let reach = Q::from(2u32) * (q(verts[a].r) + q(verts[b].r));
        verts[a].r > 0.0 && verts[b].r > 0.0 && dot(&v, &v) < &reach * &reach
    };
    let n = verts.len();
    let mut least = vec![usize::MAX; n];
    for start in 0..n {
        if least[start] != usize::MAX {
            continue;
        }
        let mut stack = vec![start];
        least[start] = start;
        while let Some(i) = stack.pop() {
            for e in edges.iter().filter(|e| short(e)) {
                for (from, to) in [(e[0], e[1]), (e[1], e[0])] {
                    if from == i && least[to] == usize::MAX {
                        least[to] = start;
                        stack.push(to);
                    }
                }
            }
        }
    }
    least
}

/// Groups on random small configurations: the components of the edges
/// shorter than `2(r_a + r_b)`, exactly (an edge of exactly that length
/// joins nothing), each named by its least member, whatever the edges'
/// order; each edge's test counted once.
#[test]
fn vertex_groups_are_the_components_of_short_edges() {
    let mut r = Rng::new(41);
    let mut joined = 0;
    for _ in 0..300 {
        let n = 2 + r.below(8) as usize;
        let verts: Vec<Blended> = (0..n)
            .map(|_| Blended {
                at: r.ints(4),
                r: r.below(5) as f64 / 4.0,
            })
            .collect();
        let mut edges: Vec<[usize; 2]> = (0..r.below(12))
            .map(|_| [r.below(n as u64) as usize, r.below(n as u64) as usize])
            .collect();
        let mut work = DirWork::default();
        let got = vertex_groups(&verts, &edges, &mut work);
        assert_eq!(got, components(&verts, &edges), "{verts:?} {edges:?}");
        assert_eq!(work.checks as usize, edges.len());
        joined += got.iter().enumerate().filter(|(i, g)| *i != **g).count();
        edges.reverse();
        assert_eq!(got, vertex_groups(&verts, &edges, &mut work));
    }
    assert!(joined > 30, "{joined} vertices joined");
    // Exactly `2(r_a + r_b)` apart: not joined; a hair closer: joined.
    let at = |x: f64| Blended {
        at: DVec3::new(x, 0.0, 0.0),
        r: 0.25,
    };
    let mut work = DirWork::default();
    assert_eq!(
        vertex_groups(&[at(0.0), at(1.0)], &[[0, 1]], &mut work),
        [0, 1]
    );
    let near = 1.0 - f64::EPSILON / 2.0;
    assert_eq!(
        vertex_groups(&[at(0.0), at(near)], &[[1, 0]], &mut work),
        [0, 0]
    );
    // A radius that isn't a positive finite number joins nothing.
    let bad = Blended {
        at: DVec3::ZERO,
        r: f64::NAN,
    };
    assert_eq!(vertex_groups(&[bad, at(0.0)], &[[0, 1]], &mut work), [0, 1]);
}

/// A group's blend weight is its nearest member's: zero within `r_G` of
/// any member, one from `2r_G` of all, and a group of one weighs as its
/// vertex does.
#[test]
fn group_weights_are_the_nearest_members() {
    let mut r = Rng::new(42);
    for _ in 0..200 {
        let members: Vec<DVec3> = (0..1 + r.below(4)).map(|_| r.ints(4)).collect();
        let x = r.ints(6) / 2.0;
        let r_g = 0.5 + r.below(4) as f64 / 2.0;
        let qm: Vec<Q3> = members.iter().map(|&m| qv(m)).collect();
        let l = group_weight_flat(&qm, &qv(x), r_g);
        let own: Vec<Q> = qm.iter().map(|m| weight_flat(m, &qv(x), r_g)).collect();
        assert_eq!(Some(&l), own.iter().min());
        let lc = group_weight(&members, x, r_g);
        let own: Vec<f64> = members.iter().map(|&m| blend_weight(m, x, r_g)).collect();
        assert!(own.iter().all(|&o| lc <= o) && own.contains(&lc));
        if members.len() == 1 {
            assert_eq!(l, weight_flat(&qm[0], &qv(x), r_g));
        }
        let near = members.iter().any(|&m| {
            let v = (x - m).abs();
            v.x <= r_g && v.y <= r_g && v.z <= r_g
        });
        assert_eq!(l == 0u32, near, "{members:?} {x}");
    }
    assert_eq!(group_weight_flat(&[], &qv(DVec3::ZERO), 1.0), 1u32);
}

fn co(normal: DVec3) -> Patch {
    Patch {
        normal,
        side: PatchSide::CoOriented,
    }
}

fn opp(normal: DVec3) -> Patch {
    Patch {
        normal,
        side: PatchSide::Opposed,
    }
}

/// `n/‖n‖∞`.
fn scaled(n: DVec3) -> Q3 {
    scaled_normal(n).expect("non-zero")
}

/// The 48 signed permutations of the axes, as `(π, s)` with `(P·v)ᵢ =
/// sᵢ·v_{πᵢ}`.
fn signed_perms() -> Vec<([usize; 3], [f64; 3])> {
    let perms = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut out = Vec::new();
    for p in perms {
        for bits in 0..8 {
            let s = [0, 1, 2].map(|i| if bits >> i & 1 == 1 { -1.0 } else { 1.0 });
            out.push((p, s));
        }
    }
    out
}

fn perm(v: DVec3, (p, s): &([usize; 3], [f64; 3])) -> DVec3 {
    DVec3::new(s[0] * v[p[0]], s[1] * v[p[1]], s[2] * v[p[2]])
}

fn perm_q(v: &Q3, (p, s): &([usize; 3], [f64; 3])) -> Q3 {
    [0, 1, 2].map(|i| q(s[i]) * &v[p[i]])
}

/// One to four patches of small integer normals, of either kind; a third
/// of the sets hold a normal and its opposite (no direction agrees).
fn random_patches(r: &mut Rng) -> Vec<Patch> {
    let mut ns: Vec<DVec3> = (0..1 + r.below(4)).map(|_| r.nonzero(2)).collect();
    if r.below(3) == 0 {
        ns.push(-ns[0]);
    }
    ns.into_iter()
        .map(|n| if r.below(2) == 0 { co(n) } else { opp(n) })
        .collect()
}

/// The flush-patch direction on flat faces: the max–min direction over
/// the patched faces alone, agreeing with each, its minimum the
/// program's, moved with the patches by the cube's symmetries; one patch
/// along an axis gives its normal, a filled reflex notch the bisector cap
/// `(1, 1, 0)`; a slit's two sides (opposite normals) are the opposed
/// rule when one patch of each kind, else the saddle rule, as is no
/// patch; the program counted.
#[test]
fn flush_patch_directions_on_flat_faces() {
    let mut work = DirWork::default();
    let dir = |p: &[Patch], work: &mut DirWork| flush_flat(p, work).expect("finite normals");
    let d_of = |f: Flush<FlatDir>| match f {
        Flush::Dir(d) => d.d(),
        other => panic!("{other:?}"),
    };
    let one = |x: i32| Q::from(x);
    assert_eq!(
        d_of(dir(&[co(DVec3::new(0.0, 0.0, 2.0))], &mut work)),
        [one(0), one(0), one(1)]
    );
    assert!(work.pivots > 0 && work.solves > 0, "{work:?}");
    assert_eq!(
        d_of(dir(&[co(DVec3::X), opp(DVec3::Y)], &mut work)),
        [one(1), one(1), one(0)]
    );
    let slit = [co(DVec3::Z), opp(-DVec3::Z)];
    assert_eq!(dir(&slit, &mut work), Flush::Opposed);
    assert_eq!(
        dir(&[co(DVec3::Z), co(-DVec3::Z)], &mut work),
        Flush::Saddle
    );
    assert_eq!(
        dir(&[opp(DVec3::Z), opp(-DVec3::Z)], &mut work),
        Flush::Saddle
    );
    assert_eq!(dir(&[], &mut work), Flush::Saddle);
    assert_eq!(flush_flat(&[co(DVec3::ZERO)], &mut work), None);
    let mut r = Rng::new(43);
    let (mut dirs, mut opposed, mut saddles) = (0, 0, 0);
    for _ in 0..300 {
        let patches = random_patches(&mut r);
        let ns: Vec<DVec3> = patches.iter().map(|p| p.normal).collect();
        let mut sq: Vec<Q3> = ns.iter().map(|&n| scaled(n)).collect();
        sq.sort();
        sq.dedup();
        let (lp_d, lp_min) = max_min(&sq);
        match dir(&patches, &mut work) {
            Flush::Dir(FlatDir::MaxMin { d, min }) => {
                dirs += 1;
                assert!(agrees(&ns, &d), "{patches:?}");
                assert_eq!((&d, &min), (&lp_d, &lp_min));
                for g in signed_perms() {
                    let moved: Vec<Patch> = patches
                        .iter()
                        .map(|p| Patch {
                            normal: perm(p.normal, &g),
                            ..*p
                        })
                        .collect();
                    assert_eq!(
                        d_of(dir(&moved, &mut work)),
                        perm_q(&d, &g),
                        "{patches:?} {g:?}"
                    );
                }
            }
            Flush::Opposed => {
                opposed += 1;
                assert!(lp_min <= 0u32);
                let sides: Vec<PatchSide> = patches.iter().map(|p| p.side).collect();
                assert!(sides.contains(&PatchSide::CoOriented));
                assert!(sides.contains(&PatchSide::Opposed));
            }
            Flush::Saddle => {
                saddles += 1;
                assert!(lp_min <= 0u32);
                assert!(patches.iter().all(|p| p.side == patches[0].side));
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(
        dirs > 50 && opposed > 5 && saddles > 5,
        "{dirs} {opposed} {saddles}"
    );
}

/// A level's value and enclosure as exact rationals, for checks.
trait Exactly: Curved {
    fn lo_hi(&self) -> (Q, Q);
}

impl Exactly for Bounded<f64> {
    fn lo_hi(&self) -> (Q, Q) {
        let [lo, hi] = self.enclose();
        (q(lo), q(hi))
    }
}

impl Exactly for Bounded<Dd> {
    fn lo_hi(&self) -> (Q, Q) {
        let [lo, hi] = self.enclose();
        (q(lo), q(hi))
    }
}

impl Exactly for Bounded<Big> {
    fn lo_hi(&self) -> (Q, Q) {
        let [lo, hi] = self.enclose();
        (q(lo), q(hi))
    }
}

/// Whether `n·d > 0` over all of `d`'s enclosure, `n` exact.
fn agrees_over<S: Exactly>(n: DVec3, d: &[S; 3]) -> bool {
    let n = qv(n);
    let mut lo = Q::from(0u32);
    for k in 0..3 {
        let (a, b) = d[k].lo_hi();
        lo += if n[k] >= 0u32 { &n[k] * a } else { &n[k] * b };
    }
    lo > 0u32
}

fn curved_flush<S: Exactly>(like: &S) {
    let mut r = Rng::new(44);
    let mut work = DirWork::default();
    let (mut dirs, mut none) = (0, 0);
    for _ in 0..200 {
        let patches = random_patches(&mut r);
        let curved: Vec<CurvedPatch<S>> = patches
            .iter()
            .map(|p| CurvedPatch {
                normal: lit3(p.normal, like),
                approx: p.normal,
                side: p.side,
            })
            .collect();
        let flat = flush_flat(&patches, &mut work).expect("finite");
        match flush_at(&curved, &mut work) {
            Flush::Dir(d) => {
                dirs += 1;
                assert!(matches!(flat, Flush::Dir(_)), "{patches:?}");
                assert!(patches.iter().all(|p| agrees_over(p.normal, &d)));
            }
            Flush::Escalate => {}
            f => {
                none += 1;
                assert_eq!(
                    std::mem::discriminant(&f),
                    std::mem::discriminant(&match flat {
                        Flush::Opposed => Flush::Opposed,
                        Flush::Saddle => Flush::Saddle,
                        other => panic!("{patches:?}: {other:?}"),
                    })
                );
            }
        }
    }
    assert!(dirs > 50 && none > 10, "{dirs} {none}");
}

/// Curved flush patches at every level, on exact normals: a direction
/// only where one agrees, certified over its whole enclosure; the opposed
/// and saddle rules only where none does, with the flat rule's kind.
#[test]
fn curved_flush_patches_certify_at_every_level() {
    curved_flush(&Bounded::exact(0.0));
    curved_flush(&Bounded::exact(Dd::from(0.0)));
    curved_flush(&Bounded::exact(Big::from_f64(0.0, Width::ALL[0])));
}

/// A cone of `A` made of closed octants (`inside[o]` for the octant of
/// signs `o`, bit `i` set for `xᵢ < 0`), mapped by an integer matrix `m`
/// of positive determinant: its faces' corners as [`Sector`]s, coplanar
/// quarters of one face merged (straight and reflex corners), and exact
/// membership.
#[derive(Clone, Debug)]
struct OctantCone {
    inside: [bool; 8],
    m: [[i64; 3]; 3],
}

impl OctantCone {
    fn det(&self) -> i64 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    fn map(&self, v: [i64; 3]) -> DVec3 {
        let r = |i: usize| (0..3).map(|k| self.m[i][k] * v[k]).sum::<i64>() as f64;
        DVec3::new(r(0), r(1), r(2))
    }

    /// The cofactor matrix: normals map by it (a positive multiple of
    /// `m^{−T}`).
    fn cof(&self, i: usize, k: usize) -> i64 {
        let m = &self.m;
        let (i1, i2) = ((i + 1) % 3, (i + 2) % 3);
        let (k1, k2) = ((k + 1) % 3, (k + 2) % 3);
        m[i1][k1] * m[i2][k2] - m[i1][k2] * m[i2][k1]
    }

    fn map_normal(&self, n: [i64; 3]) -> DVec3 {
        let r = |i: usize| (0..3).map(|k| self.cof(i, k) * n[k]).sum::<i64>() as f64;
        DVec3::new(r(0), r(1), r(2))
    }

    /// `adj(m)·x`: `det·m^{−1}·x`, in the octants' frame.
    fn unmap(&self, x: &Q3) -> Q3 {
        [0, 1, 2].map(|i| (0..3).map(|k| Q::from(self.cof(k, i)) * &x[k]).sum())
    }

    fn sectors(&self) -> Vec<Sector> {
        let mut out = Vec::new();
        let unit = |i: usize, s: i64| {
            let mut v = [0i64; 3];
            v[i] = s;
            v
        };
        for i in 0..3 {
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            // The quarter rays about `+eᵢ`, counter-clockwise.
            let rays = [unit(j, 1), unit(k, 1), unit(j, -1), unit(k, -1)];
            // Quadrant `q`'s signs of `x_j`, `x_k`.
            let quad = |q: usize| [[1, 1], [-1, 1], [-1, -1], [1, -1]][q];
            for sigma in [1i64, -1] {
                // A face in quadrant `q` with outward normal `σ·eᵢ`: `A`
                // on the `−σ` side, nothing on the `σ` side.
                let face = |q: usize| {
                    let [sj, sk] = quad(q);
                    let oct = |si: i64| {
                        let mut o = 0usize;
                        for (axis, s) in [(i, si), (j, sj), (k, sk)] {
                            if s < 0 {
                                o |= 1 << axis;
                            }
                        }
                        o
                    };
                    self.inside[oct(-sigma)] && !self.inside[oct(sigma)]
                };
                let faces: Vec<bool> = (0..4).map(face).collect();
                let n = self.map_normal(unit(i, sigma));
                if faces.iter().all(|&f| f) {
                    // A whole plane: two straight corners.
                    for (a, b) in [(0, 2), (2, 0)] {
                        let (from, to) = if sigma > 0 { (a, b) } else { (b, a) };
                        out.push(self.sector(rays[from], rays[to], n));
                    }
                    continue;
                }
                // Maximal runs of faces, cyclically, from after a gap.
                let Some(gap) = (0..4).find(|&q| !faces[q]) else {
                    continue;
                };
                let mut i = 1;
                while i < 4 {
                    let start = (gap + i) % 4;
                    let mut len = 0;
                    while i < 4 && faces[(gap + i) % 4] {
                        len += 1;
                        i += 1;
                    }
                    if len == 0 {
                        i += 1;
                        continue;
                    }
                    let (a, b) = (start, (start + len) % 4);
                    let (from, to) = if sigma > 0 { (a, b) } else { (b, a) };
                    out.push(self.sector(rays[from], rays[to], n));
                }
            }
        }
        out
    }

    fn sector(&self, from: [i64; 3], to: [i64; 3], n: DVec3) -> Sector {
        Sector::of(DVec3::ZERO, self.map(from), self.map(to), n).expect("finite")
    }

    /// Whether `x` (world frame) lies in the closed cone.
    fn contains(&self, x: &Q3) -> bool {
        let y = self.unmap(x);
        (0..8).any(|o| {
            self.inside[o]
                && (0..3).all(|i| {
                    if o >> i & 1 == 1 {
                        y[i] <= 0u32
                    } else {
                        y[i] >= 0u32
                    }
                })
        })
    }
}

/// Whether some `μ` in the plane has every `a·μ + b ≥ 0`, for a set whose
/// feasible region has a vertex when it isn't empty: some point where two
/// of the lines meet satisfies all.
fn feasible2(rows: &[([Q; 2], Q)]) -> bool {
    for (i, (a, b)) in rows.iter().enumerate() {
        for (c, e) in &rows[i + 1..] {
            let det = &a[0] * &c[1] - &a[1] * &c[0];
            if det == 0u32 {
                continue;
            }
            // `a·μ = −b`, `c·μ = −e`.
            let mu = [
                (-b * &c[1] + e * &a[1]) / &det,
                (-&a[0] * e + &c[0] * b) / &det,
            ];
            if rows
                .iter()
                .all(|(x, y)| &x[0] * &mu[0] + &x[1] * &mu[1] + y >= 0u32)
            {
                return true;
            }
        }
    }
    false
}

/// A convex partner cone of `B`, by its generators and its outward
/// normals.
#[derive(Clone, Debug)]
enum Partner {
    /// `cone(g₁, g₂, g₃)`, independent.
    Corner([DVec3; 3]),
    /// `{n₁·x ≤ 0, n₂·x ≤ 0}`, independent.
    Wedge([DVec3; 2]),
}

impl Partner {
    fn normals(&self) -> Vec<DVec3> {
        match self {
            Partner::Corner(g) => (0..3)
                .map(|i| {
                    let n = g[(i + 1) % 3].cross(g[(i + 2) % 3]);
                    if n.dot(g[i]) < 0.0 { n } else { -n }
                })
                .collect(),
            Partner::Wedge(n) => n.to_vec(),
        }
    }

    /// Whether the cone meets the closed octant cone `a` away from the
    /// apex, by brute force in the octants' frame.
    fn meets(&self, a: &OctantCone) -> bool {
        let octant = |o: usize, x: &[[Q; 3]; 3]| -> Vec<([Q; 2], Q)> {
            // `x = x₀ + μ₀·x₁ + μ₁·x₂`, each coordinate signed.
            (0..3)
                .map(|i| {
                    let s = if o >> i & 1 == 1 {
                        Q::from(-1)
                    } else {
                        Q::from(1)
                    };
                    ([&s * &x[1][i], &s * &x[2][i]], &s * &x[0][i])
                })
                .collect()
        };
        let un = |v: DVec3| a.unmap(&qv(v));
        let sub = |p: &Q3, r: &Q3| -> Q3 { [0, 1, 2].map(|i| &p[i] - &r[i]) };
        let zero = || Q::from(0u32);
        (0..8).filter(|&o| a.inside[o]).any(|o| match self {
            Partner::Corner(g) => {
                let g = g.map(un);
                // `g₃ + μ₀·(g₁ − g₃) + μ₁·(g₂ − g₃)` on the simplex.
                let x = [g[2].clone(), sub(&g[0], &g[2]), sub(&g[1], &g[2])];
                let mut rows = octant(o, &x);
                rows.push(([Q::from(1), zero()], zero()));
                rows.push(([zero(), Q::from(1)], zero()));
                rows.push(([Q::from(-1), Q::from(-1)], Q::from(1)));
                feasible2(&rows)
            }
            Partner::Wedge(n) => {
                let e = n[0].cross(n[1]);
                let w = |p: DVec3, other: DVec3| {
                    let w = e.cross(p);
                    if w.dot(other) < 0.0 { w } else { -w }
                };
                let (w1, w2) = (w(n[0], n[1]), w(n[1], n[0]));
                let (e, w1, w2) = (un(e), un(w1), un(w2));
                let in_octant = |x: &Q3| {
                    (0..3).all(|i| {
                        if o >> i & 1 == 1 {
                            x[i] <= 0u32
                        } else {
                            x[i] >= 0u32
                        }
                    })
                };
                let neg: Q3 = e.clone().map(|x| -x);
                if in_octant(&e) || in_octant(&neg) {
                    return true;
                }
                // `w₂ + μ₀·(w₁ − w₂) + μ₁·e`, `μ₀ ∈ [0, 1]`.
                let x = [w2.clone(), sub(&w1, &w2), e];
                let mut rows = octant(o, &x);
                rows.push(([Q::from(1), zero()], zero()));
                rows.push(([Q::from(-1), zero()], Q::from(1)));
                feasible2(&rows)
            }
        })
    }
}

fn random_map(r: &mut Rng) -> [[i64; 3]; 3] {
    loop {
        let m = [0; 3].map(|_| [0; 3].map(|_| r.int(2) as i64));
        let c = OctantCone {
            inside: [false; 8],
            m,
        };
        if c.det() > 0 {
            return m;
        }
    }
}

/// The saddle partner's certificate on cones of closed octants under
/// integer maps, against partners inside an empty octant (so apart),
/// random ones, and ones swallowed by `A`: certified only where the
/// brute force finds the cones apart, with `n_B·d < 0` on every partner
/// face and `d` outside `A`; nearly every apart case certified; a face
/// or a single plane's normals never.
#[test]
fn saddle_partner_is_certified_only_where_the_cones_meet_at_the_apex() {
    let mut r = Rng::new(45);
    let (mut certified, mut apart, mut met) = (0, 0, 0);
    for case in 0..600 {
        let mut inside = [false; 8];
        while inside.iter().all(|&x| x) || inside.iter().all(|&x| !x) {
            inside = [0; 8].map(|_| r.below(2) == 0);
        }
        let a = OctantCone {
            inside,
            m: if case % 4 == 0 {
                [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
            } else {
                random_map(&mut r)
            },
        };
        // In the octants' frame: inside an empty octant, or anywhere.
        let empty: Vec<usize> = (0..8).filter(|&o| !inside[o]).collect();
        let full: Vec<usize> = (0..8).filter(|&o| inside[o]).collect();
        let pick = |r: &mut Rng, octs: &[usize]| {
            let o = octs[r.below(octs.len() as u64) as usize];
            let mut v = [0i64; 3];
            for (i, x) in v.iter_mut().enumerate() {
                let m = 1 + r.below(3) as i64;
                *x = if o >> i & 1 == 1 { -m } else { m };
            }
            a.map(v)
        };
        let gens = |r: &mut Rng| -> [DVec3; 3] {
            match case % 3 {
                0 => [0; 3].map(|_| pick(r, &empty)),
                1 => [0; 3].map(|_| pick(r, &full)),
                _ => [0; 3].map(|_| r.nonzero(3)),
            }
        };
        let partner = if r.below(3) == 0 {
            let n = [r.nonzero(3), r.nonzero(3)];
            if n[0].cross(n[1]) == DVec3::ZERO {
                continue;
            }
            Partner::Wedge(n)
        } else {
            let g = gens(&mut r);
            if g[0].cross(g[1]).dot(g[2]) == 0.0 {
                continue;
            }
            Partner::Corner(g)
        };
        let normals = partner.normals();
        let mut work = DirWork::default();
        let got = saddle_partner(&a.sectors(), &normals, &mut work);
        let meets = partner.meets(&a);
        if meets {
            met += 1;
        } else {
            apart += 1;
        }
        if let Some(d) = got {
            certified += 1;
            assert!(!meets, "{a:?} {partner:?}");
            assert!(normals.iter().all(|&n| dot(&qv(n), &d) < 0u32));
            assert!(!a.contains(&d), "{a:?} {partner:?}");
            assert!(work.checks > 0);
        }
    }
    assert!(met > 100, "{met} meeting");
    assert!(
        certified * 10 >= apart * 9,
        "{certified} certified of {apart} apart"
    );
    // A face's single normal, or a partner with one plane, isn't pointed.
    let a = OctantCone {
        inside: [true, false, false, false, false, false, false, true],
        m: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
    };
    let mut work = DirWork::default();
    assert_eq!(saddle_partner(&a.sectors(), &[DVec3::X], &mut work), None);
    assert_eq!(
        saddle_partner(&a.sectors(), &[DVec3::X, DVec3::X * 2.0], &mut work),
        None
    );
    // Two opposite octants, the partner's corner in an empty one between.
    let d = saddle_partner(
        &a.sectors(),
        &Partner::Corner([
            DVec3::new(2.0, -1.0, 1.0),
            DVec3::new(1.0, -2.0, 1.0),
            DVec3::new(1.0, -1.0, 2.0),
        ])
        .normals(),
        &mut work,
    );
    assert!(d.is_some());
    // Swallowed by `A`: the corner inside a full octant, apart from every
    // face but inside the cone, isn't certified; nor one across a face.
    for g in [
        [
            DVec3::new(2.0, 1.0, 1.0),
            DVec3::new(1.0, 2.0, 1.0),
            DVec3::new(1.0, 1.0, 2.0),
        ],
        [
            DVec3::new(2.0, 1.0, 1.0),
            DVec3::new(1.0, -2.0, 1.0),
            DVec3::new(1.0, -1.0, 2.0),
        ],
    ] {
        let normals = Partner::Corner(g).normals();
        assert_eq!(saddle_partner(&a.sectors(), &normals, &mut work), None);
    }
}

fn flip(s: Sign) -> Sign {
    match s {
        Sign::Pos => Sign::Neg,
        Sign::Neg => Sign::Pos,
    }
}

fn key() -> TieKey {
    TieKey {
        row: Row::ColumnHit,
        record: RecordKey::Vv(VvKey {
            a: Query::Vert(VertId(1)),
            b: Query::Vert(VertId(2)),
        }),
    }
}

/// The height of the plane `n·x = 0` above `v = 0` moved by `δ·(σ·d +
/// η·T₂ + η²·T₃ + η³·T₄)`, times `n·UP`: its sign.
fn moved_height(n: DVec3, sigma: Sign, d: &Q3, eta: &Q, delta: &Q) -> Sign {
    let s = if sigma == Sign::Pos {
        Q::from(1)
    } else {
        Q::from(-1)
    };
    let mut v: Q3 = d.clone().map(|x| x * &s);
    let mut e = eta.clone();
    for t in T {
        let t = qv(t);
        for i in 0..3 {
            v[i] += &e * &t[i];
        }
        e *= eta;
    }
    let h = -(dot(&qv(n), &v) * delta) * q(n.dot(UP));
    if h > 0u32 { Sign::Pos } else { Sign::Neg }
}

/// Records reading the rules' directions are the moved configuration's
/// signs: a vertex of `A` on the planes of `B` (the flush patches' and a
/// saddle partner's, through the vertex), each plane's height above the
/// moved vertex decided by `decide` against `A` moved by `δ·(σ_A·d +
/// η·T₂ + η²·T₃ + η³·T₄)`, `η = 2⁻¹⁶`, `δ = 2⁻⁶⁴`. A flush patch's faces
/// are decided alike (the vertex leaves every co-oriented patch's `B`
/// face on one side); under a union the saddle vertex enters its partner
/// (below every partner plane), otherwise it leaves it.
#[test]
fn records_on_rule_directions_are_the_moved_configurations_signs() {
    let (eta, delta) = (Q::from(1u32) >> 16u64, Q::from(1u32) >> 64u64);
    let mut r = Rng::new(46);
    let mut work = DirWork::default();
    let mut decided = 0;
    for _ in 0..200 {
        let patches: Vec<Patch> = (0..1 + r.below(3)).map(|_| co(r.nonzero(2))).collect();
        let Some(Flush::Dir(f)) = flush_flat(&patches, &mut work) else {
            continue;
        };
        for sigma in [Sign::Pos, Sign::Neg] {
            let mut sides = Vec::new();
            for p in &patches {
                if p.normal.dot(UP) == 0.0 {
                    continue;
                }
                let rec = PlaneHeight {
                    normal: p.normal,
                    offset: 0.0,
                    rates: [Rate::ZERO; ORDERS],
                    q: Moving {
                        at: DVec3::ZERO,
                        rates: rates_a(sigma, f.rate()),
                    },
                };
                let got = decide_flat(&rec, key()).0.expect("a sign");
                assert_eq!(got, moved_height(p.normal, sigma, &f.d(), &eta, &delta));
                // Above or below the face along its own normal: one side
                // for every patch.
                let along = if p.normal.dot(UP) > 0.0 {
                    got
                } else {
                    flip(got)
                };
                sides.push(along);
                decided += 1;
            }
            assert!(sides.windows(2).all(|w| w[0] == w[1]), "{patches:?}");
        }
    }
    assert!(decided > 100);
    // The saddle vertex of two opposite octants against a partner corner
    // in an empty octant between them.
    let a = OctantCone {
        inside: [true, false, false, false, false, false, false, true],
        m: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
    };
    let partner = Partner::Corner([
        DVec3::new(3.0, -1.0, 1.0),
        DVec3::new(1.0, -2.0, 1.0),
        DVec3::new(1.0, -1.0, 2.0),
    ]);
    let normals = partner.normals();
    let d = saddle_partner(&a.sectors(), &normals, &mut work).expect("apart");
    for sigma in [Sign::Pos, Sign::Neg] {
        for &n in &normals {
            let rec = PlaneHeight {
                normal: n,
                offset: 0.0,
                rates: [Rate::ZERO; ORDERS],
                q: Moving {
                    at: DVec3::ZERO,
                    rates: rates_a(sigma, Rate::cleared(d.clone())),
                },
            };
            let got = decide_flat(&rec, key()).0.expect("a sign");
            assert_eq!(got, moved_height(n, sigma, &d, &eta, &delta));
            // Inside the partner's plane (the plane above along its
            // outward normal) under a union, outside otherwise.
            let above = if n.dot(UP) > 0.0 { got } else { flip(got) };
            assert_eq!(above == Sign::Pos, sigma == Sign::Pos, "{n}");
        }
    }
}
