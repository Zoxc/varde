//! Crossing counts at an edge between rounded faces. Where an edge of one
//! operand crosses an edge of the other exactly, inside it, and a face at
//! that edge is rounded (its plane isn't a coordinate plane, so its tile
//! passes within the rounding misfit of its key's plane and its vertices,
//! not through them; or its p-curves are perturbed), the crossing lies on
//! the faces' misfit-width ribbons: it counts against one face or the
//! other as the ribbons fall, not as `d` says. Its per-face counts are
//! tie-scale there; their sum over the edge's faces is what is checked.

use malachite::Rational;
use malachite::base::num::basic::traits::Zero as _;

use super::solids::Solid;
use crate::geom::form::FormKey;
use crate::topo::brep::{EdgeId, FaceId};

/// Per edge of one operand (a row of `x12` or `x21`), the groups of the
/// other operand's faces whose crossing counts are checked by their sum:
/// the faces at an edge it crosses exactly inside, one of them rounded
/// (groups sharing a face merged). Rows without a group are empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TurnedEdges {
    /// Rows of `A`'s edges, groups of `B`'s faces.
    pub x12: Vec<Vec<Vec<usize>>>,
    /// Rows of `B`'s edges, groups of `A`'s faces.
    pub x21: Vec<Vec<Vec<usize>>>,
}

impl TurnedEdges {
    /// The groups of a placement of `a` against `b`; `exact_ribbons`
    /// says the p-curves are unperturbed (else every face is rounded).
    pub(super) fn of(a: &Solid, b: &Solid, exact_ribbons: bool) -> TurnedEdges {
        TurnedEdges {
            x12: groups(a, b, exact_ribbons),
            x21: groups(b, a, exact_ribbons),
        }
    }

    /// How many groups there are, both tables.
    pub(super) fn count(&self) -> usize {
        self.x12.iter().chain(&self.x21).map(Vec::len).sum()
    }
}

/// The group of `groups` row `e` holding face `f`, if any.
pub(super) fn group_of(groups: &[Vec<Vec<usize>>], e: usize, f: usize) -> Option<&[usize]> {
    groups
        .get(e)?
        .iter()
        .find(|g| g.contains(&f))
        .map(Vec::as_slice)
}

/// `x` with each group's counts summed into its first face and its other
/// faces' set to 0: two tables equal after folding where every count
/// outside the groups and every group's sum agree.
pub(super) fn fold(groups: &[Vec<Vec<usize>>], x: &[Vec<i64>]) -> Vec<Vec<i64>> {
    let mut out = x.to_vec();
    for (row, gs) in out.iter_mut().zip(groups) {
        for g in gs {
            let sum = g.iter().map(|&f| row[f]).sum();
            for &f in g {
                row[f] = 0;
            }
            row[g[0]] = sum;
        }
    }
    out
}

/// Per edge of `rows`, the groups of `cols`'s faces at an edge of `cols`
/// it crosses exactly inside (not at the edge's ends: a vertex is exact
/// in the counting's records), one of the faces rounded. Straight edges
/// only (a curved edge's crossings aren't exact in `f64` data).
fn groups(rows: &Solid, cols: &Solid, exact_ribbons: bool) -> Vec<Vec<Vec<usize>>> {
    let faces_of = edge_faces(cols);
    let seg = |s: &Solid, e: EdgeId| s.brep.ends(e).map(|v| s.brep.pos(v));
    rows.edges()
        .into_iter()
        .map(|g| {
            let mut out: Vec<Vec<usize>> = Vec::new();
            if !rows.straight[g.index()] {
                return out;
            }
            for e in cols.edges() {
                let fs = &faces_of[e.index()];
                if !cols.straight[e.index()]
                    || fs
                        .iter()
                        .all(|&f| !rounded(cols, FaceId(f as u32), exact_ribbons))
                    || !crosses_inside(seg(rows, g), seg(cols, e))
                {
                    continue;
                }
                // Merge with the groups sharing a face.
                let mut merged: Vec<usize> = fs.clone();
                out.retain(|x| {
                    let shares = x.iter().any(|f| merged.contains(f));
                    if shares {
                        merged.extend(x.iter().copied());
                    }
                    !shares
                });
                merged.sort_unstable();
                merged.dedup();
                out.push(merged);
            }
            out.sort();
            out
        })
        .collect()
}

/// Per edge of `s`, the faces using it.
fn edge_faces(s: &Solid) -> Vec<Vec<usize>> {
    let b = &s.brep;
    let mut out = vec![Vec::new(); b.edge_count()];
    for f in s.faces() {
        for l in b.loops(f) {
            for c in b.loop_coedges(l) {
                let x = &mut out[b.edge(c).index()];
                if !x.contains(&f.index()) {
                    x.push(f.index());
                }
            }
        }
    }
    out
}

/// Whether face `f` of `s` is rounded: not a plane parallel to a
/// coordinate plane (whose tile is its key's plane exactly, through its
/// exact vertices), or its p-curves perturbed.
fn rounded(s: &Solid, f: FaceId, exact_ribbons: bool) -> bool {
    let lat = s.brep.face_surface(f).lattice();
    let flat = matches!(lat.key(), FormKey::Plane(_))
        && lat.axes()[2].iter().filter(|&&c| c == 0.0).count() == 2;
    !(flat && exact_ribbons)
}

type R3 = [Rational; 3];

fn r3(p: [f64; 3]) -> R3 {
    p.map(|c| Rational::try_from(c).expect("finite"))
}

fn sub(a: &R3, b: &R3) -> R3 {
    [0, 1, 2].map(|k| &a[k] - &b[k])
}

fn cross(a: &R3, b: &R3) -> R3 {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn dot(a: &R3, b: &R3) -> Rational {
    (0..3).map(|k| &a[k] * &b[k]).sum()
}

/// Whether the segment `p` (ends included) meets the segment `q` at a
/// point inside `q` (ends excluded), exactly; parallel segments never do.
pub(super) fn crosses_inside(p: [[f64; 3]; 2], q: [[f64; 3]; 2]) -> bool {
    let [p0, p1] = p.map(r3);
    let [q0, q1] = q.map(r3);
    let (r, s, w) = (sub(&p1, &p0), sub(&q1, &q0), sub(&q0, &p0));
    let n = cross(&r, &s);
    let nn = dot(&n, &n);
    if nn == Rational::ZERO || dot(&w, &n) != Rational::ZERO {
        return false;
    }
    // `p0 + t·r = q0 + u·s`: `t = (w × s)·n / n·n`, `u = (w × r)·n / n·n`.
    let t = dot(&cross(&w, &s), &n);
    let u = dot(&cross(&w, &r), &n);
    t >= Rational::ZERO && t <= nn && u > Rational::ZERO && u < nn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_cross_inside_exactly() {
        let x = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        // Across the middle.
        assert!(crosses_inside([[1.0, -1.0, 0.0], [1.0, 1.0, 0.0]], x));
        // Ending on it.
        assert!(crosses_inside([[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]], x));
        // Through its end: not inside it.
        assert!(!crosses_inside([[0.0, -1.0, 0.0], [0.0, 1.0, 0.0]], x));
        // Short of it, skew to it, parallel to it.
        assert!(!crosses_inside([[1.0, 0.5, 0.0], [1.0, 1.0, 0.0]], x));
        assert!(!crosses_inside([[1.0, -1.0, 1.0], [1.0, 1.0, 0.5]], x));
        assert!(!crosses_inside([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], x));
        // `1/3` rounds: a vertical segment at the rounded `y` passes by
        // the line `y = x/3` it meets exactly at a dyadic `y`.
        let q = [[0.0, 0.0, 0.0], [3.0, 1.0, 0.0]];
        let y = 1.0 / 3.0;
        assert!(!crosses_inside([[1.0, y, -1.0], [1.0, y, 1.0]], q));
        assert!(crosses_inside([[1.5, 0.5, -1.0], [1.5, 0.5, 1.0]], q));
    }

    #[test]
    fn folding_sums_each_group_into_its_first_face() {
        let groups = vec![vec![vec![1, 3]], vec![]];
        let x = vec![vec![1, 2, 3, -1], vec![4, 5, 6, 7]];
        assert_eq!(fold(&groups, &x), vec![vec![1, 1, 3, 0], vec![4, 5, 6, 7]]);
        assert_eq!(group_of(&groups, 0, 3), Some(&[1, 3][..]));
        assert_eq!(group_of(&groups, 0, 2), None);
        assert_eq!(group_of(&groups, 1, 1), None);
    }
}
