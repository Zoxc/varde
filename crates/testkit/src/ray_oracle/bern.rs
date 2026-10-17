//! Polynomials in tensor-product Bernstein form over the unit box, in up
//! to three variables, with integer coefficients known up to a positive
//! factor (only signs, and ratios within one polynomial, matter), and
//! their f64 approximations for guidance.

use malachite::Integer;
use malachite::base::num::arithmetic::traits::Sign;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::num::logic::traits::SignificantBits;
use malachite::base::rounding_modes::RoundingMode;
use std::cmp::Ordering;

/// Coefficients in row-major order of the degrees `n` (an unused
/// variable has degree 0).
#[derive(Clone, Debug)]
pub(super) struct Bern {
    pub(super) n: [usize; 3],
    pub(super) c: Vec<Integer>,
}

/// Which signs the coefficients take.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Signs {
    pub(super) pos: bool,
    pub(super) neg: bool,
    pub(super) zero: bool,
}

impl Signs {
    /// All coefficients positive: the polynomial is on the whole box.
    pub(super) fn positive(self) -> bool {
        !self.neg && !self.zero
    }

    pub(super) fn negative(self) -> bool {
        !self.pos && !self.zero
    }

    pub(super) fn strict(self) -> bool {
        self.positive() || self.negative()
    }
}

pub(super) fn strides(n: [usize; 3]) -> [usize; 3] {
    [(n[1] + 1) * (n[2] + 1), n[2] + 1, 1]
}

pub(super) fn len(n: [usize; 3]) -> usize {
    (n[0] + 1) * (n[1] + 1) * (n[2] + 1)
}

/// The indices of the coefficients whose index along `axis` is `at`.
pub(super) fn slice(n: [usize; 3], axis: usize, at: usize) -> Vec<usize> {
    let st = strides(n);
    let mut out = Vec::new();
    for i0 in 0..=n[0] {
        for i1 in 0..=n[1] {
            for i2 in 0..=n[2] {
                let i = [i0, i1, i2];
                if i[axis] == at {
                    out.push(i0 * st[0] + i1 * st[1] + i2 * st[2]);
                }
            }
        }
    }
    out
}

/// A split point `m/2^p` of the unit interval, `0 < m < 2^p`, `p ≤ 62`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cut {
    pub(super) m: u64,
    pub(super) p: u64,
}

impl Cut {
    pub(super) const HALF: Cut = Cut { m: 1, p: 1 };
}

impl Bern {
    pub(super) fn new(n: [usize; 3], c: Vec<Integer>) -> Bern {
        assert_eq!(c.len(), len(n), "Bernstein coefficient count");
        Bern { n, c }
    }

    pub(super) fn signs(&self) -> Signs {
        let mut s = Signs::default();
        for x in &self.c {
            match x.sign() {
                Ordering::Less => s.neg = true,
                Ordering::Equal => s.zero = true,
                Ordering::Greater => s.pos = true,
            }
        }
        s
    }

    pub(super) fn is_zero(&self) -> bool {
        self.c.iter().all(|x| *x == 0)
    }

    /// The two parts at `r = m/2^p` (`0 < m < 2^p`) along `axis`, each
    /// up to a positive factor.
    pub(super) fn split(&self, axis: usize, r: Cut) -> (Bern, Bern) {
        let (mut l, mut r) = self.split_scaled(axis, r);
        l.normalize();
        r.normalize();
        (l, r)
    }

    /// As [`Bern::split`], both parts times `2^(p·n)` exactly, so forms
    /// split alike keep their common factor.
    pub(super) fn split_scaled(&self, axis: usize, r: Cut) -> (Bern, Bern) {
        let (a, b, p) = (Integer::from((1u64 << r.p) - r.m), Integer::from(r.m), r.p);
        let n = self.n[axis];
        if n == 0 {
            return (self.clone(), self.clone());
        }
        let small = |x: &Integer, k: &Integer, raw: u64| -> Integer {
            match raw {
                1 => x.clone(),
                2 => x << 1u64,
                3 => (x << 1u64) + x,
                _ => x * k,
            }
        };
        let (ra, rb) = ((1u64 << r.p) - r.m, r.m);
        let s = strides(self.n)[axis];
        let mut left = self.c.clone();
        let mut right = self.c.clone();
        let mut v: Vec<Integer> = Vec::with_capacity(n + 1);
        for base in slice(self.n, axis, 0) {
            v.clear();
            v.extend((0..=n).map(|i| self.c[base + i * s].clone()));
            left[base] = &v[0] << (p * n as u64);
            right[base + n * s] = &v[n] << (p * n as u64);
            for j in 1..=n {
                for i in 0..=n - j {
                    v[i] = small(&v[i], &a, ra) + small(&v[i + 1], &b, rb);
                }
                let shift = p * (n - j) as u64;
                left[base + j * s] = &v[0] << shift;
                right[base + (n - j) * s] = &v[n - j] << shift;
            }
        }
        (
            Bern { n: self.n, c: left },
            Bern {
                n: self.n,
                c: right,
            },
        )
    }

    /// Divides out the common power of two.
    pub(super) fn normalize(&mut self) {
        normalize_all(&mut [self]);
    }

    /// The coefficients times `2^-s` as f64 (the largest near `2^60`),
    /// and `s`.
    pub(super) fn approx(&self) -> (Vec<f64>, i64) {
        let bits = self
            .c
            .iter()
            .map(|x| x.significant_bits())
            .max()
            .unwrap_or(0) as i64;
        let s = bits - 60;
        let c = self
            .c
            .iter()
            .map(|x| {
                let y = if s > 0 {
                    x >> s as u64
                } else {
                    x << (-s) as u64
                };
                f64::rounding_from(&y, RoundingMode::Nearest).0
            })
            .collect();
        (c, s)
    }

    /// A polynomial in the same variables of zero degree along the axes
    /// it doesn't use: `c` given along `axis` only.
    pub(super) fn along(axis: usize, c: Vec<Integer>) -> Bern {
        let mut n = [0; 3];
        n[axis] = c.len() - 1;
        Bern::new(n, c)
    }
}

/// Divides out the power of two common to all the forms' coefficients
/// (keeping the forms' ratios).
pub(super) fn normalize_all(forms: &mut [&mut Bern]) {
    let z = forms
        .iter()
        .flat_map(|f| f.c.iter())
        .filter_map(|x| x.trailing_zeros())
        .min();
    if let Some(z) = z
        && z > 0
    {
        for f in forms {
            for x in &mut f.c {
                *x >>= z;
            }
        }
    }
}

/// De Casteljau evaluation of a univariate Bernstein form.
fn decasteljau(c: &[f64], x: f64) -> f64 {
    let mut v = c.to_vec();
    let n = v.len();
    for j in 1..n {
        for i in 0..n - j {
            v[i] = v[i] * (1.0 - x) + v[i + 1] * x;
        }
    }
    v[0]
}

/// The value of an approximated tensor form at `x`.
pub(super) fn eval(c: &[f64], n: [usize; 3], x: [f64; 3]) -> f64 {
    let mut cur = c.to_vec();
    for axis in (0..3).rev() {
        let k = n[axis] + 1;
        if k == 1 {
            continue;
        }
        cur = cur.chunks(k).map(|f| decasteljau(f, x[axis])).collect();
    }
    cur[0]
}

/// The derivative along `axis` of an approximated tensor form, as a
/// form of one degree less along it.
pub(super) fn derivative(c: &[f64], n: [usize; 3], axis: usize) -> (Vec<f64>, [usize; 3]) {
    if n[axis] == 0 {
        return (vec![0.0], [0; 3]);
    }
    let mut dn = n;
    dn[axis] -= 1;
    let (st, dst) = (strides(n), strides(dn));
    let mut d = vec![0.0; len(dn)];
    for i0 in 0..=dn[0] {
        for i1 in 0..=dn[1] {
            for i2 in 0..=dn[2] {
                let at = i0 * st[0] + i1 * st[1] + i2 * st[2];
                d[i0 * dst[0] + i1 * dst[1] + i2 * dst[2]] =
                    n[axis] as f64 * (c[at + st[axis]] - c[at]);
            }
        }
    }
    (d, dn)
}
