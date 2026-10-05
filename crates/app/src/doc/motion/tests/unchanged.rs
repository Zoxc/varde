//! OK on a feature opened for editing and left as it was writes nothing,
//! for every kind of feature, as each kind's session made it.

use varde_document::{AxisLine, BodyOp, Combine, FeatureId, FeatureKind, OriginPlane};
use varde_view::{Angle, Edit, Look, MotionField, MotionLook, MotionPick, RevolveLook, TurnKind};

use super::holding;
use super::{Plates, plates};

/// [`plates`] with, as their sessions make them: Body 2 moved and turned
/// about the Z axis, then mirrored across YZ, Body 3 in a linear pattern
/// and Body 2 in a circular one, and Body 3 cut from Body 1 by a combine
/// (as a file would hold it). Answered by `regen`, so that its cache
/// holds the patterns' and the combine's work for the test after.
fn transformed(regen: &mut varde_regen::Regenerator) -> Plates {
    let mut plates = plates();
    let [plate, right, left] = plates.bodies;
    let commit = |plates: &mut Plates, regen: &mut varde_regen::Regenerator| {
        answer(plates, regen);
        plates.doc.update(Edit::CommitMotion);
        assert!(plates.doc.motion.is_none(), "committed");
        answer(plates, regen);
    };
    plates.click(right);
    plates.doc.look(Look::StartMove);
    plates.input(MotionField::Offset(varde_document::Axis3::X), "5");
    plates.input(MotionField::Angle, "30");
    commit(&mut plates, regen);
    plates.click(right);
    plates.doc.look(Look::StartMirror);
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.motion(MotionLook::OriginPlane(OriginPlane::YZ));
    commit(&mut plates, regen);
    plates.click(left);
    plates.doc.look(Look::StartPattern);
    plates.input(MotionField::Count, "4");
    plates.input(MotionField::Spread, "12");
    commit(&mut plates, regen);
    plates.click(right);
    plates.doc.look(Look::StartCircularPattern);
    commit(&mut plates, regen);
    let document = plates.doc.editor.document();
    let combine = Combine {
        target: plate,
        tools: vec![left],
        op: BodyOp::Subtract,
        keep_tools: false,
    };
    plates.doc.apply(document.add_feature(combine.into()));
    answer(&mut plates, regen);
    plates
}

/// Answers the requests waiting of `plates` with `regen`, keeping its
/// cache, as the document's lane does.
fn answer(plates: &mut Plates, regen: &mut varde_regen::Regenerator) {
    for request in plates.requests.take() {
        plates.doc.computed(regen.handle(request));
    }
}

/// A revolve of a rectangle a quarter turn about its sketch's Y axis, as
/// its session made it, held as plates are.
fn revolved() -> Plates {
    let mut lathe = crate::doc::revolve::tests::lathe();
    lathe.set_up(AxisLine::SketchY);
    lathe.revolve(RevolveLook::Extent(TurnKind::OneSide));
    lathe.input(Angle::First, "90");
    lathe.answer();
    lathe.doc.update(Edit::CommitRevolve);
    assert!(lathe.doc.revolve.is_none(), "committed");
    let document = lathe.doc.editor.document().clone();
    let body = document.bodies()[0].id;
    let (doc, requests) = holding(document);
    Plates {
        doc,
        requests,
        bodies: [body; 3],
    }
}

/// Opens the feature `id` of `plates` for editing and presses OK (Accept
/// error where its preview failed): nothing is written, not even a
/// revision with no change.
fn ok_writes_nothing(plates: &mut Plates, regen: &mut varde_regen::Regenerator, id: FeatureId) {
    // Answered by `regen`, whose cache is kept between the features of
    // one fixture, as the document's lane does.
    let kind = plates
        .doc
        .editor
        .document()
        .feature(id)
        .unwrap()
        .kind
        .clone();
    let name = format!("{kind:?}");
    answer(plates, regen);
    let revision = plates.doc.editor.revision();
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::EditFeature(id));
    answer(plates, regen);
    let commit = match kind {
        FeatureKind::Extrude(_) => Edit::CommitExtrude,
        FeatureKind::Revolve(_) => Edit::CommitRevolve,
        FeatureKind::Combine(_) => Edit::CommitCombine,
        _ => Edit::CommitMotion,
    };
    let open = |plates: &Plates| {
        let doc = &plates.doc;
        doc.motion.is_some()
            || doc.extrude.is_some()
            || doc.revolve.is_some()
            || doc.combine.is_some()
    };
    assert!(open(plates), "not opened: {name}");
    plates.doc.update(commit);
    if open(plates) {
        plates.doc.update(Edit::AcceptError);
    }
    assert!(!open(plates), "still open: {name}");
    assert_eq!(*plates.doc.editor.document(), before, "{name}");
    assert_eq!(plates.doc.editor.revision(), revision, "{name}");
}

/// Every feature of every kind but a sketch, opened and OK'd unchanged,
/// writes nothing.
#[test]
fn ok_on_any_feature_opened_and_left_as_it_was_writes_nothing() {
    type Fixture = (&'static str, fn(&mut varde_regen::Regenerator) -> Plates);
    let fixtures: [Fixture; 13] = [
        ("transformed", transformed),
        ("revolved", |_| revolved()),
        ("chamfer", |_| super::chamfer::made().0),
        ("fillet", |_| super::fillet::made().0),
        ("shell", |_| super::shell::made().0),
        ("offset face", |_| super::offset_face::made().0),
        ("draft", |_| super::face_draft::made().0),
        ("split", |_| super::split::made().0),
        ("sweep", |_| super::sweep::made().0),
        ("loft", |_| super::loft::made().0),
        ("scale", |_| super::scale::made().0),
        ("align", |_| super::align::made().0),
        ("plates", |_| plates()),
    ];
    let mut kinds = std::collections::HashSet::new();
    for (_, make) in fixtures {
        let mut regen = varde_regen::Regenerator::default();
        let mut plates = make(&mut regen);
        let features: Vec<(FeatureId, FeatureKind)> = (plates.doc.editor.document().features())
            .iter()
            .filter(|feature| !matches!(feature.kind, FeatureKind::Sketch { .. }))
            .map(|feature| (feature.id, feature.kind.clone()))
            .collect();
        for (id, kind) in features {
            ok_writes_nothing(&mut plates, &mut regen, id);
            kinds.insert(std::mem::discriminant(&kind));
        }
    }
    // Every kind there is but a sketch: extrude, revolve, combine, move,
    // mirror, pattern, scale, align, split, chamfer, fillet, shell,
    // offset face, draft, sweep and loft.
    assert_eq!(kinds.len(), 16);
}

/// Keeping one side of a split then both again, or an extrude's new
/// body turned into a join then back, within one edit, keeps the new
/// body's id: OK writes nothing, so what names that body (a sketch on
/// its face) still finds it.
#[test]
fn a_new_body_dropped_and_made_again_in_one_edit_keeps_its_id() {
    use varde_document::{Keep, Operation};
    use varde_view::{ExtrudeLook, OperationKind};

    varde_regen::testing::split_by_booleans();
    let (mut split, id) = super::split::made();
    let revision = split.doc.editor.revision();
    split.doc.look(Look::EditFeature(id));
    split.motion(MotionLook::Keep(Keep::Front));
    split.answer();
    split.motion(MotionLook::Keep(Keep::Both));
    split.answer();
    split.doc.update(Edit::CommitMotion);
    assert!(split.doc.motion.is_none());
    assert_eq!(split.doc.editor.revision(), revision);

    let mut discs = plates();
    let disc = (discs.doc.editor.document().features().iter())
        .rev()
        .find(|feature| matches!(feature.kind, FeatureKind::Extrude(_)))
        .unwrap();
    let (disc, made) = (disc.id, disc.kind.new_body());
    assert!(made.is_some());
    let revision = discs.doc.editor.revision();
    discs.doc.look(Look::EditFeature(disc));
    discs
        .doc
        .look(Look::Extrude(ExtrudeLook::Operation(OperationKind::Join)));
    discs.answer();
    discs.doc.look(Look::Extrude(ExtrudeLook::Operation(
        OperationKind::NewBody,
    )));
    discs.answer();
    discs.doc.update(Edit::CommitExtrude);
    assert!(discs.doc.extrude.is_none());
    assert_eq!(discs.doc.editor.revision(), revision);
    let FeatureKind::Extrude(extrude) = &discs.doc.editor.document().feature(disc).unwrap().kind
    else {
        panic!("an extrude");
    };
    assert_eq!(extrude.operation, Operation::NewBody(made.unwrap()));
}
