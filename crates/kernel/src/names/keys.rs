//! Face keys, the digests by which keys refer to other names, the term
//! table that keeps what each digest stands for, and the tags vertices
//! and edges carry.
//!
//! A [`FaceKey`] is made from what the caller gave an operation: its
//! [`OpCtx`] (feature and copy) and the part of the feature a face is
//! ([`FacePart`]), never an id or a count of the kernel's. All kernel
//! faces of one constructed part share one key (a cylinder's two halves,
//! a torus's quarters).
//!
//! **Digests.** A part that refers to other names (a blend corner to its
//! edges' names, an offset wall to its source face's key) holds their
//! 128-bit [`Digest`], so every key has one size and compares exactly.
//! A digest is a fixed mix of the name's words ([`Term::digest`]): it is
//! kept with the body's names and must never change, so the tests pin
//! its values. A caller's own 64-bit name (a blend chain's) becomes a
//! digest by zero-extension ([`Digest::from_u64`]). The body's
//! [`TermTable`] maps each digest it made back to its [`Term`], for
//! showing a name and for references' qualifiers.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;

use crate::infra::ops::{FeatureId, InstanceId, OpCtx};

use super::derive::Between;

/// A 128-bit digest of a name, by which a key refers to other names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest(u128);

impl Digest {
    /// A caller's 64-bit name as a digest, zero-extended: a name the
    /// caller made (a blend chain's), standing for itself, with no term.
    pub const fn from_u64(name: u64) -> Digest {
        Digest(name as u128)
    }

    /// The digest with these bits, as kept with a body's names.
    pub const fn from_bits(bits: u128) -> Digest {
        Digest(bits)
    }

    /// The digest's bits.
    pub const fn to_bits(self) -> u128 {
        self.0
    }
}

/// The odd constant splitmix64 steps by: `2⁶⁴/φ`.
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// The second lane's step, another odd constant.
const GAMMA_B: u64 = 0xd1b5_4a32_d192_ed03;

/// splitmix64's finalizer: a bijection of `u64` mixing every bit into
/// every other.
fn finalize(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The fixed mix of a sequence of words into a [`Digest`]. Two lanes,
/// `a` and `b`, start at 0; each word `w` steps them as
/// `a = f((a + γ) ^ w)`, `b = f((b + γ_b) ^ w ^ rotl(a, 32))`, with `f`
/// splitmix64's finalizer (wrapping arithmetic); the word count is then
/// pushed as a last word, and the digest is `hi = f(a ^ rotl(b, 29))`,
/// `lo = f(b ^ hi)`. Each step and the end are bijections of the 128-bit
/// state. Names are kept with bodies, so it never changes: the tests pin
/// its values.
#[derive(Clone, Copy, Debug)]
struct Mix {
    a: u64,
    b: u64,
    n: u64,
}

impl Mix {
    fn new() -> Mix {
        Mix { a: 0, b: 0, n: 0 }
    }

    fn step(&mut self, w: u64) {
        self.a = finalize(self.a.wrapping_add(GAMMA) ^ w);
        self.b = finalize(self.b.wrapping_add(GAMMA_B) ^ w ^ self.a.rotate_left(32));
    }

    fn push(&mut self, w: u64) {
        self.step(w);
        self.n = self.n.wrapping_add(1);
    }

    fn push_digest(&mut self, d: Digest) {
        self.push((d.0 >> 64) as u64);
        self.push(d.0 as u64);
    }

    fn finish(mut self) -> Digest {
        let n = self.n;
        self.step(n);
        let hi = finalize(self.a ^ self.b.rotate_left(29));
        let lo = finalize(self.b ^ hi);
        Digest((u128::from(hi) << 64) | u128::from(lo))
    }
}

/// The digest of a text written into it ([`fmt::Write`]): a tag word,
/// then the text's bytes in 8-byte little-endian words (the last padded
/// with zeros), then its length in bytes, mixed as a name's words are.
/// The tag keeps a text's digest apart from any name's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextDigest {
    mix: Mix,
    word: u64,
    len: u64,
}

/// The tag word a [`TextDigest`] starts with: "text" in ASCII.
const TEXT_TAG: u64 = 0x7465_7874;

impl TextDigest {
    pub(crate) fn new() -> TextDigest {
        let mut mix = Mix::new();
        mix.push(TEXT_TAG);
        TextDigest {
            mix,
            word: 0,
            len: 0,
        }
    }

    pub(crate) fn finish(mut self) -> Digest {
        if self.len % 8 != 0 {
            self.mix.push(self.word);
        }
        self.mix.push(self.len);
        self.mix.finish()
    }
}

impl fmt::Write for TextDigest {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &byte in s.as_bytes() {
            self.word |= u64::from(byte) << (8 * (self.len % 8));
            self.len = self.len.wrapping_add(1);
            if self.len % 8 == 0 {
                self.mix.push(self.word);
                self.word = 0;
            }
        }
        Ok(())
    }
}

/// Which pole of a form: towards its key's sign-ruled axis direction
/// (`N`) or away from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PoleEnd {
    /// Towards the axis direction.
    N,
    /// Away from it.
    S,
}

/// A vertex's tag: kept by name, through operations, while the vertex
/// lasts (a pole snapped away keeps it with kind regular).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VertTag {
    /// A sphere's, torus's or revolved form's pole.
    Pole(PoleEnd),
    /// A cone's apex.
    Apex,
}

/// An edge's tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeTag {
    /// A seam: an edge of the same form between faces of one key.
    Seam,
}

/// What part of its feature a face is, from what the feature was given.
/// Its fields are the caller's numbers (sketch curves, regions, segment
/// and span counts along them) and digests of names; no kernel id fits
/// in one.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FacePart {
    /// The cap an extrusion or a turn starts at; `region` the least
    /// sketch region tag of the profile loop it closes, 0 untagged.
    StartCap { region: u32 },
    /// The cap it ends at.
    EndCap { region: u32 },
    /// The wall swept by segment `segment` of profile curve `curve`.
    Side { curve: u64, segment: u32 },
    /// A helical sweep's wall: profile curve `curve`, turn `turn` of the
    /// path.
    Turn { curve: u64, turn: u32 },
    /// A primitive's face, by its number in the primitive.
    Primitive(u32),
    /// A blend's face along the chain named `chain` (the caller's name).
    Blend { chain: Digest },
    /// A blend's corner: the digest of its blended edges' names
    /// ([`TermTable::corner`]), the ordinal `k` among corners with that
    /// digest.
    BlendCorner { edges: Digest, k: u32 },
    /// An offset of the face whose key has digest `of`
    /// ([`TermTable::face`]).
    Offset { of: Digest },
    /// A loft's face through span `span` of section curve `curve`.
    Loft { curve: u64, span: u32 },
    /// A tapered wall behind the neutral plane.
    BackSide { curve: u64, segment: u32 },
}

impl FacePart {
    /// Pushes the part's words: its number (fixed, never reused), then
    /// its fields in order, a digest as two words (high first). The
    /// number fixes how many words follow, so the words of a sequence of
    /// parts read back one way.
    fn push_words(&self, m: &mut Mix) {
        let small = |m: &mut Mix, tag: u64, a: u64, b: u64| {
            m.push(tag);
            m.push(a);
            m.push(b);
        };
        match *self {
            FacePart::StartCap { region } => small(m, 1, u64::from(region), 0),
            FacePart::EndCap { region } => small(m, 2, u64::from(region), 0),
            FacePart::Side { curve, segment } => small(m, 3, curve, u64::from(segment)),
            FacePart::Turn { curve, turn } => small(m, 4, curve, u64::from(turn)),
            FacePart::Primitive(k) => small(m, 5, u64::from(k), 0),
            FacePart::Blend { chain } => {
                m.push(6);
                m.push_digest(chain);
            }
            FacePart::BlendCorner { edges, k } => {
                m.push(7);
                m.push_digest(edges);
                m.push(u64::from(k));
            }
            FacePart::Offset { of } => {
                m.push(8);
                m.push_digest(of);
            }
            FacePart::Loft { curve, span } => small(m, 9, curve, u64::from(span)),
            FacePart::BackSide { curve, segment } => small(m, 10, curve, u64::from(segment)),
        }
    }
}

/// A face's key: the feature, the copy of it and the part, from what the
/// caller gave the operation, never a kernel count or an id. Keys order
/// by the leaf order: feature, then copy, then part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaceKey {
    /// The feature.
    pub feature: FeatureId,
    /// The part of it.
    pub part: FacePart,
    /// The copy of it.
    pub instance: InstanceId,
}

impl FaceKey {
    /// The key of part `part` of the feature and copy `ctx` builds.
    pub fn new(ctx: OpCtx, part: FacePart) -> FaceKey {
        FaceKey {
            feature: ctx.feature,
            part,
            instance: ctx.instance,
        }
    }

    /// The feature and copy the key names.
    pub fn ctx(&self) -> OpCtx {
        OpCtx {
            feature: self.feature,
            instance: self.instance,
        }
    }

    /// The key's digest, as [`Term::Face`] gives it.
    pub fn digest(&self) -> Digest {
        let mut m = Mix::new();
        m.push(TERM_FACE);
        self.push_words(&mut m);
        m.finish()
    }

    /// Pushes the key's words: feature, copy, part.
    fn push_words(&self, m: &mut Mix) {
        m.push(self.feature.0);
        m.push(u64::from(self.instance.0));
        self.part.push_words(m);
    }
}

impl Ord for FaceKey {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.feature, self.instance, self.part).cmp(&(o.feature, o.instance, o.part))
    }
}

impl PartialOrd for FaceKey {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

/// The number [`Term::Face`]'s words start with.
const TERM_FACE: u64 = 1;
/// The number [`Term::Edges`]' words start with.
const TERM_EDGES: u64 = 2;

/// What a digest stands for: a name nested in a key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Term {
    /// A face's key (an offset wall's source face).
    Face(FaceKey),
    /// The names of edges, sorted, repeats kept (a blend corner's blended
    /// edges).
    Edges(Vec<Between>),
}

impl Term {
    /// The term's digest: the [`Mix`] of its number, then for a face its
    /// key's words (feature, copy, part number and fields), for edges
    /// their count and each name's two keys' words, lesser key first.
    pub fn digest(&self) -> Digest {
        match self {
            Term::Face(k) => k.digest(),
            Term::Edges(edges) => {
                let mut m = Mix::new();
                m.push(TERM_EDGES);
                m.push(edges.len() as u64);
                for e in edges {
                    let [lo, hi] = e.keys();
                    lo.push_words(&mut m);
                    hi.push_words(&mut m);
                }
                m.finish()
            }
        }
    }
}

/// Two different terms with one digest: never seen, 128 bits making it
/// as unlikely as any hardware fault, but refused rather than let one
/// name stand for another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DigestCollision {
    /// The digest.
    pub digest: Digest,
    /// The term the table holds for it.
    pub held: Term,
    /// The other term.
    pub other: Term,
}

impl std::fmt::Display for DigestCollision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "two names share the digest {:032x}: {:?} and {:?}",
            self.digest.0, self.held, self.other
        )
    }
}

impl std::error::Error for DigestCollision {}

/// A body's term table: what each digest its keys hold stands for, in
/// digest order. Kept with the body's names; a caller's own names
/// ([`Digest::from_u64`]) have no entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TermTable {
    terms: BTreeMap<Digest, Term>,
}

impl TermTable {
    /// Adds `term` if not held, and gives its digest; refused if another
    /// term has the digest (the table unchanged).
    pub fn intern(&mut self, term: Term) -> Result<Digest, DigestCollision> {
        let d = term.digest();
        match self.terms.entry(d) {
            Entry::Vacant(v) => {
                v.insert(term);
                Ok(d)
            }
            Entry::Occupied(o) if *o.get() == term => Ok(d),
            Entry::Occupied(o) => Err(DigestCollision {
                digest: d,
                held: o.get().clone(),
                other: term,
            }),
        }
    }

    /// The digest of face key `key`, interned: an offset wall's `of`.
    pub fn face(&mut self, key: FaceKey) -> Result<Digest, DigestCollision> {
        self.intern(Term::Face(key))
    }

    /// The digest of the names `edges` sorted, interned: a blend corner's
    /// `edges`. The order given doesn't matter; repeats count.
    pub fn corner(
        &mut self,
        edges: impl IntoIterator<Item = Between>,
    ) -> Result<Digest, DigestCollision> {
        let mut edges: Vec<Between> = edges.into_iter().collect();
        edges.sort_unstable();
        self.intern(Term::Edges(edges))
    }

    /// Adds every term of `other` this table doesn't hold; refused,
    /// adding none, if one shares its digest with a different term here.
    pub fn merge(&mut self, other: &TermTable) -> Result<(), DigestCollision> {
        for (d, t) in &other.terms {
            if let Some(held) = self.terms.get(d)
                && held != t
            {
                return Err(DigestCollision {
                    digest: *d,
                    held: held.clone(),
                    other: t.clone(),
                });
            }
        }
        for (d, t) in &other.terms {
            self.terms.entry(*d).or_insert_with(|| t.clone());
        }
        Ok(())
    }

    /// The term `d` stands for, if the table holds it.
    pub fn get(&self, d: Digest) -> Option<&Term> {
        self.terms.get(&d)
    }

    /// The terms held, in digest order.
    pub fn iter(&self) -> impl Iterator<Item = (Digest, &Term)> + '_ {
        self.terms.iter().map(|(d, t)| (*d, t))
    }

    /// How many terms the table holds.
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

#[cfg(test)]
mod tests;
