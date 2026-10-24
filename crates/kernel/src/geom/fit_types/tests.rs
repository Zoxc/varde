//! Intents' extension laws, the ids they name and their remapping, and
//! exact rows' remapping.

use super::*;
use crate::geom::form::PlaneKey;

fn c(i: u32) -> CurveRef {
    CurveId(i)
}

fn axis() -> Axis {
    Axis {
        point: [0.0; 3],
        dir: [0.0, 0.0, 1.0],
    }
}

/// One intent of every kind, each naming distinct ids.
fn every() -> Vec<Intent> {
    let helix = PathRef::Helix {
        axis: axis(),
        radius: 2.0,
        pitch: 0.5,
        turns: 3.0,
        handedness: Hand::Right,
    };
    vec![
        Intent::Form(PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into()),
        Intent::Offset {
            base: SurfId(1),
            d: 0.25,
        },
        Intent::Extrusion {
            profile: c(1),
            dir: [0.0, 0.0, 1.0],
        },
        Intent::Revolution {
            profile: c(2),
            axis: axis(),
        },
        Intent::Ruled {
            rails: [c(3), c(4)],
        },
        Intent::ConstantSlope {
            profile: c(5),
            pull: [0.0, 0.0, 1.0],
            angle: 3.0,
        },
        Intent::RollingBall {
            supports: [SurfId(2), SurfId(3)],
            radius: Law::Constant(1.0),
            spine: c(6),
        },
        Intent::Sweep {
            profile: c(7),
            path: PathRef::Curve(c(8)),
            frame: FrameLaw::RotationMinimising,
            scale: Law::Constant(1.0),
        },
        Intent::Sweep {
            profile: c(9),
            path: helix,
            frame: FrameLaw::Frenet,
            scale: Law::Constant(1.0),
        },
        Intent::Loft {
            sections: vec![c(10), c(11)],
            rails: vec![c(12)],
        },
        Intent::VertexBlend {
            boundaries: vec![c(13), c(14), c(15)],
        },
        Intent::Import {
            entity: ImportRef(42),
        },
    ]
}

/// Each intent continues by its stated law; an affine image by its
/// inner one's, at any depth; only extrapolations report a deviation.
#[test]
fn every_intent_states_its_extension_law() {
    use ExtensionLaw as L;
    let laws: Vec<L> = every().iter().map(Intent::extension).collect();
    let want = [
        L::Defining,
        L::Defining,
        L::Defining,
        L::Defining,
        L::Defining,
        L::Defining,
        L::Spine,
        L::Path,
        L::Path,
        L::EndRows,
        L::BoundaryRows,
        L::BoundaryRows,
    ];
    assert_eq!(laws, want);
    let map = Affine3 {
        linear: [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]],
        shift: [1.0, 0.0, 0.0],
    };
    for i in every() {
        let law = i.extension();
        let twice = Intent::Affine {
            map,
            inner: Box::new(Intent::Affine {
                map,
                inner: Box::new(i),
            }),
        };
        assert_eq!(twice.extension(), law);
    }
    let reported: Vec<L> = want.into_iter().filter(|l| l.reports_dev()).collect();
    assert_eq!(reported, [L::BoundaryRows; 2]);
}

/// The ids an intent names come in its fields' order, an affine image's
/// inner ones included; a helix and a form name none.
#[test]
fn intents_name_their_ids_in_order() {
    use IntentId::{Curve, Surface};
    let ids: Vec<Vec<IntentId>> = every().iter().map(Intent::ids).collect();
    let cs = |v: &[u32]| v.iter().map(|&i| Curve(c(i))).collect::<Vec<_>>();
    let want = vec![
        vec![],
        vec![Surface(SurfId(1))],
        cs(&[1]),
        cs(&[2]),
        cs(&[3, 4]),
        cs(&[5]),
        vec![Surface(SurfId(2)), Surface(SurfId(3)), Curve(c(6))],
        cs(&[7, 8]),
        cs(&[9]),
        cs(&[10, 11, 12]),
        cs(&[13, 14, 15]),
        vec![],
    ];
    assert_eq!(ids, want);
    let wrapped = Intent::Affine {
        map: Affine3 {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            shift: [0.0; 3],
        },
        inner: Box::new(every().swap_remove(6)),
    };
    assert_eq!(wrapped.ids(), want[6]);
}

/// Remapping moves every id it names and nothing else, and refuses an id
/// that maps to nothing.
#[test]
fn remapping_moves_every_id() {
    let mut up_s = |s: SurfId| Some(SurfId(s.0 + 100));
    let mut up_c = |x: CurveId| Some(CurveId(x.0 + 100));
    for i in every() {
        let moved = i.remap(&mut up_s, &mut up_c).unwrap();
        let shifted: Vec<IntentId> = (i.ids().into_iter())
            .map(|id| match id {
                IntentId::Surface(s) => IntentId::Surface(SurfId(s.0 + 100)),
                IntentId::Curve(x) => IntentId::Curve(CurveId(x.0 + 100)),
            })
            .collect();
        assert_eq!(moved.ids(), shifted);
        assert_eq!(moved.extension(), i.extension());
        let mut back_s = |s: SurfId| Some(SurfId(s.0 - 100));
        let mut back_c = |x: CurveId| Some(CurveId(x.0 - 100));
        assert_eq!(moved.remap(&mut back_s, &mut back_c), Some(i.clone()));
        // An id dropped: refused wherever it is named.
        let named = !i.ids().is_empty();
        let mut drop_s = |s: SurfId| (s.0 != 3).then_some(s);
        let mut drop_c = |x: CurveId| (x.0 % 2 == 0).then_some(x);
        let dropped = i.ids().iter().any(|id| match *id {
            IntentId::Surface(s) => s.0 == 3,
            IntentId::Curve(x) => x.0 % 2 == 1,
        });
        let r = i.remap(&mut drop_s, &mut drop_c);
        assert_eq!(r.is_none(), dropped, "{i:?}");
        assert!(named || r == Some(i));
    }
}

/// Exact rows remap their curves, in order, and refuse one dropped.
#[test]
fn exact_rows_remap_their_curves() {
    let row = |along, at, first, curve| ExactRow {
        along,
        at,
        first,
        curve: c(curve),
    };
    let rows = ExactRows::new(vec![row(Dir::U, 0, -1, 4), row(Dir::V, 3, 0, 7)]);
    assert_eq!(ExactRows::default().rows(), []);
    let moved = rows.remap(&mut |x| Some(CurveId(x.0 * 2))).unwrap();
    assert_eq!(moved.rows(), [row(Dir::U, 0, -1, 8), row(Dir::V, 3, 0, 14)]);
    assert_eq!(rows.remap(&mut |x| (x.0 != 7).then_some(x)), None);
}
