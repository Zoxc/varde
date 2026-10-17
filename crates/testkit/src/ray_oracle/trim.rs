//! The face's loops in its UV plane: the winding number of a UV point
//! and whether a closed UV box meets no p-curve, both exact.

use super::bern::{Bern, Cut, normalize_all};
use super::exact::{Dy, common};
use malachite::Integer;

/// One rational quadratic p-curve piece, homogeneous `(U, V, W)` up to
/// a positive factor, with the direction its loop runs it in.
pub(super) struct PSpan {
    pub(super) u: [Integer; 3],
    pub(super) v: [Integer; 3],
    pub(super) w: [Integer; 3],
    /// `+1` along the parameter, `-1` against it (a reversed coedge).
    pub(super) sign: i64,
    /// `[u_min, u_max, v_min, v_max]` of the affine control points
    /// (rounded to nearest: compared with a margin).
    pub(super) bbox: [f64; 4],
}

pub(super) struct Trims {
    pub(super) spans: Vec<PSpan>,
}

/// `a·x − b·W` for the UV value `b`, as a univariate form.
fn offset(x: &[Integer; 3], w: &[Integer; 3], b: &Dy) -> Bern {
    let v: Vec<Dy> = (0..3)
        .map(|i| {
            Dy {
                m: x[i].clone(),
                e: 0,
            }
            .sub(
                &Dy {
                    m: w[i].clone(),
                    e: 0,
                }
                .mul(b),
            )
        })
        .collect();
    Bern::along(0, common(&v))
}

fn margin(a: f64, b: f64) -> f64 {
    (a.abs() + b.abs()) * 1e-9 + 1e-300
}

fn sgn(x: &Integer) -> i64 {
    if *x > 0 {
        1
    } else if *x < 0 {
        -1
    } else {
        0
    }
}

impl Trims {
    /// The winding number of all loops around the UV point `p`, which
    /// must not lie on a p-curve; `None` past `cap` subdivisions deep.
    ///
    /// Counts the signed crossings of the ray `p + r·(1, 0)`, `r > 0`:
    /// a piece of p-curve where `U − p_u·W > 0` throughout adds
    /// `(sign V(end) − sign V(start)) / 2` (`V` relative to `p_v`), so
    /// an end exactly on the ray counts half from each side and the
    /// halves meet at the joins.
    pub(super) fn winding(&self, p: &[Dy; 2], cap: u32) -> Option<i64> {
        let (pu, pv) = (p[0].approx(), p[1].approx());
        let mut twice = 0;
        for s in &self.spans {
            let [umin, umax, vmin, vmax] = s.bbox;
            if vmin > pv + margin(vmin, pv)
                || vmax < pv - margin(vmax, pv)
                || umax < pu - margin(umax, pu)
            {
                continue;
            }
            let g = offset(&s.v, &s.w, &p[1]);
            let h = offset(&s.u, &s.w, &p[0]);
            let _ = umin;
            let mut stack = vec![(g, h, 0u32)];
            let mut sum = 0;
            while let Some((g, h, depth)) = stack.pop() {
                let gs = g.signs();
                if gs.strict() {
                    continue;
                }
                let hs = h.signs();
                if hs.negative() {
                    continue;
                }
                if hs.positive() {
                    sum += sgn(&g.c[2]) - sgn(&g.c[0]);
                    continue;
                }
                if depth >= cap {
                    return None;
                }
                let (g0, g1) = g.split(0, Cut::HALF);
                let (h0, h1) = h.split(0, Cut::HALF);
                stack.push((g0, h0, depth + 1));
                stack.push((g1, h1, depth + 1));
            }
            twice += s.sign * sum;
        }
        assert!(twice % 2 == 0, "the face's loops don't close in UV");
        Some(twice / 2)
    }

    /// Every p-curve piece, whole, to start [`refine`] from.
    pub(super) fn pieces(&self) -> Vec<TrimPiece> {
        self.spans
            .iter()
            .map(|s| TrimPiece {
                u: Bern::along(0, s.u.to_vec()),
                v: Bern::along(0, s.v.to_vec()),
                w: Bern::along(0, s.w.to_vec()),
            })
            .collect()
    }
}

/// A part of a p-curve piece (its parameter interval subdivided), up to
/// a positive factor.
#[derive(Clone, Debug)]
pub(super) struct TrimPiece {
    u: Bern,
    v: Bern,
    w: Bern,
}

/// The parts of `pieces` that may meet the closed UV box `[lo, hi]`, and
/// whether none certainly does (a part is split until it is outside the
/// box or has an end in it, `cap` levels at most). `pieces` must hold
/// every part of the trims that meets a box containing this one (a
/// parent's result), so a subdivision passes them down and refines them
/// only near its boxes.
pub(super) fn refine(
    pieces: Vec<TrimPiece>,
    lo: &[Dy; 2],
    hi: &[Dy; 2],
    cap: u32,
) -> (Vec<TrimPiece>, bool) {
    let mut out = Vec::new();
    let mut free = true;
    let mut work: Vec<(TrimPiece, u32)> = pieces.into_iter().map(|p| (p, 0)).collect();
    while let Some((p, depth)) = work.pop() {
        // Each is positive where the curve is strictly inside the box's
        // slab on that side.
        let neg = |b: Bern| Bern {
            n: b.n,
            c: b.c.into_iter().map(|x| -x).collect(),
        };
        let polys = [
            offset_b(&p.u, &p.w, &lo[0]),
            neg(offset_b(&p.u, &p.w, &hi[0])),
            offset_b(&p.v, &p.w, &lo[1]),
            neg(offset_b(&p.v, &p.w, &hi[1])),
        ];
        if polys.iter().any(|q| q.signs().negative()) {
            continue;
        }
        // A curve point (an end of the part) in the closed box, or not
        // settled: keep it.
        let ends_in = polys.iter().all(|q| q.c[0] >= 0) || polys.iter().all(|q| q.c[2] >= 0);
        if ends_in || depth >= cap {
            free = false;
            out.push(p);
            continue;
        }
        // Split alike and scaled alike: (U, V, W) is homogeneous.
        let (mut u0, mut u1) = p.u.split_scaled(0, Cut::HALF);
        let (mut v0, mut v1) = p.v.split_scaled(0, Cut::HALF);
        let (mut w0, mut w1) = p.w.split_scaled(0, Cut::HALF);
        normalize_all(&mut [&mut u0, &mut v0, &mut w0]);
        normalize_all(&mut [&mut u1, &mut v1, &mut w1]);
        work.push((
            TrimPiece {
                u: u0,
                v: v0,
                w: w0,
            },
            depth + 1,
        ));
        work.push((
            TrimPiece {
                u: u1,
                v: v1,
                w: w1,
            },
            depth + 1,
        ));
    }
    (out, free)
}

/// `x − b·w` for univariate forms `x`, `w` with one common factor.
fn offset_b(x: &Bern, w: &Bern, b: &Dy) -> Bern {
    let v: Vec<Dy> = (0..3)
        .map(|i| {
            Dy {
                m: x.c[i].clone(),
                e: 0,
            }
            .sub(
                &Dy {
                    m: w.c[i].clone(),
                    e: 0,
                }
                .mul(b),
            )
        })
        .collect();
    Bern::along(0, common(&v))
}
