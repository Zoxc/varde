//! Univariate polynomials with exact rational coefficients, and their
//! real roots: the exact stage of the sign conditions at the roots of a
//! polynomial in stored `f64` data (a column's hit on a ruled ribbon or a
//! tile, a crossing of a trim with a line in UV), at the cost of rational
//! evaluations, never a tolerance.
//!
//! **Polynomials** ([`Poly`]) are in the power basis, lowest coefficient
//! first, without trailing zeros ([`trim`]); the zero polynomial is
//! empty.
//!
//! **Roots** ([`roots`]). The distinct real roots of `p` in a closed
//! interval, in increasing order, each with its multiplicity: those of
//! `p`'s square-free part `s = p / gcd(p, p′)`, isolated by the Bernstein
//! root isolation in its exact form ([`isolate_exact`]: Descartes' rule
//! on `s`'s Bernstein coefficients over an interval, no sign change no
//! root inside, one one root, halving where it doesn't settle, a zero at
//! an end or a split point a root exactly there). A
//! root ([`Root`]) is held either exactly (a rational root the halving
//! met) or by an open interval `(lo, hi)` holding it alone, `s(lo)` and
//! `s(hi)` of opposite signs.
//!
//! **Signs at a root** ([`Root::sign_of`]). The sign of another
//! polynomial `q` at a root `r`: zero exactly when `gcd(s, q)` has its
//! root in `r`'s interval; else the interval is halved until `q`'s
//! Bernstein coefficients over it have one sign, which is `q`'s at `r`.
//! So every comparison at an algebraic point (a coordinate against a
//! line, a height against zero) is exact.

use std::cmp::Ordering;

use malachite::Rational;
use malachite::base::num::arithmetic::traits::Sign as _;
use malachite::base::num::basic::traits::Zero as _;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Floor};

use crate::bezier::roots::{Exact, MAX_ROOT_DEPTH, isolate_exact};
use crate::bezier::{self, Dyadic, Limits};

/// A polynomial, power basis, lowest coefficient first, no trailing
/// zeros (the zero polynomial empty).
pub type Poly = Vec<Rational>;

/// `p` without its trailing zeros.
pub fn trim(mut p: Poly) -> Poly {
    while p.last().is_some_and(|c| *c == 0u32) {
        p.pop();
    }
    p
}

/// The constant `c`.
pub fn constant(c: Rational) -> Poly {
    trim(vec![c])
}

/// `p`'s degree; `None` for the zero polynomial.
pub fn degree(p: &[Rational]) -> Option<usize> {
    p.len().checked_sub(1)
}

/// `a + b`.
pub fn add(a: &[Rational], b: &[Rational]) -> Poly {
    let n = a.len().max(b.len());
    trim(
        (0..n)
            .map(|i| match (a.get(i), b.get(i)) {
                (Some(x), Some(y)) => x + y,
                (Some(x), None) | (None, Some(x)) => x.clone(),
                (None, None) => Rational::ZERO,
            })
            .collect(),
    )
}

/// `a − b`.
pub fn sub(a: &[Rational], b: &[Rational]) -> Poly {
    add(a, &neg(b))
}

/// `−a`.
pub fn neg(a: &[Rational]) -> Poly {
    a.iter().map(|x| -x).collect()
}

/// `k·a`.
pub fn scale(a: &[Rational], k: &Rational) -> Poly {
    trim(a.iter().map(|x| x * k).collect())
}

/// `a·b`.
pub fn mul(a: &[Rational], b: &[Rational]) -> Poly {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![Rational::ZERO; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    trim(out)
}

/// `p′`.
pub fn deriv(p: &[Rational]) -> Poly {
    trim(
        p.iter()
            .enumerate()
            .skip(1)
            .map(|(i, c)| c * Rational::from(i as u64))
            .collect(),
    )
}

/// `p(x)`.
pub fn eval(p: &[Rational], x: &Rational) -> Rational {
    p.iter().rev().fold(Rational::ZERO, |acc, c| acc * x + c)
}

/// The sign of `p(x)`.
pub fn sign_at(p: &[Rational], x: &Rational) -> Ordering {
    eval(p, x).sign()
}

/// `p(a + b·τ)` as a polynomial in `τ`.
pub fn compose_affine(p: &[Rational], a: &Rational, b: &Rational) -> Poly {
    let lin = trim(vec![a.clone(), b.clone()]);
    let mut out: Poly = Vec::new();
    for c in p.iter().rev() {
        out = add(&mul(&out, &lin), &constant(c.clone()));
    }
    out
}

/// The polynomial of Bernstein coefficients `b` over `[0, 1]` (degree
/// `b.len() − 1`) in the power basis.
pub fn from_bernstein(b: &[Rational]) -> Poly {
    let n = match b.len() {
        0 => return Vec::new(),
        n => n - 1,
    };
    // `Σ bᵢ·C(n, i)·τⁱ·(1 − τ)^(n − i)`.
    let mut out: Poly = Vec::new();
    for (i, bi) in b.iter().enumerate() {
        let mut term = constant(bi * Rational::from(binomial(n, i)));
        for _ in 0..i {
            term = mul(&term, &[Rational::ZERO, Rational::from(1u32)]);
        }
        for _ in i..n {
            term = mul(&term, &[Rational::from(1u32), Rational::from(-1i32)]);
        }
        out = add(&out, &term);
    }
    out
}

fn binomial(n: usize, k: usize) -> u64 {
    (0..k).fold(1u64, |c, i| c * (n - i) as u64 / (i as u64 + 1))
}

/// The remainder of `a` divided by `b` (`b` not zero).
pub fn rem(a: &[Rational], b: &[Rational]) -> Poly {
    let db = degree(b).expect("a non-zero divisor");
    let lead = &b[db];
    let mut r = trim(a.to_vec());
    while let Some(dr) = degree(&r) {
        if dr < db {
            break;
        }
        let k = &r[dr] / lead;
        for (i, c) in b.iter().enumerate() {
            let x = &k * c;
            r[dr - db + i] -= x;
        }
        // The leading coefficient is now exactly zero.
        r.pop();
        r = trim(r);
    }
    r
}

/// The quotient of `a` divided by `b` (`b` not zero), exact where `b`
/// divides `a`.
pub fn quot(a: &[Rational], b: &[Rational]) -> Poly {
    let db = degree(b).expect("a non-zero divisor");
    let lead = &b[db];
    let mut r = trim(a.to_vec());
    let Some(da) = degree(&r) else {
        return Vec::new();
    };
    if da < db {
        return Vec::new();
    }
    let mut q = vec![Rational::ZERO; da - db + 1];
    while let Some(dr) = degree(&r) {
        if dr < db {
            break;
        }
        let k = &r[dr] / lead;
        for (i, c) in b.iter().enumerate() {
            let x = &k * c;
            r[dr - db + i] -= x;
        }
        q[dr - db] = k;
        r.pop();
        r = trim(r);
    }
    trim(q)
}

/// The monic greatest common divisor of `a` and `b` (zero only if both
/// are).
pub fn gcd(a: &[Rational], b: &[Rational]) -> Poly {
    let (mut x, mut y) = (trim(a.to_vec()), trim(b.to_vec()));
    while !y.is_empty() {
        let r = rem(&x, &y);
        x = y;
        y = r;
    }
    match x.last() {
        None => x,
        Some(l) => {
            let l = l.clone();
            x.iter().map(|c| c / &l).collect()
        }
    }
}

/// `p`'s square-free part: `p / gcd(p, p′)`, with `p`'s distinct roots,
/// each simple (`p` not zero).
pub fn square_free(p: &[Rational]) -> Poly {
    let g = gcd(p, &deriv(p));
    if degree(&g).unwrap_or(0) == 0 {
        trim(p.to_vec())
    } else {
        quot(p, &g)
    }
}

/// The Bernstein coefficients over `[0, 1]` of degree `n − 1` (`n` at
/// least the power-basis `p`'s length).
pub fn to_bernstein(p: &[Rational], n: usize) -> Vec<Rational> {
    let d = n.saturating_sub(1);
    // `bₖ = Σ_{i ≤ k} C(k, i)/C(d, i)·aᵢ`.
    (0..=d)
        .map(|k| {
            (0..=k).fold(Rational::ZERO, |s, i| match p.get(i) {
                Some(a) => s + a * Rational::from(binomial(k, i)) / Rational::from(binomial(d, i)),
                None => s,
            })
        })
        .collect()
}

/// `p`'s Bernstein coefficients over `[a, b]`, of `p`'s degree.
fn bernstein_on(p: &[Rational], a: &Rational, b: &Rational) -> Vec<Rational> {
    let q = compose_affine(p, a, &(b - a));
    to_bernstein(&q, p.len().max(1))
}

/// One strict sign of all of `c`, if they have one.
fn one_sign(c: &[Rational]) -> Option<Ordering> {
    let s = c.first()?.sign();
    (s != Ordering::Equal && c.iter().all(|x| x.sign() == s)).then_some(s)
}

/// A real root of a square-free polynomial `s`: exactly, or alone in the
/// open interval `(lo, hi)` with `s(lo)·s(hi) < 0` (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    s: Poly,
    lo: Rational,
    hi: Rational,
}

impl Root {
    /// The exact root `x` of `s`.
    fn exact(s: Poly, x: Rational) -> Root {
        Root {
            s,
            lo: x.clone(),
            hi: x,
        }
    }

    /// The root itself, where it is known exactly (a rational root).
    pub fn rational(&self) -> Option<&Rational> {
        (self.lo == self.hi).then_some(&self.lo)
    }

    /// The closed interval known to hold the root.
    pub fn interval(&self) -> [&Rational; 2] {
        [&self.lo, &self.hi]
    }

    /// The interval rounded outwards to `f64`s.
    pub fn enclose(&self) -> [f64; 2] {
        [
            f64::rounding_from(&self.lo, Floor).0,
            f64::rounding_from(&self.hi, Ceiling).0,
        ]
    }

    /// Halves the interval once, keeping the root (exact where the
    /// middle is it).
    fn halve(&mut self) {
        if self.lo == self.hi {
            return;
        }
        let m = (&self.lo + &self.hi) / Rational::from(2u32);
        match sign_at(&self.s, &m) {
            Ordering::Equal => {
                self.lo = m.clone();
                self.hi = m;
            }
            o if o == sign_at(&self.s, &self.lo) => self.lo = m,
            _ => self.hi = m,
        }
    }

    /// Halves the interval until it is at most `width` wide (or exact).
    pub fn narrow(&mut self, width: &Rational) {
        while self.lo != self.hi && &(&self.hi - &self.lo) > width {
            self.halve();
        }
    }

    /// The sign of `q` at the root, exactly (module docs).
    pub fn sign_of(&self, q: &[Rational]) -> Ordering {
        let q = trim(q.to_vec());
        if q.is_empty() {
            return Ordering::Equal;
        }
        if let Some(x) = self.rational() {
            return sign_at(&q, x);
        }
        // `q` and its remainder by `s` agree at every root of `s`: a
        // polynomial of high degree (a sign substituted into a tile's
        // resultant) is read at `s`'s degree.
        let q = if degree(&q) >= degree(&self.s) {
            rem(&q, &self.s)
        } else {
            q
        };
        if q.is_empty() {
            return Ordering::Equal;
        }
        // First `q` over the interval, by its Bernstein coefficients there,
        // halving it a few times: most signs settle so, far from a zero of
        // `q`.
        let mut r = self.clone();
        for _ in 0..FAST_HALVINGS {
            if let Some(x) = r.rational() {
                return sign_at(&q, x);
            }
            if let Some(o) = one_sign(&bernstein_on(&q, &r.lo, &r.hi)) {
                return o;
            }
            r.halve();
        }
        // A common factor with its root here: `q` vanishes at the root.
        let g = gcd(&self.s, &q);
        if degree(&g).unwrap_or(0) > 0 {
            let (a, b) = (sign_at(&g, &r.lo), sign_at(&g, &r.hi));
            if r.rational().is_none() && a != b {
                return Ordering::Equal;
            }
        }
        // Else halve until `q`'s Bernstein coefficients over the interval
        // have one sign (they converge to `q` at the root, not zero).
        loop {
            if let Some(x) = r.rational() {
                return sign_at(&q, x);
            }
            if let Some(o) = one_sign(&bernstein_on(&q, &r.lo, &r.hi)) {
                return o;
            }
            r.halve();
        }
    }

    /// The root against the rational `x`.
    pub fn cmp_rational(&self, x: &Rational) -> Ordering {
        // The sign of `τ − x` at the root.
        self.sign_of(&[-x, Rational::from(1u32)])
    }
}

/// The halvings [`Root::sign_of`] tries enclosing the polynomial before
/// its exact stage.
const FAST_HALVINGS: u32 = 48;

/// An enclosure of `p` over `[lo, hi]` (`lo ≤ hi`): Horner in interval
/// arithmetic, exactly.
pub fn enclose(p: &[Rational], lo: &Rational, hi: &Rational) -> [Rational; 2] {
    let mut acc = [Rational::ZERO, Rational::ZERO];
    for c in p.iter().rev() {
        let prods = [&acc[0] * lo, &acc[0] * hi, &acc[1] * lo, &acc[1] * hi];
        let (mut mn, mut mx) = (prods[0].clone(), prods[0].clone());
        for x in &prods[1..] {
            if *x < mn {
                mn = x.clone();
            }
            if *x > mx {
                mx = x.clone();
            }
        }
        acc = [mn + c, mx + c];
    }
    acc
}

/// The distinct real roots of `p` (not zero, of degree at most
/// [`MAX_DEGREE`](crate::bezier::MAX_DEGREE)) in `[lo, hi]` (`lo ≤ hi`), increasing, each with its
/// multiplicity in `p` (module docs).
pub fn roots(p: &[Rational], lo: &Rational, hi: &Rational) -> Vec<(Root, u32)> {
    let p = trim(p.to_vec());
    assert!(!p.is_empty(), "the zero polynomial has every root");
    if degree(&p) == Some(0) {
        return Vec::new();
    }
    let s = square_free(&p);
    let mut out = Vec::new();
    if lo == hi {
        if sign_at(&s, lo) == Ordering::Equal {
            out.push(Root::exact(s.clone(), lo.clone()));
        }
    } else {
        isolate(&s, lo, hi, true, &mut out);
    }
    out.into_iter()
        .map(|r| {
            // The multiplicity: the order of the first derivative of `p`
            // not vanishing at the root.
            let mut m = 1;
            let mut d = deriv(&p);
            while !d.is_empty() && r.sign_of(&d) == Ordering::Equal {
                m += 1;
                d = deriv(&d);
            }
            (r, m)
        })
        .collect()
}

/// The roots of the square-free `s` in `[a, b]` (`a < b`; its ends only
/// where `ends`), increasing: the Bernstein root isolation's exact form
/// ([`isolate_exact`]) on `s`'s coefficients over `[a, b]`, a cluster it
/// leaves at its depth (roots closer than `2⁻⁶³` of the interval)
/// isolated again on its own interval.
fn isolate(s: &Poly, a: &Rational, b: &Rational, ends: bool, out: &mut Vec<Root>) {
    let c = bernstein_on(s, a, b);
    let bp = bezier::Poly::new(c).expect("a degree at most MAX_DEGREE");
    let limits = Limits {
        max_depth: MAX_ROOT_DEPTH,
        max_splits: MAX_SPLITS,
    };
    let found = isolate_exact(&bp, limits)
        .expect("a square-free polynomial isolates within the splits")
        .found;
    let w = b - a;
    let at = |d: Dyadic| a + &w * d.rational();
    for f in found {
        match f {
            Exact::Point(d) => {
                if ends || (d != Dyadic::ZERO && d != Dyadic::ONE) {
                    out.push(Root::exact(s.clone(), at(d)));
                }
            }
            Exact::Root { lo, hi, .. } => out.push(Root {
                s: s.clone(),
                lo: at(lo),
                hi: at(hi),
            }),
            Exact::Cluster { lo, hi, .. } => isolate(s, &at(lo), &at(hi), false, out),
        }
    }
}

/// The most splits of one exact isolation: far above what a square-free
/// polynomial of degree [`MAX_DEGREE`](crate::bezier::MAX_DEGREE) takes (each depth holds fewer
/// unsettled intervals than its degree).
const MAX_SPLITS: u32 = 1 << 20;

#[cfg(test)]
mod tests;
