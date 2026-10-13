//! The chain a curve is in, and a chain as an offset runs it.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec2;

use super::*;
use crate::intersect::Geom;
use crate::{Curve, EditError, Id, Side, Sketch};

/// The curves ending at each point, of `curves`, each an id and its ends
/// if it has them.
pub(super) fn ending(
    curves: impl IntoIterator<Item = (Id, Option<[Id; 2]>)>,
) -> BTreeMap<Id, Vec<Id>> {
    let mut ending: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for (id, ends) in curves {
        for point in ends.into_iter().flatten() {
            ending.entry(point).or_default().push(id);
        }
    }
    ending
}

/// A chain of curves joined end to end, as offsetting takes it, see
/// [`Sketch::chained`].
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Chain {
    pub(super) links: Vec<Link>,
    /// Whether the last ends where the first starts, or it's a circle.
    pub(super) closed: bool,
}

impl Chain {
    /// The point where the piece `before` meets the one after, and where
    /// it is.
    pub(super) fn corner(&self, sketch: &Sketch, before: usize) -> Option<(Id, DVec2)> {
        let link = &self.links[before];
        let [start, end] = sketch.curve(link.id)?.curve.ends()?;
        let point = if link.forward { end } else { start };
        Some((point, sketch.point(point)?.at))
    }

    /// How far `point` is from the nearest of its pieces, and which side of
    /// the chain it's on there, the left of the way it runs
    /// [`Side::Positive`]: of the piece, or where it's nearest a corner
    /// (within `tol`), where two pieces meet, the side the corner points
    /// to (away from the way it turns), the only side a place can be
    /// nearest a corner from; past a corner turning back on itself, either
    /// (`None`). `None` without pieces.
    pub(super) fn nearest(&self, point: DVec2, tol: f64) -> Option<(f64, Option<Side>)> {
        let (index, u, found) = self
            .links
            .iter()
            .enumerate()
            .map(|(index, link)| {
                let (u, found) = link.geom.closest_with_distance(point);
                (index, u, found)
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))?;
        let count = self.links.len();
        let link = &self.links[index];
        let (into, out) = link.params();
        // The corner it's nearest, if it's nearest an end joined to
        // another piece: the pieces into it and out of it.
        let joined = |at: f64| link.geom.length(u, at) <= tol;
        let corner = if joined(out) && (index + 1 < count || self.closed) && count > 1 {
            Some((index, (index + 1) % count))
        } else if joined(into) && (index > 0 || self.closed) && count > 1 {
            Some(((index + count - 1) % count, index))
        } else {
            None
        };
        if let Some((before, after)) = corner {
            let way_in = self.links[before].heading_out();
            let way_out = self.links[after].heading_in();
            let turn = way_in.perp_dot(way_out);
            if turn.abs() > TANGENT {
                return Some((found, Some(Side::of(-turn))));
            }
            if way_in.dot(way_out) < 0.0 {
                return Some((found, None));
            }
        }
        let (heading, _) = link.geom.heading(u, link.forward);
        let side = Side::of(heading.perp_dot(point - link.geom.at(u)));
        Some((found, Some(side)))
    }
}

/// A curve of a chain, as the chain runs it: a line, a circle or an arc.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Link {
    pub(super) id: Id,
    /// Whether the chain runs it from its start to its end (an arc
    /// counter-clockwise, as a circle always is).
    pub(super) forward: bool,
    pub(super) geom: Geom,
}

/// A curve run one way by a chain, [`Link`] or [`Raw`]: forward is with
/// its parameter, an arc counter-clockwise.
pub(super) trait Run {
    fn geom(&self) -> &Geom;

    fn forward(&self) -> bool;

    /// Its parameters where the chain comes in and goes out.
    fn params(&self) -> (f64, f64) {
        let last = self.geom().last();
        if self.forward() {
            (0.0, last)
        } else {
            (last, 0.0)
        }
    }

    /// Where the chain comes in.
    fn start(&self) -> DVec2 {
        self.geom().at(self.params().0)
    }

    /// Where the chain goes out.
    fn end(&self) -> DVec2 {
        self.geom().at(self.params().1)
    }

    /// Which way it runs where the chain comes in.
    fn heading_in(&self) -> DVec2 {
        self.geom().heading(self.params().0, self.forward()).0
    }

    /// Which way it runs where the chain goes out.
    fn heading_out(&self) -> DVec2 {
        self.geom().heading(self.params().1, self.forward()).0
    }
}

impl Run for Link {
    fn geom(&self) -> &Geom {
        &self.geom
    }

    fn forward(&self) -> bool {
        self.forward
    }
}

impl Link {
    /// Its copy `shift` to the left of the way it runs (right, below
    /// zero): a line along itself, run the same way; an arc or a circle
    /// about its centre. `None` where an arc's or a circle's copy would
    /// have no radius left, and for a spline, which isn't copied so.
    pub(super) fn raw(&self, shift: f64) -> Option<Geom> {
        match self.geom {
            Geom::Spline(_) => None,
            Geom::Segment { .. } => {
                let (from, to) = self.params();
                let (from, to) = (self.geom.at(from), self.geom.at(to));
                let left = (to - from).normalize().perp();
                Some(Geom::Segment {
                    start: from + left * shift,
                    end: to + left * shift,
                })
            }
            Geom::Round {
                center,
                radius,
                begin,
                sweep,
            } => {
                // Left of counter-clockwise is inwards.
                let radius = if self.forward {
                    radius - shift
                } else {
                    radius + shift
                };
                (radius > 0.0 && radius.is_finite()).then_some(Geom::Round {
                    center,
                    radius,
                    begin,
                    sweep,
                })
            }
        }
    }

    /// The edges of the band `distance` round it: its copies either side,
    /// and circles round its ends. A copy with no radius left, within
    /// `tol` of its centre, is left out.
    pub(super) fn band(&self, distance: f64, tol: f64) -> Vec<Geom> {
        let cap = |center| Geom::circle(center, distance);
        match self.geom {
            Geom::Spline(_) => Vec::new(),
            Geom::Segment { start, end } => {
                let left = (end - start).normalize().perp() * distance;
                vec![
                    Geom::Segment {
                        start: start + left,
                        end: end + left,
                    },
                    Geom::Segment {
                        start: start - left,
                        end: end - left,
                    },
                    cap(start),
                    cap(end),
                ]
            }
            Geom::Round {
                center,
                radius,
                begin,
                sweep,
            } => {
                let round = |radius| Geom::Round {
                    center,
                    radius,
                    begin,
                    sweep,
                };
                let mut edges = vec![round(radius + distance)];
                if radius - distance > tol {
                    edges.push(round(radius - distance));
                }
                if self.geom.has_ends() {
                    edges.extend([cap(self.geom.at(0.0)), cap(self.geom.at(sweep))]);
                }
                edges
            }
        }
    }
}

impl Sketch {
    /// The chain the curve `curve` is in: the curves joined to it end to
    /// end, on through each point where two of them end and nothing else
    /// does, `curve` first and then the rest in no order that matters. A
    /// circle or a closed spline alone. Empty if `curve` is none of the
    /// sketch's.
    pub fn chain_of(&self, curve: Id) -> Vec<Id> {
        let Some(entry) = self.curve(curve) else {
            return Vec::new();
        };
        if let Curve::Circle { .. } = entry.curve {
            return vec![curve];
        }
        let ending = ending(
            self.curves
                .iter()
                .map(|entry| (entry.id, entry.curve.ends())),
        );
        let mut chain = vec![curve];
        let mut seen = BTreeSet::from([curve]);
        let mut next = 0;
        while let Some(&id) = chain.get(next) {
            next += 1;
            let Some(ends) = self.curve(id).and_then(|entry| entry.curve.ends()) else {
                continue;
            };
            for point in ends {
                if let Some(&[a, b]) = ending.get(&point).map(Vec::as_slice) {
                    let other = if a == id { b } else { a };
                    if seen.insert(other) {
                        chain.push(other);
                    }
                }
            }
        }
        chain
    }

    /// Whether the curves `ids` are a chain an offset can copy: lines and
    /// arcs joined end to end, no point the end of more than two, or a
    /// circle or a spline alone (see
    /// [`SketchEdit::Offset`](crate::SketchEdit::Offset)).
    pub fn is_chain(&self, ids: &[Id]) -> bool {
        self.lone_spline(ids).is_some() || self.chained(ids).is_ok()
    }

    /// The curves `ids` as a chain, run so that the first goes forward
    /// (from its start, an arc counter-clockwise): each line or arc joined
    /// to the next end to end, a loop if the last ends where the first
    /// starts, or a circle alone. `Target` for an id that's no line, arc
    /// or circle of the sketch's, or one with no size; `NotAChain` if they
    /// don't join up so, a point is the end of more than two, or one is a
    /// spline, which is offset alone (see `offset/spline.rs`).
    pub(super) fn chained(&self, ids: &[Id]) -> Result<Chain, EditError> {
        let mut seen = BTreeSet::new();
        let ids: Vec<Id> = ids.iter().copied().filter(|&id| seen.insert(id)).collect();
        let &first = ids.first().ok_or(EditError::NotAChain)?;
        let mut shapes = BTreeMap::new();
        for &id in &ids {
            let entry = self.curve(id).ok_or(EditError::Target(id))?;
            let geom = Geom::of(self, &entry.curve).ok_or(EditError::Target(id))?;
            if let Geom::Spline(_) = geom {
                return Err(EditError::NotAChain);
            }
            shapes.insert(id, (entry.curve.ends(), geom));
        }
        let link = |id: Id, forward: bool| Link {
            id,
            forward,
            geom: shapes[&id].1.clone(),
        };
        let ends_of = |id: Id| shapes[&id].0;
        if ends_of(first).is_none() {
            return if ids.len() == 1 {
                Ok(Chain {
                    links: vec![link(first, true)],
                    closed: true,
                })
            } else {
                Err(EditError::NotAChain)
            };
        }
        if ids.iter().any(|&id| ends_of(id).is_none()) {
            return Err(EditError::NotAChain);
        }
        let ending = ending(ids.iter().map(|&id| (id, ends_of(id))));
        if ending.values().any(|curves| curves.len() > 2) {
            return Err(EditError::NotAChain);
        }
        // The curve other than `from` ending at `point`, and whether it
        // runs away from there.
        let beyond = |from: Id, point: Id| {
            let curves = ending.get(&point)?;
            let &next = curves.iter().find(|&&id| id != from)?;
            let [start, _] = ends_of(next)?;
            Some((next, start == point))
        };
        let [first_start, first_end] = ends_of(first).ok_or(EditError::NotAChain)?;
        // The links met walking on from `first` through `at`, each run the
        // way of the walk if `onward`, else against it, and whether the
        // walk came back to `first`.
        let walk = |mut at: Id, onward: bool| {
            let (mut links, mut from) = (Vec::new(), first);
            while let Some((next, away)) = beyond(from, at) {
                if next == first {
                    return Ok((links, true));
                }
                // Joined up otherwise, it's been round already: can't be,
                // as no point ends more than two.
                if links.len() > ids.len() {
                    return Err(EditError::NotAChain);
                }
                links.push(link(next, away == onward));
                let [start, end] = ends_of(next).ok_or(EditError::NotAChain)?;
                at = if away { end } else { start };
                from = next;
            }
            Ok((links, false))
        };
        let (ahead, closed) = walk(first_end, true)?;
        // Behind, run towards `first`.
        let behind = if closed {
            Vec::new()
        } else {
            walk(first_start, false)?.0
        };
        let links: Vec<Link> = behind
            .into_iter()
            .rev()
            .chain([link(first, true)])
            .chain(ahead)
            .collect();
        if links.len() != ids.len() {
            return Err(EditError::NotAChain);
        }
        Ok(Chain { links, closed })
    }
}
