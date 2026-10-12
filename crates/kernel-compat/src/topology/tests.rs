use glam::DVec3;

use super::*;
use crate::mesh::{FaceName, FacePart, PartKey};

fn key(feature: u64, part: PartKey) -> FaceKey {
    FaceKey {
        feature,
        part,
        instance: 0,
    }
}

/// A box's side: 0 at `-y`, 1 at `+x`, 2 at `+y`, 3 at `-x`.
fn side(curve: u64) -> PartKey {
    PartKey::Side { curve }
}

#[test]
fn the_mix_is_fixed() {
    // Stored in files: these values must never change. Worked out
    // independently of this code, from the definition.
    assert_eq!(mix(&[]), 0);
    assert_eq!(mix(&[0]), 0x910a_2dec_8902_5cc1);
    assert_eq!(mix(&[1, 2, 3]), 0x25a1_5d91_607b_47d2);
    assert_eq!(mix(&[3, 2, 1]), 0x4f02_9006_f812_af6c);
    assert_eq!(mix(&[0, 0]), 0x6468_4c4f_0fd7_84b4);
    assert_eq!(key(7, side(3)).mixed(), 0x90bb_bddc_bee9_3480);
    // Order and length count.
    assert_ne!(mix(&[1, 2]), mix(&[2, 1]));
    assert_ne!(mix(&[0]), mix(&[0, 0]));
}

#[test]
fn derived_names_follow_their_parts() {
    let (a, b) = (key(1, PartKey::EndCap), key(2, side(5)));
    assert_eq!(blend_edge([a, b], 0), blend_edge([b, a], 0));
    assert_ne!(blend_edge([a, b], 0), blend_edge([a, b], 1));
    // A copy of a copy is unique and stable.
    let name = FaceName::new(1, FacePart::EndCap);
    let copy = name.copy(9, 2);
    assert_eq!(copy, name.copy(9, 2));
    assert_ne!(copy.instance, 0);
    assert_ne!(copy.copy(9, 2).instance, copy.instance);
    assert_ne!(name.copy(9, 1).instance, copy.instance);
    assert_eq!(copy.key().instance, copy.instance);
    // Pieces of one wall share a key; a loft's spans don't.
    let piece = |segment| FaceName::new(1, FacePart::Side { curve: 4, segment });
    assert_eq!(piece(0).key(), piece(3).key());
    let span = |span| {
        FaceName::new(
            1,
            FacePart::Lofted {
                curve: 4,
                span,
                segment: 0,
            },
        )
    };
    assert_ne!(span(0).key(), span(1).key());
}

#[test]
fn the_empty_solid_s_topology_resolves_nothing() {
    let solid = Solid::empty();
    let topology = solid.topology();
    let (a, b, c) = (key(1, PartKey::EndCap), key(1, side(0)), key(1, side(1)));
    let far = DVec3::splat(f64::NAN);
    assert_eq!(topology.face(&solid, &a, far), Err(NotFound::Face));
    assert_eq!(topology.edge(&solid, [a, b], far), Err(NotFound::Edge));
    assert_eq!(
        topology.corner(&solid, [a, b, c], far),
        Err(NotFound::Corner)
    );
    assert!(topology.tangent_chains(&solid).is_empty());
    assert_eq!(topology.region_of(u32::MAX), u32::MAX);
    assert_eq!(topology.form(&solid, u32::MAX), &crate::mesh::Form::Unknown);
    assert_eq!(topology.runs_with(u32::MAX, &[a, b]), None);
    assert!(topology.corner_point(&solid, [a, b, c], far).is_err());
    assert!(topology.edge_direction(&solid, [a, b], far).is_err());
    assert!(topology.normal(&solid, &a, far).is_err());
}
