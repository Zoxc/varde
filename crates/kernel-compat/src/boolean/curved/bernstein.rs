//! Polynomials on `[0, 1]` in the Bernstein basis: products, evaluation
//! and root isolation.
//!
//! A polynomial of degree `n` is `Σ c_k·C(n, k)·t^k·(1 − t)^(n−k)`. Its
//! roots in `(0, 1)` are isolated by Descartes' rule of signs on the
//! coefficients: no sign change means no root, one means exactly one
//! (simple) root, found by bisection; otherwise the polynomial is split
//! at `½` by de Casteljau and each half looked at again. Everything is
//! `+ − ×` and comparisons, so the roots are the same on every platform.

/// How deep the halving goes before a stretch with several sign changes
/// is decided from its ends' signs alone: `2^-48` of the whole.
const MAX_DEPTH: u32 = 48;

/// The Bernstein coefficients of the product of two quadratics.
pub(crate) fn product2(f: [f64; 3], g: [f64; 3]) -> [f64; 5] {
    [
        f[0] * g[0],
        (f[0] * g[1] + f[1] * g[0]) / 2.0,
        (f[0] * g[2] + 4.0 * f[1] * g[1] + f[2] * g[0]) / 6.0,
        (f[1] * g[2] + f[2] * g[1]) / 2.0,
        f[2] * g[2],
    ]
}

/// The value at `t`, by de Casteljau.
pub(crate) fn eval(c: &[f64], t: f64) -> f64 {
    let mut b = [0.0; 9];
    let n = c.len();
    b[..n].copy_from_slice(c);
    for k in 1..n {
        for i in 0..n - k {
            b[i] = b[i] * (1.0 - t) + b[i + 1] * t;
        }
    }
    b[0]
}

/// The two halves at `½`, by de Casteljau.
fn halves(c: &[f64]) -> ([f64; 9], [f64; 9]) {
    let n = c.len();
    let mut b = [0.0; 9];
    b[..n].copy_from_slice(c);
    let (mut left, mut right) = ([0.0; 9], [0.0; 9]);
    left[0] = b[0];
    right[n - 1] = b[n - 1];
    for k in 1..n {
        for i in 0..n - k {
            b[i] = (b[i] + b[i + 1]) * 0.5;
        }
        left[k] = b[0];
        right[n - 1 - k] = b[n - 1 - k];
    }
    (left, right)
}

fn sign(x: f64) -> i8 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// The number of sign changes along the coefficients, zeros skipped.
fn changes(c: &[f64]) -> usize {
    let mut last = 0;
    let mut n = 0;
    for &x in c {
        let s = sign(x);
        if s != 0 {
            if last != 0 && s != last {
                n += 1;
            }
            last = s;
        }
    }
    n
}

/// The roots in `(0, 1)` of the polynomial with Bernstein coefficients
/// `c` (degree 1 to 8), ascending. Simple roots are each found once. A
/// double root (a tangency) comes out as rounding has it: as none, or as
/// two roots close together. A cluster of roots closer together than the
/// halving goes is decided by the signs at its stretch's ends: one root
/// if they differ, none if not. Coefficients that aren't finite give no
/// roots.
pub(crate) fn roots(c: &[f64]) -> Vec<f64> {
    let mut out = Vec::new();
    if (2..=9).contains(&c.len()) && c.iter().all(|x| x.is_finite()) {
        isolate(c, 0.0, 1.0, 0, &mut out);
    }
    out
}

fn isolate(c: &[f64], lo: f64, hi: f64, depth: u32, out: &mut Vec<f64>) {
    let n = c.len();
    let v = changes(c);
    if v == 0 {
        return;
    }
    let (first, last) = (sign(c[0]), sign(c[n - 1]));
    if v == 1 && first != 0 && last != 0 {
        out.push(lo + (hi - lo) * bisect(c, first));
        return;
    }
    let mid = (lo + hi) * 0.5;
    if depth >= MAX_DEPTH || mid <= lo || mid >= hi {
        if first * last < 0 {
            out.push(mid);
        }
        return;
    }
    let (left, right) = halves(c);
    isolate(&left[..n], lo, mid, depth + 1, out);
    if left[n - 1] == 0.0 {
        // A root exactly at the split.
        out.push(mid);
    }
    isolate(&right[..n], mid, hi, depth + 1, out);
}

/// The one root in `(0, 1)` of a polynomial whose value at 0 has the sign
/// `first` and at 1 the other, by bisection to the last bit.
fn bisect(c: &[f64], first: i8) -> f64 {
    let (mut a, mut b) = (0.0f64, 1.0f64);
    for _ in 0..80 {
        let m = (a + b) * 0.5;
        if m <= a || m >= b {
            break;
        }
        match sign(eval(c, m)) {
            0 => return m,
            s if s == first => a = m,
            _ => b = m,
        }
    }
    (a + b) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rng::Rng;

    /// The Bernstein coefficients of `Π (t − r)` over `rs`, times `scale`.
    fn from_roots(rs: &[f64], scale: f64) -> Vec<f64> {
        // Start from the constant, and multiply by (t − r), whose
        // Bernstein coefficients are (−r, 1 − r), degree-elevating as we
        // go.
        let mut c = vec![scale];
        for &r in rs {
            let n = c.len(); // degree n − 1 → n
            let mut next = vec![0.0; n + 1];
            for (k, slot) in next.iter_mut().enumerate() {
                // (t − r)·p(t): coefficient k of degree n from those of
                // degree n − 1: (k/n)·c[k−1]·(1 − r) + ((n − k)/n)·c[k]·(−r).
                let nf = n as f64;
                let a = if k > 0 {
                    c[k - 1] * (1.0 - r) * k as f64 / nf
                } else {
                    0.0
                };
                let b = if k < n {
                    c[k] * (-r) * (n - k) as f64 / nf
                } else {
                    0.0
                };
                *slot = a + b;
            }
            c = next;
        }
        c
    }

    #[test]
    fn products_multiply() {
        let f = [1.0, -2.0, 0.5];
        let g = [0.25, 3.0, -1.0];
        let p = product2(f, g);
        for k in 0..=10 {
            let t = k as f64 / 10.0;
            let want = eval(&f, t) * eval(&g, t);
            assert!((eval(&p, t) - want).abs() < 1e-12, "{t}");
        }
    }

    #[test]
    fn simple_roots_are_found() {
        let mut rng = Rng::new(3);
        for _ in 0..500 {
            let k = 1 + (rng.next_u64() % 4) as usize;
            let mut rs: Vec<f64> = (0..k).map(|_| rng.range(-0.5, 1.5)).collect();
            rs.sort_by(f64::total_cmp);
            let c = from_roots(&rs, rng.range(0.1, 10.0));
            let want: Vec<f64> = rs.iter().copied().filter(|&r| r > 0.0 && r < 1.0).collect();
            // Roots too close to tell apart may merge; skip those draws.
            if rs.windows(2).any(|w| w[1] - w[0] < 1e-6) {
                continue;
            }
            let got = roots(&c);
            assert_eq!(got.len(), want.len(), "{rs:?} {got:?}");
            for (g, w) in got.iter().zip(&want) {
                assert!((g - w).abs() < 1e-9, "{rs:?} {got:?}");
            }
        }
    }

    #[test]
    fn double_roots_are_none_or_two_close_ones() {
        let near = |r: &[f64], x: f64| r.iter().all(|&r| (r - x).abs() < 1e-6);
        let r = roots(&from_roots(&[0.3, 0.3], 1.0));
        assert!(r.is_empty() || (r.len() == 2 && near(&r, 0.3)), "{r:?}");
        let r = roots(&from_roots(&[0.3, 0.3, 0.7], 1.0));
        let (double, simple) = r.split_at(r.len() - 1);
        assert!(
            double.is_empty() || (double.len() == 2 && near(double, 0.3)),
            "{r:?}"
        );
        assert!((simple[0] - 0.7).abs() < 1e-9, "{r:?}");
    }

    #[test]
    fn a_root_at_a_split_is_found_once() {
        let c = from_roots(&[0.5], 1.0);
        assert_eq!(roots(&c), vec![0.5]);
        let c = from_roots(&[0.25, 0.5, 0.75], 1.0);
        let r = roots(&c);
        assert_eq!(r.len(), 3, "{r:?}");
    }
}
