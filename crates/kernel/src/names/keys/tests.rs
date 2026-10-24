//! Digests pinned (they are kept with bodies' names and never change),
//! telling every field and part apart; the term table interning,
//! merging and refusing a collision; keys in the leaf order.

use super::*;

fn ctx(feature: u64, instance: u32) -> OpCtx {
    OpCtx {
        feature: FeatureId(feature),
        instance: InstanceId(instance),
    }
}

fn key(feature: u64, instance: u32, part: FacePart) -> FaceKey {
    FaceKey::new(ctx(feature, instance), part)
}

/// One part of each kind, fields 1 and 2 where it has them.
fn parts() -> Vec<FacePart> {
    let d = Digest::from_u64(1);
    vec![
        FacePart::StartCap { region: 1 },
        FacePart::EndCap { region: 1 },
        FacePart::Side {
            curve: 1,
            segment: 2,
        },
        FacePart::Turn { curve: 1, turn: 2 },
        FacePart::Primitive(1),
        FacePart::Blend { chain: d },
        FacePart::BlendCorner { edges: d, k: 2 },
        FacePart::Offset { of: d },
        FacePart::Loft { curve: 1, span: 2 },
        FacePart::BackSide {
            curve: 1,
            segment: 2,
        },
    ]
}

#[test]
fn digests_are_pinned() {
    // The empty mix and one word 0; a key's words are 1 (a face), its
    // feature and copy, then the part's number and fields: here
    // `[1, 7, 3, 3, 11, 2]`.
    let mut m = Mix::new();
    assert_eq!(
        m.finish().to_bits(),
        0x9036_1817_88bf_3aff_2800_664e_7f20_29e8
    );
    m.push(0);
    assert_eq!(
        m.finish().to_bits(),
        0x748e_d868_6f26_4838_83bf_d77c_4c96_958e
    );
    let k = key(
        7,
        3,
        FacePart::Side {
            curve: 11,
            segment: 2,
        },
    );
    assert_eq!(
        k.digest().to_bits(),
        0x3d89_a108_8b80_eb2c_4e3f_d667_e5c8_d6af
    );
    let corner = Term::Edges(vec![
        Between::new(k, key(7, 3, FacePart::StartCap { region: 0 })),
        Between::new(k, k),
    ]);
    assert_eq!(
        corner.digest().to_bits(),
        0xc369_bdf4_72ed_61ac_5fd6_7bee_cd8e_fff9
    );
    let offset = key(
        9,
        0,
        FacePart::Offset {
            of: Digest::from_bits(u128::MAX - 5),
        },
    );
    assert_eq!(
        offset.digest().to_bits(),
        0xdbba_4581_1803_05bb_b387_5ac0_81a1_dc65
    );
}

#[test]
fn a_callers_name_is_zero_extended() {
    let d = Digest::from_u64(u64::MAX);
    assert_eq!(d.to_bits(), u128::from(u64::MAX));
    assert_eq!(Digest::from_bits(d.to_bits()), d);
    assert!(Digest::from_u64(1) < Digest::from_u64(2));
}

#[test]
fn every_field_and_part_has_its_own_digest() {
    let mut keys = Vec::new();
    for part in parts() {
        keys.push(key(1, 1, part));
        keys.push(key(2, 1, part));
        keys.push(key(1, 2, part));
    }
    // Each field changed alone, and two fields swapped.
    keys.extend([
        key(1, 1, FacePart::StartCap { region: 2 }),
        key(
            1,
            1,
            FacePart::Side {
                curve: 2,
                segment: 1,
            },
        ),
        key(
            1,
            1,
            FacePart::Side {
                curve: 1,
                segment: 3,
            },
        ),
        key(1, 1, FacePart::Turn { curve: 2, turn: 1 }),
        key(1, 1, FacePart::Primitive(2)),
        key(
            1,
            1,
            FacePart::Blend {
                chain: Digest::from_u64(2),
            },
        ),
        key(
            1,
            1,
            FacePart::Blend {
                chain: Digest::from_bits(1 << 64),
            },
        ),
        key(
            1,
            1,
            FacePart::BlendCorner {
                edges: Digest::from_u64(1),
                k: 3,
            },
        ),
        key(
            1,
            1,
            FacePart::Offset {
                of: Digest::from_u64(2),
            },
        ),
        key(1, 1, FacePart::Loft { curve: 1, span: 3 }),
    ]);
    let mut digests: Vec<Digest> = keys.iter().map(FaceKey::digest).collect();
    let n = digests.len();
    digests.sort();
    digests.dedup();
    assert_eq!(digests.len(), n, "every key has its own digest");
    // A key's digest is its term's, and differs from a list of edges'.
    let k = keys[0];
    assert_eq!(Term::Face(k).digest(), k.digest());
    assert_ne!(Term::Edges(vec![]).digest(), k.digest());
    let e = Between::new(keys[0], keys[1]);
    let t = |v: Vec<Between>| Term::Edges(v).digest();
    assert_ne!(t(vec![e]), t(vec![e, e]), "repeats count");
    assert_ne!(t(vec![]), t(vec![e]));
}

#[test]
fn the_table_interns_each_term_once() {
    let mut t = TermTable::default();
    assert!(t.is_empty());
    let a = key(1, 0, FacePart::Primitive(0));
    let b = key(1, 0, FacePart::Primitive(1));
    let c = key(2, 0, FacePart::EndCap { region: 4 });
    let d = t.face(a).unwrap();
    assert_eq!(d, a.digest());
    assert_eq!(t.face(a).unwrap(), d);
    assert_eq!(t.get(d), Some(&Term::Face(a)));
    // A corner's edges in any order give one digest; repeats count.
    let (ab, bc) = (Between::new(a, b), Between::new(c, b));
    let e = t.corner([bc, ab]).unwrap();
    assert_eq!(t.corner([ab, bc]).unwrap(), e);
    assert_eq!(t.get(e), Some(&Term::Edges(vec![ab, bc])));
    assert_ne!(t.corner([ab, bc, ab]).unwrap(), e);
    assert_eq!(t.len(), 3);
    // A caller's name has no term.
    assert_eq!(t.get(Digest::from_u64(5)), None);
    let digests: Vec<Digest> = t.iter().map(|(d, _)| d).collect();
    assert!(digests.is_sorted());
}

#[test]
fn merging_adds_the_others_terms_unless_one_collides() {
    let a = key(1, 0, FacePart::Primitive(0));
    let b = key(1, 0, FacePart::Primitive(1));
    let mut s = TermTable::default();
    s.face(a).unwrap();
    let mut t = TermTable::default();
    t.face(a).unwrap();
    t.face(b).unwrap();
    s.merge(&t).unwrap();
    assert_eq!(s, t);
    // A forged entry: `b`'s digest standing for another term.
    let mut forged = TermTable::default();
    forged.terms.insert(b.digest(), Term::Face(a));
    let mut u = TermTable::default();
    u.face(key(3, 0, FacePart::Primitive(0))).unwrap();
    let before = u.clone();
    u.face(b).unwrap();
    let held = u.clone();
    let err = u.merge(&forged).unwrap_err();
    assert_eq!(
        err,
        DigestCollision {
            digest: b.digest(),
            held: Term::Face(b),
            other: Term::Face(a),
        }
    );
    assert_eq!(u, held, "nothing added");
    assert_ne!(u, before);
    // Interning a term whose digest the table holds for another.
    let err = forged.face(b).unwrap_err();
    assert_eq!((err.held, err.other), (Term::Face(a), Term::Face(b)));
}

#[test]
fn keys_order_by_feature_then_copy_then_part() {
    let side = FacePart::Side {
        curve: 0,
        segment: 0,
    };
    let cap = FacePart::StartCap { region: 9 };
    let mut keys = vec![
        key(2, 0, cap),
        key(1, 1, cap),
        key(1, 0, side),
        key(1, 1, side),
        key(1, 0, cap),
    ];
    keys.sort();
    assert_eq!(
        keys,
        [
            key(1, 0, cap),
            key(1, 0, side),
            key(1, 1, cap),
            key(1, 1, side),
            key(2, 0, cap),
        ]
    );
    let k = key(4, 2, cap);
    assert_eq!(k.ctx(), ctx(4, 2));
    assert_eq!(FaceKey::new(k.ctx(), k.part), k);
}
