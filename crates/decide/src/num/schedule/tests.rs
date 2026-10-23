//! The schedule's levels, the escalation rule, promotion along the
//! levels, and the unit table's shape.

use super::*;
use crate::num::bigf::{Big, BigF};
use crate::num::dd::Dd;

/// The levels run `L0` to `L4` in order, one width each above
/// double-double, the last [`P_MAX_BITS`] wide; nothing follows it.
#[test]
fn the_schedule_runs_from_f64_to_848_bits() {
    let mut seq = vec![Level::L0];
    while let Some(next) = seq.last().and_then(|l| l.next()) {
        seq.push(next);
    }
    assert_eq!(seq, Level::ALL);
    assert_eq!(seq.len(), LEVELS);
    assert_eq!(Level::TOP.next(), None);
    assert_eq!(*seq.last().unwrap(), Level::TOP);
    for (i, l) in Level::ALL.into_iter().enumerate() {
        assert_eq!(l.index(), i);
        assert_eq!(Level::from_index(i), Some(l));
    }
    assert_eq!(Level::from_index(LEVELS), None);
    let widths: Vec<u32> = Level::ALL
        .iter()
        .filter_map(|l| l.width())
        .map(|w| w.bits())
        .collect();
    assert_eq!(widths, [212, 424, 848]);
    assert_eq!(Level::L0.width(), None);
    assert_eq!(Level::L1.width(), None);
    assert_eq!(Level::TOP.width().map(|w| w.bits()), Some(P_MAX_BITS));
    assert!(Level::ALL.windows(2).all(|w| w[0] < w[1]));
}

/// Precision while the rounding error dominates, space otherwise: equal
/// parts refine in space, an uncertified rounding error in precision, an
/// uncertified truncation error in space.
#[test]
fn rounding_dominating_escalates_in_precision_else_space() {
    let at = |t: f64, r: f64| escalation(&Bounded::new(1.0, t, r));
    assert_eq!(at(1e-20, 1e-18), Escalation::Precision);
    assert_eq!(at(1e-18, 1e-20), Escalation::Space);
    assert_eq!(at(1e-18, 1e-18), Escalation::Space);
    assert_eq!(at(0.0, 0.0), Escalation::Space);
    assert_eq!(at(0.0, f64::MIN_POSITIVE), Escalation::Precision);
    assert_eq!(at(1.0, f64::INFINITY), Escalation::Precision);
    assert_eq!(at(f64::INFINITY, 1.0), Escalation::Space);
    assert_eq!(at(1.0, f64::NAN), Escalation::Space);
}

/// A value promoted level by level up to the top keeps every bit and
/// both error parts: `1 + 2⁻⁸⁰` (not an `f64`) from double-double to 848
/// bits, and an `f64` from the first level.
#[test]
fn promotion_up_the_schedule_is_exact() {
    let (t, r) = (3e-30, 5e-31);
    let tiny = f64::from_bits((1023 - 80) << 52);
    let dd = Bounded::new(Dd::sum(1.0, tiny), t, r);
    let mut big = dd.promote();
    for l in &Level::ALL[2..] {
        let w = l.width().expect("a big-float level");
        big = big.widen(w);
        assert_eq!(big.v.width(), w, "{l:?}");
        assert_eq!((big.err_trunc, big.err_round), (t, r), "{l:?}");
        let rest = big.v.sub(&Big::from_f64(1.0, w), w);
        assert_eq!(rest.v.to_f64(), tiny, "{l:?}: the bits past f64 kept");
    }
    let first = Bounded::new(0.1, t, r).promote();
    assert_eq!((first.v.hi(), first.v.lo()), (0.1, 0.0));
    let top = first.promote().widen(Level::TOP.width().unwrap());
    assert_eq!(top.v.to_f64(), 0.1);
    assert_eq!((top.err_trunc, top.err_round), (t, r));
}

/// Every kind with levels costs at least as much at a level as at the one
/// below, and something at each: a precision step is never free, nor
/// cheaper than the computation it repeats. Every predicate kind prices
/// each of its stages, a later stage dearer than the filter. The
/// operand scan prices a position and a net, the net dearer; a form key
/// settles exactly at no less than its top level; a tree's leaf built
/// costs more than a box tested; a fold check's whole exact numerator
/// more than one of its lines, a level's numerator more than a piece.
#[test]
fn the_unit_table_grows_with_the_level() {
    let r = &UNITS.record;
    for (kind, row) in [
        ("seam cell", UNITS.seam_cell),
        ("implicit cell, quadric", UNITS.implicit_cell.quadric),
        ("implicit cell, quartic", UNITS.implicit_cell.quartic),
        ("count piece", UNITS.count_piece),
        ("record eval", r.eval),
        ("record root", r.root),
        ("xrec piece", UNITS.xrec_piece),
        ("foot polynomial", r.foot_poly),
        ("key at", r.key_at),
        ("at end", r.at_end),
        ("z-order box", r.zorder_box),
        ("form key level", UNITS.form_key.level),
        ("fold level", UNITS.fold.level),
        ("fold piece", UNITS.fold.piece),
    ] {
        assert!(row[0] > 0, "{kind}: {row:?}");
        assert!(row.windows(2).all(|w| w[0] <= w[1]), "{kind}: {row:?}");
    }
    for k in PredKind::ALL {
        let e = r.of(k);
        assert!(e.filter > 0 && e.expansion > 0, "{k:?}: {e:?}");
        assert!(e.rational > e.filter, "{k:?}: {e:?}");
    }
    assert!(r.rational > 0 && r.hit > 0);
    const { assert!(UNITS.face_setup > 0) };
    // A net is a tile's nine control points: dearer than one position.
    let scan = UNITS.operand_scan;
    assert!(scan.position > 0 && scan.net > scan.position, "{scan:?}");
    // Settling exactly is dearer than any level; forming the exact value
    // costs something.
    let key = UNITS.form_key;
    assert!(
        key.rational > 0 && key.settle >= key.level[LEVELS - 1],
        "{key:?}"
    );
    // Building a leaf is dearer than testing a box against it, which
    // costs something.
    let bvh = UNITS.bvh;
    assert!(
        bvh.hit > 0 && bvh.build > bvh.test && bvh.test > 0,
        "{bvh:?}"
    );
    // The fold check's exact part costs something, the whole numerator
    // more than one of its lines; a level's numerator more than a piece.
    let fold = UNITS.fold;
    assert!(fold.line > 0 && fold.full > fold.line, "{fold:?}");
    assert!(
        (0..LEVELS).all(|l| fold.level[l] > fold.piece[l]),
        "{fold:?}"
    );
}

/// The kinds index their rows in order, each once.
#[test]
fn predicate_kinds_index_their_rows() {
    for (i, k) in PredKind::ALL.into_iter().enumerate() {
        assert_eq!(k.index(), i);
    }
    assert_eq!(PredKind::ALL.len(), PredKind::COUNT);
}
