//! The crossing-order row against the really moved geometry: an edge
//! crossing two faces at one point (a tie), the edge or the faces moving
//! by an affine field (motion 1) or by `Tₖ`, the crossings' parameters
//! solved again on the moved geometry, and their difference's central
//! difference compared with the row: flat exactly in rationals, curved in
//! big floats on rational quadratic edges against planes and spheres.
//! Then the flat row through `decide` against `A` moved by `δ·(σ_A·d +
//! η·T₂ + η²·T₃ + η³·T₄)` with `δ ≪ η`: the answer is that configuration's
//! exact sign.

use malachite::Rational as Q;

use super::*;
use crate::bezier::{HomCurve, Scalar};
use crate::ids::VertId;
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::Bounded;
use crate::num::decide::{Fail, Row, TieKey};
use crate::num::exact::{order_value, value};
use crate::perturb::{T, curve_point, decide_flat};
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

    fn field(&mut self) -> Field {
        Field {
            f0: self.ints(8) / 8.0,
            j: [0; 3].map(|_| self.ints(8) / 8.0),
        }
    }
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn qv(v: DVec3) -> [Q; 3] {
    [q(v.x), q(v.y), q(v.z)]
}

fn qdot(a: &[Q; 3], b: &[Q; 3]) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn qsub(a: &[Q; 3], b: &[Q; 3]) -> [Q; 3] {
    [0, 1, 2].map(|i| &a[i] - &b[i])
}

fn qpow2(e: i64) -> Q {
    if e >= 0 {
        Q::from(1u32) << u64::try_from(e).expect("non-negative")
    } else {
        Q::from(1u32) >> u64::try_from(-e).expect("positive")
    }
}

/// An affine field `x ↦ f0 + J·x` (`J` by rows), exact in `f64` on the
/// tests' small dyadic points.
#[derive(Clone, Copy, Debug)]
struct Field {
    f0: DVec3,
    j: [DVec3; 3],
}

impl Field {
    const ZERO: Field = Field {
        f0: DVec3::ZERO,
        j: [DVec3::ZERO; 3],
    };

    fn constant(t: DVec3) -> Field {
        Field {
            f0: t,
            j: [DVec3::ZERO; 3],
        }
    }

    fn at(&self, x: DVec3) -> DVec3 {
        self.f0 + DVec3::new(self.j[0].dot(x), self.j[1].dot(x), self.j[2].dot(x))
    }

    fn neg(&self) -> Field {
        Field {
            f0: -self.f0,
            j: self.j.map(|r| -r),
        }
    }
}

/// An exact affine field `x ↦ g + J·x`.
#[derive(Clone, Debug)]
struct QField {
    g: [Q; 3],
    j: [[Q; 3]; 3],
}

impl QField {
    fn of(f: &Field) -> QField {
        QField {
            g: qv(f.f0),
            j: f.j.map(qv),
        }
    }

    /// `σ·F + η·T₂ + η²·T₃ + η³·T₄`: the moved configuration's motion.
    fn combined(own: &Field, eta: &Q) -> QField {
        let mut f = QField::of(own);
        let mut e = eta.clone();
        for t in T {
            let t = qv(t);
            for i in 0..3 {
                f.g[i] += &e * &t[i];
            }
            e *= eta;
        }
        f
    }

    fn at(&self, x: &[Q; 3]) -> [Q; 3] {
        [0, 1, 2].map(|i| &self.g[i] + qdot(&self.j[i], x))
    }

    /// `x + δ·F(x)`.
    fn moved(&self, x: &[Q; 3], delta: &Q) -> [Q; 3] {
        let f = self.at(x);
        [0, 1, 2].map(|i| &x[i] + delta * &f[i])
    }

    /// The plane `n·x = c` carried by `x ↦ x + δ·F(x)`: `n′ = (I +
    /// δJ)^{−T}·n` up to the positive factor `det(I + δJ)` (for small
    /// `δ`), so `n′ = cof(I + δJ)·n`, and `c′ = n′·(image of a point of
    /// the plane)`.
    fn moved_plane(&self, n: &[Q; 3], c: &Q, delta: &Q) -> ([Q; 3], Q) {
        let m: [[Q; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|k| {
                let id = if i == k { Q::from(1u32) } else { Q::from(0u32) };
                id + delta * &self.j[i][k]
            })
        });
        // The adjugate's transpose is the cofactor matrix.
        let cof = |i: usize, k: usize| {
            let (i1, i2) = ((i + 1) % 3, (i + 2) % 3);
            let (k1, k2) = ((k + 1) % 3, (k + 2) % 3);
            &m[i1][k1] * &m[i2][k2] - &m[i1][k2] * &m[i2][k1]
        };
        let n2: [Q; 3] = std::array::from_fn(|i| (0..3).map(|k| cof(i, k) * &n[k]).sum());
        // A point of the plane: along the largest component of `n`.
        let big = (0..3).max_by_key(|&i| if n[i] < 0u32 { -&n[i] } else { n[i].clone() });
        let big = big.expect("three");
        let mut p: [Q; 3] = std::array::from_fn(|_| Q::from(0u32));
        p[big] = c / &n[big];
        let c2 = qdot(&n2, &self.moved(&p, delta));
        (n2, c2)
    }
}

/// A tie: the edge from `a` to `b` crossing both planes at `x`, and which
/// side moves.
#[derive(Clone, Debug)]
struct Tie {
    line: [DVec3; 2],
    normals: [DVec3; 2],
    offsets: [f64; 2],
    x: DVec3,
    edge_moves: bool,
}

fn random_tie(r: &mut Rng) -> Tie {
    loop {
        let x = r.ints(8) / 4.0;
        let u = r.nonzero(3);
        let t = r.below(5) as f64 / 4.0;
        let a = x - u * t;
        let normals = [r.nonzero(3), r.nonzero(3)];
        if normals.iter().any(|n| n.dot(u) == 0.0) || normals[0].cross(normals[1]) == DVec3::ZERO {
            continue;
        }
        return Tie {
            line: [a, a + u],
            offsets: normals.map(|n| n.dot(x)),
            normals,
            x,
            edge_moves: r.below(2) == 0,
        };
    }
}

impl Tie {
    /// The row with the moving side's rates from `own` (motion 1) and the
    /// translations, all read at the crossing `x`.
    fn row(&self, own: &Field) -> OrderRow {
        let a_rates: [Rate; ORDERS] =
            std::array::from_fn(|k| if k == 0 { own.at(self.x) } else { T[k - 1] }.into());
        let none = || [Rate::ZERO; ORDERS];
        let face = |i: usize| OrderFace {
            normal: self.normals[i],
            offset: self.offsets[i],
            rates: if self.edge_moves {
                none()
            } else {
                a_rates.clone()
            },
            edge_rates: if self.edge_moves {
                a_rates.clone()
            } else {
                none()
            },
        };
        let (row, _) = OrderRow::new(self.line, [face(0), face(1)]);
        row.expect("transversal")
    }

    /// `t₂ − t₁` on the geometry moved by `x ↦ x + δ·F(x)` on the moving
    /// side, exactly.
    fn gap(&self, f: &QField, delta: &Q) -> Q {
        let [a, b] = self.line.map(qv);
        let (a, b) = if self.edge_moves {
            (f.moved(&a, delta), f.moved(&b, delta))
        } else {
            (a, b)
        };
        let c = qsub(&b, &a);
        let t = |i: usize| {
            let (n, off) = (qv(self.normals[i]), q(self.offsets[i]));
            let (n, off) = if self.edge_moves {
                (n, off)
            } else {
                f.moved_plane(&n, &off, delta)
            };
            (off - qdot(&n, &a)) / qdot(&n, &c)
        };
        t(1) - t(0)
    }

    /// `|n₁·C′|·|n₂·C′|`, the row's factor over `t₂ − t₁`.
    fn factor(&self) -> Q {
        let [a, b] = self.line;
        let c = qv(b - a);
        let f = |i: usize| {
            let v = qdot(&qv(self.normals[i]), &c);
            if v < 0u32 { -v } else { v }
        };
        f(0) * f(1)
    }
}

fn order_q<P: Pred>(p: &P, k: usize) -> Q {
    match k {
        1 => order_value::<P, 1>(p),
        2 => order_value::<P, 2>(p),
        3 => order_value::<P, 3>(p),
        _ => order_value::<P, 4>(p),
    }
}

/// The flat row at ties: its value zero, each order the derivative of
/// `t₂ − t₁` on the really moved geometry (the moving plane turned by the
/// field, the edge turned with its ends) times the common factor, the
/// central differences within `δ²` of it; the facings certified, and the
/// row a positive multiple of `t₂ − t₁` away from ties too.
#[test]
fn flat_order_rows_are_the_derivatives_at_ties() {
    let mut r = Rng::new(31);
    for _ in 0..200 {
        let tie = random_tie(&mut r);
        let own = r.field();
        let row = tie.row(&own);
        assert_eq!(value(&row), 0u32, "{tie:?}");
        let factor = tie.factor();
        for k in 1..=ORDERS {
            let fld = QField::of(&if k == 1 {
                own
            } else {
                Field::constant(T[k - 2])
            });
            let want = order_q(&row, k) / &factor;
            for e in [20i64, 40] {
                let d = qpow2(-e);
                let fd = (tie.gap(&fld, &d) - tie.gap(&fld, &-&d)) / (Q::from(2u32) * &d);
                let off = fd - &want;
                let off = if off < 0u32 { -off } else { off };
                assert!(off <= qpow2(30 - 2 * e), "{tie:?} order {k} δ = 2^-{e}");
            }
        }
        // Moved off the tie by a translation of the moving side: the
        // value is the gap times the factor, exactly.
        let shift = r.ints(4) / 8.0;
        let moved_tie = Tie {
            line: if tie.edge_moves {
                tie.line.map(|p| p + shift)
            } else {
                tie.line
            },
            offsets: if tie.edge_moves {
                tie.offsets
            } else {
                [0, 1].map(|i| tie.offsets[i] + tie.normals[i].dot(shift))
            },
            ..tie.clone()
        };
        let moved_row = moved_tie.row(&Field::ZERO);
        let gap = tie.gap(&QField::of(&Field::constant(shift)), &Q::from(1u32));
        assert_eq!(value(&moved_row), gap * moved_tie.factor(), "{tie:?}");
    }
    // Along a plane's direction the edge's crossing never moves: no
    // crossing to order.
    let face = OrderFace {
        normal: DVec3::Z,
        offset: 0.0,
        rates: [Rate::ZERO; ORDERS],
        edge_rates: [Rate::ZERO; ORDERS],
    };
    let (none, work) = OrderRow::new([DVec3::ZERO, DVec3::X], [face.clone(), face]);
    assert_eq!(none, None);
    assert_eq!(work.filters(), 1, "the first facing, then none");
}

fn key() -> TieKey {
    TieKey {
        row: Row::CrossingOrder,
        record: RecordKey::Vv(VvKey {
            a: Query::Vert(VertId(1)),
            b: Query::Vert(VertId(2)),
        }),
    }
}

/// The flat row through `decide`, at ties: its answer is the sign of
/// `t₂ − t₁` in the moved configuration, `A` moved by `δ·(σ_A·F + η·T₂ +
/// η²·T₃ + η³·T₄)` with `η = 2⁻¹⁶` and `δ = 2⁻²⁴⁰ ≪ η³` (the field turns
/// the moving element, so `δ²` terms appear, below the deciding order).
/// Directions that leave the order unmoved at motion 1 (the field zero,
/// or along both planes) are decided by the translations, and so is every
/// motion's sign realised.
#[test]
fn flat_order_answers_are_the_moved_configurations_signs() {
    let mut r = Rng::new(32);
    let (eta, delta) = (qpow2(-16), qpow2(-240));
    let mut by_t = 0;
    for i in 0..300 {
        let tie = random_tie(&mut r);
        let own = match i % 3 {
            0 => r.field(),
            1 => Field::ZERO,
            // Along both planes: the crossings stay put at first order.
            _ => Field::constant(tie.normals[0].cross(tie.normals[1]) / 8.0),
        };
        let own = if r.below(2) == 0 { own } else { own.neg() };
        let row = tie.row(&own);
        let got = decide_flat(&row, key()).0;
        let moved = tie.gap(&QField::combined(&own, &eta), &delta);
        let want = if moved > 0u32 {
            Sign::Pos
        } else if moved < 0u32 {
            Sign::Neg
        } else {
            panic!("no first-order motion: {tie:?}")
        };
        assert_eq!(got, Ok(want), "{tie:?} {own:?}");
        if order_q(&row, 1) == 0u32 {
            by_t += 1;
        }
    }
    assert!(by_t > 50, "{by_t} decided by the translations");
    // Parallel planes through the crossing: one plane, no order at any
    // motion of a translation: the guard.
    let tie = Tie {
        line: [DVec3::ZERO, DVec3::new(1.0, 2.0, 3.0)],
        normals: [DVec3::new(1.0, 1.0, 1.0), DVec3::new(2.0, 2.0, 2.0)],
        offsets: [3.0, 6.0],
        x: DVec3::new(0.5, 1.0, 1.5),
        edge_moves: true,
    };
    assert_eq!(
        decide_flat(&tie.row(&Field::constant(DVec3::X)), key()).0,
        Err(Fail::Internal { key: key() })
    );
}

type B = Bounded<Big>;

const W: Width = Width::ALL[1];

fn big(x: f64) -> B {
    B::exact(Big::from_f64(x, W))
}

/// `2^e`, exactly.
fn p2(e: i32) -> f64 {
    f64::from_bits(((1023 + e) as u64) << 52)
}

fn fb(x: &B) -> f64 {
    x.v.to_f64()
}

fn bv(v: DVec3) -> [B; 3] {
    [big(v.x), big(v.y), big(v.z)]
}

fn bdot(a: &[B; 3], b: &[B; 3]) -> B {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

fn bsub(a: &[B; 3], b: &[B; 3]) -> [B; 3] {
    [0, 1, 2].map(|i| a[i].sub(&b[i]))
}

/// The value alone: the moved geometry's solvers only need values.
fn clean(x: &B) -> B {
    B::exact(x.v.clone())
}

/// `F` at `x` in big floats, scaled by `s`.
fn field_big(f: &Field, x: &[B; 3], s: &B) -> [B; 3] {
    let f0 = bv(f.f0);
    [0, 1, 2].map(|i| f0[i].add(&bdot(&bv(f.j[i]), x)).mul(s))
}

/// `x + δ·F(x)`.
fn moved_big(f: &Field, x: &[B; 3], delta: &B) -> [B; 3] {
    let m = field_big(f, x, delta);
    [0, 1, 2].map(|i| x[i].add(&m[i]))
}

/// `y` with `y + δ·F(y) = x`, by fixed-point steps (`δ` tiny).
fn unmoved_big(f: &Field, x: &[B; 3], delta: &B) -> [B; 3] {
    let mut y = x.clone();
    for _ in 0..24 {
        let m = field_big(f, &y, delta);
        y = [0, 1, 2].map(|i| clean(&x[i].sub(&m[i])));
    }
    y
}

/// A curved face through the crossing: a plane `n·y = n·x`, or a sphere
/// through `x` about `x − v`, its outward normal `v` there.
#[derive(Clone, Debug)]
enum Surf {
    Plane(DVec3),
    Sphere(DVec3),
}

impl Surf {
    /// The face's implicit value at `y`, its geometry moved by `δ·F`
    /// (when `moves`): the unmoved one at the preimage of `y`.
    fn implicit(&self, x: &[B; 3], y: &[B; 3], f: &Field, delta: &B, moves: bool) -> B {
        let y = if moves {
            unmoved_big(f, y, delta)
        } else {
            y.clone()
        };
        match self {
            Surf::Plane(n) => bdot(&bv(*n), &bsub(&y, x)),
            Surf::Sphere(v) => {
                let o = bsub(x, &bv(*v));
                let d = bsub(&y, &o);
                bdot(&d, &d).sub(&bdot(&bv(*v), &bv(*v)))
            }
        }
    }

    fn normal(&self) -> DVec3 {
        match self {
            Surf::Plane(n) | Surf::Sphere(n) => *n,
        }
    }
}

/// `t` near `t` solving `φ(t) = 0`, by Newton's method with a central
/// difference slope.
fn solve(mut t: B, phi: impl Fn(&B) -> B) -> B {
    let h = big(p2(-100));
    for _ in 0..12 {
        let slope = phi(&t.add(&h)).sub(&phi(&t.sub(&h))).div(&h.add(&h));
        t = clean(&t.sub(&phi(&t).div(&slope)));
    }
    t
}

/// The curved row at ties on random rational quadratic edges crossing a
/// plane and a sphere (or two of either) at the curve's middle, the edge
/// or the faces moving by an affine field or `Tₖ`: each order is the
/// derivative of `‖C′‖·(t₂ − t₁)` on the moved geometry, its parameters
/// solved again, within `δ²`; the value is zero; an order no element
/// moves along is an exact zero; an edge along a face's plane has no row.
#[test]
fn curved_order_rows_are_the_derivatives_at_ties() {
    let mut r = Rng::new(33);
    let mut checked = 0;
    while checked < 24 {
        let ctrl: Vec<(DVec3, f64)> = (0..3)
            .map(|_| (r.ints(8) / 2.0, 1.0 + r.below(4) as f64 / 2.0))
            .collect();
        let curve = |mv: &dyn Fn(&[B; 3]) -> [B; 3]| -> HomCurve<B, 3> {
            HomCurve::new(ctrl.iter().map(|(p, w)| {
                let w = big(*w);
                (mv(&bv(*p)).map(|v| v.mul(&w)), w)
            }))
            .expect("a span")
        };
        let c0 = curve(&|p| p.clone());
        let half = big(0.5);
        let (x, tangent_w2) = curve_point(&c0, &half);
        let (_, w) = c0.eval_hom(&half);
        // `C′ = (W·P′ − W′·P)/W²`.
        let tangent = tangent_w2.map(|v| v.div(&w.mul(&w)));
        let x = x.map(|v| clean(&v));
        let surfs = [0, 1].map(|_| {
            let v = r.nonzero(3);
            if r.below(2) == 0 {
                Surf::Plane(v)
            } else {
                Surf::Sphere(v)
            }
        });
        let tf: Vec<f64> = tangent.iter().map(fb).collect();
        let tf = DVec3::new(tf[0], tf[1], tf[2]);
        if surfs
            .iter()
            .any(|s| s.normal().normalize().dot(tf.normalize()).abs() < 0.2)
        {
            continue;
        }
        let edge_moves = r.below(2) == 0;
        let own = r.field();
        let at_x = |f: &Field| field_big(f, &x, &big(1.0));
        let zero = || [0; 3].map(|_| big(0.0));
        let a_rates: [[B; 3]; ORDERS] =
            std::array::from_fn(|k| if k == 0 { at_x(&own) } else { bv(T[k - 1]) });
        let none: [[B; 3]; ORDERS] = std::array::from_fn(|_| zero());
        let (face_rates, edge_rates) = if edge_moves {
            (
                [none.clone(), none.clone()],
                [a_rates.clone(), a_rates.clone()],
            )
        } else {
            (
                [a_rates.clone(), a_rates.clone()],
                [none.clone(), none.clone()],
            )
        };
        let rec = OrderAt {
            tangent: tangent.clone(),
            t: [half.clone(), half.clone()],
            normals: surfs.clone().map(|s| bv(s.normal())),
            face_rates,
            edge_rates,
        };
        let row = order_row(&rec).expect("transversal");
        assert_eq!(fb(&row.g), 0.0);
        let len = bdot(&tangent, &tangent).sqrt_nonneg();
        for k in 1..=ORDERS {
            let fld = if k == 1 {
                own
            } else {
                Field::constant(T[k - 2])
            };
            let gap = |delta: &B| {
                let c = if edge_moves {
                    curve(&|p| moved_big(&fld, p, delta))
                } else {
                    c0.clone()
                };
                let t = |i: usize| {
                    solve(half.clone(), |t| {
                        let (p, w) = c.eval_hom(t);
                        let y = p.map(|v| v.div(&w));
                        surfs[i].implicit(&x, &y, &fld, delta, !edge_moves)
                    })
                };
                len.mul(&t(1).sub(&t(0)))
            };
            let want = &row.ord[k - 1];
            for e in [-20, -30] {
                let d = big(p2(e));
                let fd = gap(&d).sub(&gap(&d.neg())).div(&d.add(&d));
                let off = fb(&fd.sub(want)).abs();
                assert!(
                    off <= 1e9 * p2(2 * e) * (1.0 + fb(want).abs()),
                    "order {k} δ = 2^{e}: {} vs {}",
                    fb(&fd),
                    fb(want)
                );
            }
        }
        // `B`'s elements alone: every order an exact zero.
        let fixed = OrderAt {
            face_rates: [none.clone(), none.clone()],
            edge_rates: [none.clone(), none.clone()],
            ..rec.clone()
        };
        let fixed = order_row(&fixed).expect("transversal");
        assert!(fixed.ord.iter().all(|o| o.is_int(0)));
        checked += 1;
    }
    // A face's plane along the edge: no certified facing, no row.
    let one = big(1.0);
    let zero = big(0.0);
    let none: [[B; 3]; ORDERS] = std::array::from_fn(|_| [0; 3].map(|_| zero.clone()));
    let along = OrderAt {
        tangent: [one.clone(), zero.clone(), zero.clone()],
        t: [zero.clone(), zero.clone()],
        normals: [
            [zero.clone(), zero.clone(), one.clone()],
            [one.clone(), zero.clone(), zero.clone()],
        ],
        face_rates: [none.clone(), none.clone()],
        edge_rates: [none.clone(), none],
    };
    assert_eq!(order_row(&along), None);
}
