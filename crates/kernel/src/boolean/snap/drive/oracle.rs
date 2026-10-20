//! The driver's outcome against the exact oracle
//! ([`varde_testkit::rational_oracle`]), on the quantities the oracle
//! gives:
//! - **face pairs**: a pair with a Near (Contact) leaf has its gap along
//!   the leaf's `ℓ` within `b_K` everywhere over the faces' common
//!   projection, co-oriented (opposed); a coplanar pair overlapping in
//!   positive area has a leaf of its orientation's kind; a pair apart
//!   only Empty leaves; a transversal pair a Simple or tangent leaf;
//! - **zone area**: a cluster's region (its rim's boundary on each
//!   snapped face, before cancelling) projected along `ℓ` has the area of
//!   the oracle's cells within `b_K` over the cluster's face pairs, to the
//!   UV grid's rounding (the stores' UVs are rounded to `2⁻⁵²` of a tile);
//! - **coincident edges**: the pairs of a snapped edge and a target edge
//!   whose projections along `ℓ` overlap in a segment of positive length,
//!   each where both faces lie in the zone, are the pairs the zone's
//!   trim boolean bundles (a rewrite edge along the target edge, made from
//!   that snapped edge).
//!
//! Faces are the stores' face ids (the fixture's face order), edges the
//! pairs of their vertex ids, low first, as the oracle names them.

use std::collections::{BTreeMap, BTreeSet};

use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use varde_decide::ids::FaceId;
use varde_decide::leaves::flat::{Kind as LeafKind, Leaf};
use varde_decide::records::keys::Operand;
use varde_testkit::rational_oracle::{ExactPair, Kind};

use super::super::target::Map;
use super::run::{Setup, Snap};
use crate::geom::lattice::CELL;

/// A disagreement with the oracle.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mismatch {
    pub(crate) what: String,
}

fn f(x: &Rational) -> f64 {
    f64::rounding_from(x, Nearest).0
}

/// The face-pair kinds against the oracle (module docs).
pub(crate) fn face_pairs(
    s: &Setup<'_>,
    exact: &ExactPair,
    leaves: &[Leaf],
    b_k: f64,
) -> (usize, Vec<Mismatch>) {
    // Per face pair, the leaf kinds and the directions of its tangent
    // leaves.
    #[derive(Default)]
    struct Kinds {
        empty: bool,
        simple: bool,
        near: Vec<[Rational; 3]>,
        contact: Vec<[Rational; 3]>,
    }
    let mut by: BTreeMap<(u32, u32), Kinds> = BTreeMap::new();
    for l in leaves {
        let k = by.entry((l.a.face().0, l.b.face().0)).or_default();
        match &l.kind {
            LeafKind::Empty { .. } => k.empty = true,
            LeafKind::Simple { .. } => k.simple = true,
            LeafKind::Near(t) => k.near.push(t.ell.clone()),
            LeafKind::Contact(t) => k.contact.push(t.ell.clone()),
        }
    }
    let _ = s;
    let mut bad = Vec::new();
    let pairs = exact.face_pairs(0.0).expect("the oracle takes the fixture");
    let meet: BTreeMap<(u32, u32), &Kind> = pairs.iter().map(|p| ((p.a, p.b), &p.kind)).collect();
    let mut checked = 0;
    for (&(fa, fb), k) in &by {
        checked += 1;
        let oracle = meet.get(&(fa, fb));
        // Tangent leaves: the gap along their direction within `b_K`.
        for (ells, opposed) in [(&k.near, false), (&k.contact, true)] {
            let mut seen: BTreeSet<[Rational; 3]> = BTreeSet::new();
            for ell in ells {
                if !seen.insert(ell.clone()) {
                    continue;
                }
                let l = [f(&ell[0]), f(&ell[1]), f(&ell[2])];
                match exact.gap(&[fa], &[fb], l) {
                    Ok(g) => {
                        if let Some([lo, hi]) = g.range() {
                            let bk = Rational::try_from(b_k).expect("finite");
                            // `s` is in units of `|ℓ|`: compare `|s|·|ℓ|`.
                            let n2 = &ell[0] * &ell[0] + &ell[1] * &ell[1] + &ell[2] * &ell[2];
                            let within = |x: &Rational| x * x * &n2 <= &bk * &bk;
                            if !(within(&lo) && within(&hi)) {
                                bad.push(Mismatch {
                                    what: format!(
                                        "faces {fa},{fb}: tangent leaf, gap {:?} beyond b_K",
                                        [f(&lo), f(&hi)]
                                    ),
                                });
                            }
                        }
                    }
                    Err(e) => bad.push(Mismatch {
                        what: format!("faces {fa},{fb}: oracle refused ℓ: {e:?}"),
                    }),
                }
                if let Some(Kind::Coplanar { opposed: o, .. }) = oracle
                    && *o != opposed
                {
                    bad.push(Mismatch {
                        what: format!("faces {fa},{fb}: orientation"),
                    });
                }
            }
        }
        match oracle {
            None | Some(Kind::Apart) => {
                if k.simple || !k.near.is_empty() || !k.contact.is_empty() {
                    // Faces apart may still be within `b_K` (offset
                    // sheets): only a Simple leaf is wrong there.
                    if k.simple {
                        bad.push(Mismatch {
                            what: format!("faces {fa},{fb}: apart, Simple leaf"),
                        });
                    }
                }
            }
            Some(Kind::Coplanar { opposed, area2, .. }) => {
                if *area2 > 0u32 {
                    let has = if *opposed {
                        !k.contact.is_empty()
                    } else {
                        !k.near.is_empty()
                    };
                    if !has {
                        bad.push(Mismatch {
                            what: format!("faces {fa},{fb}: coplanar overlap, no tangent leaf"),
                        });
                    }
                }
            }
            Some(Kind::Meet { .. }) => {}
        }
    }
    // Pairs the oracle finds meeting that no leaf names.
    for p in &pairs {
        if p.dist2 == 0u32 && !by.contains_key(&(p.a, p.b)) {
            bad.push(Mismatch {
                what: format!("faces {},{}: meet, no leaves", p.a, p.b),
            });
        }
        if let Kind::Meet { .. } = &p.kind
            && p.is_transversal()
            && let Some(k) = by.get(&(p.a, p.b))
            && !(k.simple || !k.near.is_empty() || !k.contact.is_empty())
        {
            bad.push(Mismatch {
                what: format!("faces {},{}: transversal, only Empty leaves", p.a, p.b),
            });
        }
    }
    (checked, bad)
}

/// A snap's `ℓ`, rounded.
pub(crate) fn ell(z: &Snap) -> Option<[f64; 3]> {
    match &z.target.map {
        Map::Along(l) => Some([f(&l[0]), f(&l[1]), f(&l[2])]),
        Map::Closest => None,
    }
}

/// UV point `p` of face `face` of `o` in space.
fn at(s: &Setup<'_>, o: Operand, face: FaceId, p: &[Rational; 2]) -> [f64; 3] {
    let lat = s.body(o).brep.surface(face).lattice();
    let (org, [u, v, _]) = (lat.origin(), lat.axes());
    let (a, b) = ((f(&p[0]) - 0.5) * CELL, (f(&p[1]) - 0.5) * CELL);
    [0, 1, 2].map(|k| org[k] + a * u[k] + b * v[k])
}

/// The zone's region area projected along `ℓ` against the oracle's, and
/// the relative difference.
pub(crate) fn zone_area(s: &Setup<'_>, exact: &ExactPair, z: &Snap, b_k: f64) -> (f64, f64) {
    let l = ell(z).expect("a direction");
    // The oracle's axis: `ℓ`'s largest component, the first on a tie.
    let axis = (0..3).fold(0, |m, k| if l[k].abs() > l[m].abs() { k } else { m });
    let s_op = z.target.operand.other();
    // Project along `ℓ` onto the plane `x_axis = 0`.
    let proj = |p: [f64; 3]| -> [f64; 2] {
        let t = p[axis] / l[axis];
        let q: Vec<f64> = (0..3)
            .filter(|&k| k != axis)
            .map(|k| p[k] - t * l[k])
            .collect();
        [q[0], q[1]]
    };
    // Per face its region's signed area (one sign per face: its UV
    // turns one way about the projection), summed unsigned.
    let mut per: BTreeMap<FaceId, f64> = BTreeMap::new();
    for b in &z.rim.bounds {
        let p = proj(at(s, s_op, b.face, &b.ends[0]));
        let r = proj(at(s, s_op, b.face, &b.ends[1]));
        *per.entry(b.face).or_default() += 0.5 * (p[0] * r[1] - p[1] * r[0]);
    }
    let area: f64 = per.values().map(|a| a.abs()).sum();
    // The oracle's cells over the zone's face pairs, within `b_K`.
    let (s_faces, t_faces): (Vec<u32>, Vec<u32>) = (
        z.faces.iter().map(|f| f.0).collect(),
        z.targets.iter().map(|t| t.0.0).collect(),
    );
    let (af, bf) = match s_op {
        Operand::A => (s_faces, t_faces),
        Operand::B => (t_faces, s_faces),
    };
    let g = exact.gap(&af, &bf, l).expect("faces graphs over ℓ⊥");
    let bk = Rational::try_from(b_k).expect("finite");
    let mut exact_want = Rational::from(0);
    for c in &g.cells {
        let w = |x: &Rational| x * x <= &bk * &bk;
        if w(c.min_s()) && w(c.max_s()) {
            exact_want += &c.area;
        }
    }
    let want = f(&exact_want);
    let rel = if want == 0.0 {
        area
    } else {
        ((area - want) / want).abs()
    };
    (area, rel)
}

/// The coincident edge pairs `(snapped edge, target edge)` of a zone by
/// the oracle (module docs), as vertex-id pairs low first.
pub(crate) fn coincident(
    s: &Setup<'_>,
    exact: &ExactPair,
    z: &Snap,
) -> BTreeSet<([u32; 2], [u32; 2])> {
    let l = ell(z).expect("a direction");
    let s_op = z.target.operand.other();
    let t_op = z.target.operand;
    let edges_of = |o: Operand, faces: &[FaceId]| -> BTreeSet<[u32; 2]> {
        let br = &s.body(o).brep;
        let mut out = BTreeSet::new();
        for &fc in faces {
            for lp in br.loops(fc) {
                for c in br.loop_coedges(lp) {
                    out.insert(br.ends(br.edge(c)).map(|v| v.0));
                }
            }
        }
        out
    };
    let se = edges_of(s_op, &z.faces);
    let tf: Vec<FaceId> = z.targets.iter().map(|t| t.0).collect();
    let te = edges_of(t_op, &tf);
    let all = exact.coincident_edges(l).expect("a direction");
    all.into_iter()
        .filter_map(|o| {
            let (sa, ta) = match s_op {
                Operand::A => (o.a, o.b),
                Operand::B => (o.b, o.a),
            };
            (se.contains(&sa) && te.contains(&ta)).then_some((sa, ta))
        })
        .collect()
}
