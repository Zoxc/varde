//! Which ties the copies of an offset keep.
//!
//! **Ties.** Each copy of a line is [`Constraint::Parallel`] to it, and a
//! copy of an arc or a circle made about the same centre point; a round
//! join is made about the corner. How far each copy is from its original
//! is an offset pair ([`Sketch::offset_pair`]): the first copy's is held
//! by a driving [`Measure::Offset`] of the distance, and every other's
//! [`Constraint::EqualOffset`] to it, so the whole copy follows the one
//! dimension. Copies meeting at a point share it; where they're tangent
//! there they're held [`Constraint::Tangent`], which takes the place of one
//! of their own ties (a line's offset, then its parallel, an arc's
//! offset), as the point where two tangent curves meet is no place the
//! solver can tell along them: which tie each tangent takes is a matching
//! ([`untie`]). Round a loop of arcs alone, each tangent to the next, one
//! copy is made about a centre of its own, where its original's is.

use super::*;
use crate::Id;

/// A copy made, what it's a copy of, and which of its ties it keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct OffsetCopy {
    pub(super) id: Id,
    /// Its original: a line, arc or circle, or a round join's corner.
    pub(super) base: Id,
    pub(super) pair: OffsetPair,
    /// Whether it's tied by its offset: the lead's dimension, or equal to
    /// it.
    pub(super) distance: bool,
    /// Whether a line's copy is held parallel to it.
    pub(super) parallel: bool,
}

/// Which copies' ties the tangents between them stand for, see [`untie`].
pub(super) struct Untied {
    /// The copy whose offset the dimension holds.
    pub(super) lead: usize,
    /// The copy each tangent stands for a tie of, if any.
    pub(super) assigned: Vec<Option<usize>>,
    /// The copies of arcs made about centres of their own.
    pub(super) freed: Vec<bool>,
}

/// Which copy's ties each tangent between `copies` (`tangents`, by their
/// indices) stands for, and which copy leads, its offset held by the
/// dimension: a line's copy gives up its offset, then its parallel (the
/// lead its parallel alone), an arc's or a round join's its offset. The
/// first lead, lines' copies before arcs' before round joins', of the
/// first few, that lets every tangent stand for a tie, or failing any, the
/// one letting the most.
///
/// Round a loop of arcs alone, each tangent to the next, about centres
/// that stay where they are, the radii that make each tangent to the next
/// make the last tangent to the first too, so one of those tangents is
/// always left over, and without it the two arcs there meet where their
/// circles touch, which the solver can't place along them. So an arc's
/// copy tangent at both ends is made about a centre of its own there,
/// which the two tangents hold, standing for two more ties. A tangent
/// standing for none still is left out. `None` without copies.
pub(super) fn untie(copies: &[OffsetCopy], tangents: &[[usize; 2]]) -> Option<Untied> {
    /// How many leads are tried at most, each a matching.
    const LEADS: usize = 8;
    let order = [OffsetPair::Lines, OffsetPair::Rounds, OffsetPair::Join];
    let leads = order.iter().flat_map(|&pair| {
        let of = copies.iter().enumerate();
        of.filter(move |(_, copy)| copy.pair == pair)
            .map(|(index, _)| index)
    });
    let mut best: Option<(usize, Vec<Option<usize>>, usize)> = None;
    for lead in leads.take(LEADS) {
        let capacity = |copy: usize| ties(&copies[copy], false, copy == lead);
        let assigned = matching(copies.len(), tangents, capacity);
        let matched = assigned.iter().flatten().count();
        let full = matched == tangents.len();
        if best.as_ref().is_none_or(|(_, _, most)| matched > *most) {
            best = Some((lead, assigned, matched));
        }
        if full {
            break;
        }
    }
    let (lead, mut assigned, _) = best?;
    let mut freed = vec![false; copies.len()];
    let tangent_ends = |copy: usize| tangents.iter().filter(|ends| ends.contains(&copy)).count();
    loop {
        let unmatched = assigned.iter().position(Option::is_none);
        let free = unmatched.and_then(|tangent| {
            tangents[tangent].into_iter().find(|&copy| {
                copy != lead
                    && !freed[copy]
                    && copies[copy].pair == OffsetPair::Rounds
                    && tangent_ends(copy) == 2
            })
        });
        let Some(free) = free else {
            break;
        };
        freed[free] = true;
        let capacity = |copy: usize| ties(&copies[copy], freed[copy], copy == lead);
        assigned = matching(copies.len(), tangents, capacity);
    }
    Some(Untied {
        lead,
        assigned,
        freed,
    })
}

/// How many ties `copy` gives up for [`untie`]: a line's copy two, its
/// offset and its parallel, an arc's made about a centre of its own
/// (`freed`) three, any other one, its offset; the lead one fewer, its
/// offset held by the dimension.
pub(super) fn ties(copy: &OffsetCopy, freed: bool, lead: bool) -> usize {
    let ties = match copy.pair {
        OffsetPair::Lines => 2,
        _ if freed => 3,
        _ => 1,
    };
    ties - usize::from(lead)
}

/// A matching of `tangents`, each between two of `count` copies, to the
/// copies, each taking at most `capacity` of them: as many as can be, each
/// tangent's copy or none. Augmenting paths found breadth first, so no
/// recursion however long the chain.
pub(super) fn matching(
    count: usize,
    tangents: &[[usize; 2]],
    capacity: impl Fn(usize) -> usize,
) -> Vec<Option<usize>> {
    let mut assigned: Vec<Option<usize>> = vec![None; tangents.len()];
    let mut load = vec![0; count];
    let mut held: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (tangent, copies) in tangents.iter().enumerate() {
        // The tangent each copy was reached through, which would move to it.
        let mut came: Vec<Option<usize>> = vec![None; count];
        let mut queue = std::collections::VecDeque::new();
        for &copy in copies {
            if came[copy].is_none() {
                came[copy] = Some(tangent);
                queue.push_back(copy);
            }
        }
        let mut free = None;
        while let Some(copy) = queue.pop_front() {
            if load[copy] < capacity(copy) {
                free = Some(copy);
                break;
            }
            for &other in &held[copy] {
                for next in tangents[other] {
                    if next != copy && came[next].is_none() {
                        came[next] = Some(other);
                        queue.push_back(next);
                    }
                }
            }
        }
        // Moves each tangent along the path back to the new one.
        let mut at = free;
        while let Some(copy) = at {
            let Some(moved) = came[copy] else {
                break;
            };
            let from = assigned[moved].replace(copy);
            load[copy] += 1;
            held[copy].push(moved);
            if let Some(from) = from {
                load[from] -= 1;
                held[from].retain(|&t| t != moved);
            }
            at = from;
        }
    }
    assigned
}
