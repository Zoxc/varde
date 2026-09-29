//! Several regions as one: [`Profiles::merge`].

use std::cmp::Ordering;
use std::fmt;

use super::{Piece, Profiles, pinches};

/// Why regions couldn't be merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeError {
    /// No regions were given.
    Empty,
    /// There's no region by this index.
    NoRegion(usize),
    /// The pieces left don't join up into loops, or name vertices the
    /// profiles don't have: profiles that weren't found by
    /// [`Sketch::profiles`](crate::Sketch::profiles).
    Open,
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MergeError::Empty => f.write_str("no regions to merge"),
            MergeError::NoRegion(index) => write!(f, "there is no region {index}"),
            MergeError::Open => f.write_str("the regions' pieces don't join up"),
        }
    }
}

impl std::error::Error for MergeError {}

impl Profiles {
    /// The regions `picked` (indices into [`Profiles::regions`], in any
    /// order, repeats counting once) as one: the loops bounding them all
    /// together, each with what they bound on its left, so outer loops run
    /// counter-clockwise and holes clockwise, in no particular nesting.
    ///
    /// It's exact: regions never overlap, and two regions side by side
    /// share the pieces between them, the same edge of the same curve
    /// with the same parameters to the bit, run opposite ways. Those
    /// cancel, and what's left is traced again into loops by the vertices
    /// the pieces share ([`Piece::start`], [`Piece::end`]), from each
    /// piece to the one after it in its own region's loop where that one's
    /// still there. Picked regions that aren't side by side give loops of
    /// their own. Where the loops left pass a vertex twice (regions meeting
    /// at a corner only, or a region's hole touching its outer loop) they
    /// are cut there into loops that don't, which touch, and such a
    /// profile is refused when it's extruded.
    ///
    /// The loops come in the same order for the same regions, starting
    /// from the first piece left of the first region picked.
    pub fn merge(&self, picked: &[usize]) -> Result<Vec<Vec<Piece>>, MergeError> {
        let mut picked = picked.to_vec();
        picked.sort_unstable();
        picked.dedup();
        if picked.is_empty() {
            return Err(MergeError::Empty);
        }
        // Every loop's pieces, each with the next one in its loop.
        let mut pieces: Vec<Piece> = Vec::new();
        let mut next: Vec<usize> = Vec::new();
        for &index in &picked {
            let region = self.regions.get(index).ok_or(MergeError::NoRegion(index))?;
            for found in std::iter::once(&region.outer).chain(&region.holes) {
                let first = pieces.len();
                for k in 0..found.len() {
                    next.push(first + (k + 1) % found.len());
                }
                pieces.extend_from_slice(found);
            }
        }
        let alive = cancel(&pieces);
        trace(&pieces, &next, alive, self.vertices.len())
    }
}

/// Which of `pieces` are left once each piece run one way cancels one of
/// the same edge run the other.
fn cancel(pieces: &[Piece]) -> Vec<bool> {
    // The edge a piece runs along: its curve and its parameters, lowest
    // first, to the bit.
    let edge = |piece: &Piece| {
        let (low, high) = if piece.from.total_cmp(&piece.to).is_le() {
            (piece.from, piece.to)
        } else {
            (piece.to, piece.from)
        };
        (piece.curve, low, high)
    };
    let compare = |a: &usize, b: &usize| {
        let (ca, la, ha) = edge(&pieces[*a]);
        let (cb, lb, hb) = edge(&pieces[*b]);
        ca.cmp(&cb)
            .then(la.total_cmp(&lb))
            .then(ha.total_cmp(&hb))
            .then(a.cmp(b))
    };
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by(compare);
    let mut alive = vec![true; pieces.len()];
    let same = |a: &usize, b: &usize| {
        let (ca, la, ha) = edge(&pieces[*a]);
        let (cb, lb, hb) = edge(&pieces[*b]);
        ca == cb && la.total_cmp(&lb) == Ordering::Equal && ha.total_cmp(&hb) == Ordering::Equal
    };
    for group in order.chunk_by(same) {
        // A piece going round (from 0 to a full turn) runs forwards one
        // way and backwards the other, as any other does.
        let forwards = |&i: &usize| pieces[i].from.total_cmp(&pieces[i].to).is_lt();
        let (ahead, back): (Vec<usize>, Vec<usize>) = group.iter().partition(|i| forwards(i));
        for (&a, &b) in ahead.iter().zip(&back) {
            alive[a] = false;
            alive[b] = false;
        }
    }
    alive
}

/// The `alive` pieces traced into loops that pass no vertex twice, each
/// piece followed by its `next` in its own loop where that's alive and
/// not yet taken, or else by the first alive piece not yet taken that
/// starts where it ends. `vertices` is how many there are.
fn trace(
    pieces: &[Piece],
    next: &[usize],
    alive: Vec<bool>,
    vertices: usize,
) -> Result<Vec<Vec<Piece>>, MergeError> {
    if pieces
        .iter()
        .any(|piece| piece.start >= vertices || piece.end >= vertices)
    {
        return Err(MergeError::Open);
    }
    // The pieces left by the vertex each starts at, in order, and how many
    // of those at the front are taken, as they only ever become.
    let mut leaving: Vec<(Vec<usize>, usize)> = vec![(Vec::new(), 0); vertices];
    for (i, piece) in pieces.iter().enumerate() {
        if alive[i] {
            leaving[piece.start].0.push(i);
        }
    }
    let mut taken = vec![false; pieces.len()];
    let mut passed = vec![usize::MAX; vertices];
    let mut loops = Vec::new();
    for first in 0..pieces.len() {
        if !alive[first] || taken[first] {
            continue;
        }
        let mut walk = Vec::new();
        let mut at = first;
        loop {
            taken[at] = true;
            walk.push(pieces[at]);
            let end = pieces[at].end;
            let follow = next[at];
            at = if alive[follow] && !taken[follow] && pieces[follow].start == end {
                follow
            } else {
                let (out, skip) = &mut leaving[end];
                while out.get(*skip).is_some_and(|&i| taken[i]) {
                    *skip += 1;
                }
                match out.get(*skip) {
                    Some(&i) => i,
                    None => break,
                }
            };
        }
        // Every vertex has as many pieces left arriving as leaving, so the
        // walk stops where it started.
        if walk[walk.len() - 1].end != walk[0].start {
            return Err(MergeError::Open);
        }
        pinches(&walk, |piece| piece.start, &mut passed, &mut loops);
    }
    Ok(loops)
}
