//! Decoding sequences no longer than a bound, refused as soon as they
//! pass it, so a short message can't claim (or hold) more elements than
//! the page should build. Postcard says a sequence's length up front, so
//! most are refused before any element is read.

use std::fmt;
use std::marker::PhantomData;

use serde::Deserializer;
use serde::de::{Deserialize, Error, SeqAccess, Visitor};

/// At most `max` elements, and at most `budget` of what `weigh`
/// gives them, all together.
struct Seq<T, F> {
    max: usize,
    weigh: F,
    budget: usize,
    element: PhantomData<T>,
}

impl<'de, T: Deserialize<'de>, F: Fn(&T) -> usize> Visitor<'de> for Seq<T, F> {
    type Value = Vec<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at most {} elements", self.max)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
        if let Some(len) = seq.size_hint().filter(|&len| len > self.max) {
            return Err(A::Error::invalid_length(len, &self));
        }
        let mut out = Vec::new();
        let mut weight = 0usize;
        while let Some(element) = seq.next_element::<T>()? {
            weight = weight.saturating_add((self.weigh)(&element));
            if out.len() >= self.max || weight > self.budget {
                return Err(A::Error::invalid_length(out.len().saturating_add(1), &self));
            }
            out.push(element);
        }
        Ok(out)
    }
}

/// A sequence of at most `max` elements, and at most `budget` of what
/// `weigh` gives them, all together.
pub(crate) fn seq<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
    max: usize,
    weigh: impl Fn(&T) -> usize,
    budget: usize,
) -> Result<Vec<T>, D::Error> {
    deserializer.deserialize_seq(Seq {
        max,
        weigh,
        budget,
        element: PhantomData,
    })
}

/// At most `max` elements, however large.
pub(crate) fn at_most<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
    max: usize,
) -> Result<Vec<T>, D::Error> {
    seq(d, max, |_| 0, 0)
}
