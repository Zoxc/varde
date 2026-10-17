//! Exact dyadic numbers `m·2^e` (every f64 is one) and the conversions
//! between them, rationals and integer coefficient vectors.

use malachite::base::num::arithmetic::traits::Lcm;
use malachite::base::num::basic::traits::{One, Zero};
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::num::logic::traits::SignificantBits;
use malachite::base::rounding_modes::RoundingMode;
use malachite::{Integer, Natural, Rational};
use std::cmp::Ordering;

/// The exact value `m·2^e`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Dy {
    pub(super) m: Integer,
    pub(super) e: i64,
}

impl Dy {
    pub(super) const ZERO: Dy = Dy {
        m: <Integer as Zero>::ZERO,
        e: 0,
    };

    /// The exact value of a finite `x`.
    pub(super) fn from_f64(x: f64) -> Dy {
        assert!(
            x.is_finite(),
            "the counting oracle takes finite inputs, not {x}"
        );
        if x == 0.0 {
            return Dy::ZERO;
        }
        let bits = x.to_bits();
        let biased = ((bits >> 52) & 0x7ff) as i64;
        let fraction = bits & ((1u64 << 52) - 1);
        let (mantissa, e) = if biased == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1u64 << 52), biased - 1075)
        };
        let m = Integer::from(mantissa);
        Dy {
            m: if bits >> 63 == 1 { -m } else { m },
            e,
        }
    }

    pub(super) fn from_int(i: i64) -> Dy {
        Dy {
            m: Integer::from(i),
            e: 0,
        }
    }

    pub(super) fn is_zero(&self) -> bool {
        self.m == 0
    }

    /// `⌊log2 |self|⌋`, for a non-zero value.
    pub(super) fn log2(&self) -> i64 {
        self.e + (&self.m).significant_bits() as i64 - 1
    }

    pub(super) fn add(&self, o: &Dy) -> Dy {
        if self.is_zero() {
            return o.clone();
        }
        if o.is_zero() {
            return self.clone();
        }
        let e = if self.e < o.e { self.e } else { o.e };
        Dy {
            m: (&self.m << (self.e - e) as u64) + (&o.m << (o.e - e) as u64),
            e,
        }
    }

    pub(super) fn sub(&self, o: &Dy) -> Dy {
        self.add(&o.neg())
    }

    pub(super) fn mul(&self, o: &Dy) -> Dy {
        if self.is_zero() || o.is_zero() {
            return Dy::ZERO;
        }
        Dy {
            m: &self.m * &o.m,
            e: self.e + o.e,
        }
    }

    pub(super) fn neg(&self) -> Dy {
        Dy {
            m: -&self.m,
            e: self.e,
        }
    }

    /// `self·2^k`.
    pub(super) fn shl(&self, k: i64) -> Dy {
        Dy {
            m: self.m.clone(),
            e: self.e + k,
        }
    }

    /// The nearest f64, saturating far outside its range (bounding boxes
    /// only).
    pub(super) fn approx(&self) -> f64 {
        if self.is_zero() {
            return 0.0;
        }
        let bits = (&self.m).significant_bits() as i64;
        let (m, e) = if bits > 62 {
            (&self.m >> (bits - 62) as u64, self.e + bits - 62)
        } else {
            (self.m.clone(), self.e)
        };
        let f = f64::rounding_from(&m, RoundingMode::Nearest).0;
        libm::scalbn(f, e.clamp(-2000, 2000) as i32)
    }
}

pub(super) fn ord_sign(o: Ordering) -> i64 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// The values as integers over one common power of two: the same
/// values times one positive factor.
pub(super) fn common(v: &[Dy]) -> Vec<Integer> {
    let e = v
        .iter()
        .filter(|x| !x.is_zero())
        .map(|x| x.e)
        .min()
        .unwrap_or(0);
    v.iter()
        .map(|x| {
            if x.is_zero() {
                Integer::ZERO
            } else {
                &x.m << (x.e - e) as u64
            }
        })
        .collect()
}

/// The rationals as integers over their common denominator: the same
/// values times one positive factor.
pub(super) fn integerize(v: &[Rational]) -> Vec<Integer> {
    let mut l = Natural::ONE;
    for x in v {
        l = l.lcm(x.to_denominator());
    }
    v.iter()
        .map(|x| {
            let n = Integer::from(x.to_numerator()) * Integer::from(&l / x.to_denominator());
            if *x < 0 { -n } else { n }
        })
        .collect()
}

pub(super) fn q(x: f64) -> Rational {
    Rational::try_from(x).expect("the counting oracle takes finite inputs")
}

pub(super) fn rational_approx(x: &Rational) -> f64 {
    f64::rounding_from(x, RoundingMode::Nearest).0
}

/// The three affine coordinates, exactly.
pub(super) fn dy3(p: [f64; 3]) -> [Dy; 3] {
    p.map(Dy::from_f64)
}

pub(super) fn dot(a: &[Dy; 3], b: &[Dy; 3]) -> Dy {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

pub(super) fn cross(a: &[Dy; 3], b: &[Dy; 3]) -> [Dy; 3] {
    [
        a[1].mul(&b[2]).sub(&a[2].mul(&b[1])),
        a[2].mul(&b[0]).sub(&a[0].mul(&b[2])),
        a[0].mul(&b[1]).sub(&a[1].mul(&b[0])),
    ]
}
