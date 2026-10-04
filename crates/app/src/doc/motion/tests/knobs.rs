//! The handles of the operations with a value to drag
//! ([`super::super::knobs`]): where each knob stands on the example's
//! plate (60 × 40 × 10 about the Z axis from z 0 up), and a knob dragged
//! typed into its field.

use std::f64::consts::{FRAC_1_SQRT_2, SQRT_2};

use glam::DVec3;
use varde_document::{ChamferSize, Document, FeatureKind};
use varde_view::{
    ChamferType, KnobPath, KnobRadius, KnobScale, KnobTone, Look, MotionField, MotionLook, OpKnob,
};

use super::chamfer::{FRONT, click_edge, plate};
use super::face_session::{click, face_pick, held};
use super::{Plates, near};

/// The knobs the view has.
fn knobs(plates: &Plates) -> Vec<OpKnob> {
    plates
        .doc
        .motion_state()
        .map_or_else(Vec::new, |state| state.knobs)
}

/// The line a knob drags along: its origin and direction.
fn line(knob: &OpKnob) -> (DVec3, DVec3) {
    match knob.path {
        KnobPath::Line { origin, along } => (origin, along),
        KnobPath::Arc { .. } => panic!("a line: {knob:?}"),
    }
}

/// What the last request previews.
fn drafted(plates: &Plates) -> FeatureKind {
    plates.last_draft().expect("a draft").1
}

fn drag(plates: &mut Plates, knob: usize, value: f64) {
    plates.motion(MotionLook::DragKnob { knob, value });
}

/// A chamfer of the plate's top front edge: Equal, its knob from the
/// edge along the bisector into the plate, half the bisector's sum a
/// millimetre of distance, so on the chamfer's middle; Two distances, a
/// knob along each face. Dragged, the distance is typed; nothing at or
/// below zero.
#[test]
fn a_chamfer_s_knobs_stand_on_its_first_edge() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartChamfer);
    assert!(knobs(&plates).is_empty(), "no edge yet");
    click_edge(&mut plates, plate, FRONT);
    let [knob] = knobs(&plates)[..] else {
        panic!("{:?}", knobs(&plates));
    };
    assert_eq!(knob.field, MotionField::ChamferDistance);
    assert_eq!(knob.tone, KnobTone::Modify);
    let (origin, along) = line(&knob);
    assert!(near(origin, DVec3::new(0.0, -20.0, 10.0)), "{origin}");
    assert!(
        near(along, DVec3::new(0.0, FRAC_1_SQRT_2, -FRAC_1_SQRT_2)),
        "{along}"
    );
    let KnobScale::Times(times) = knob.scale else {
        panic!("{knob:?}");
    };
    assert!((times - FRAC_1_SQRT_2).abs() < 1e-9, "{times}");
    drag(&mut plates, 0, 3.0);
    let FeatureKind::Chamfer(chamfer) = drafted(&plates) else {
        panic!("a chamfer");
    };
    assert!(matches!(chamfer.distances, ChamferSize::Equal(d) if d.value == 3.0));
    assert_eq!(knobs(&plates)[0].value, 3.0);
    drag(&mut plates, 0, 0.0);
    drag(&mut plates, 0, -2.0);
    assert_eq!(knobs(&plates)[0].value, 3.0);
    // The preview answered, the knob stays where the edge was.
    plates.answer();
    assert!(near(line(&knobs(&plates)[0]).0, origin));

    plates.motion(MotionLook::ChamferType(ChamferType::Two));
    let [first, second] = knobs(&plates)[..] else {
        panic!("{:?}", knobs(&plates));
    };
    assert_eq!(first.field, MotionField::ChamferDistance);
    assert_eq!(second.field, MotionField::ChamferSecond);
    let ways = [line(&first).1, line(&second).1];
    assert!(ways.iter().any(|&way| near(way, DVec3::Y)), "{ways:?}");
    assert!(ways.iter().any(|&way| near(way, -DVec3::Z)), "{ways:?}");
    drag(&mut plates, 1, 2.0);
    let FeatureKind::Chamfer(chamfer) = drafted(&plates) else {
        panic!("a chamfer");
    };
    assert!(matches!(chamfer.distances, ChamferSize::Two(_, b) if b.value == 2.0));
}

/// A fillet of the same edge: its knob along the bisector, on the
/// round's middle, √2 − 1 of the radius from the edge for faces square
/// to each other.
#[test]
fn a_fillet_s_knob_is_on_the_round_s_middle() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartFillet);
    click_edge(&mut plates, plate, FRONT);
    let [knob] = knobs(&plates)[..] else {
        panic!("{:?}", knobs(&plates));
    };
    assert_eq!(knob.field, MotionField::Radius);
    let KnobScale::Times(times) = knob.scale else {
        panic!("{knob:?}");
    };
    assert!((times - (SQRT_2 - 1.0)).abs() < 1e-9, "{times}");
    drag(&mut plates, 0, 4.0);
    let FeatureKind::Fillet(fillet) = drafted(&plates) else {
        panic!("a fillet");
    };
    assert_eq!(fillet.radius.value, 4.0);
}

/// A shell with the plate's top removed: its knob from the top down into
/// the plate at the thickness, out of it for Outward walls.
#[test]
fn a_shell_s_knob_goes_into_the_body_from_its_face_removed() {
    let mut plates = held(Document::example());
    let plate = plates.bodies[0];
    plates.doc.look(Look::StartShell);
    let top = face_pick(&plates, plate, DVec3::Z, 10.0, DVec3::new(20.0, 15.0, 10.0));
    click(&mut plates, top);
    let [knob] = knobs(&plates)[..] else {
        panic!("{:?}", knobs(&plates));
    };
    assert_eq!(knob.field, MotionField::Thickness);
    let (origin, along) = line(&knob);
    assert!(near(origin, DVec3::new(20.0, 15.0, 10.0)), "{origin}");
    assert!(near(along, -DVec3::Z));
    drag(&mut plates, 0, 2.5);
    let FeatureKind::Shell(shell) = drafted(&plates) else {
        panic!("a shell");
    };
    assert_eq!(shell.thickness.value, 2.5);
    plates.motion(MotionLook::ShellDirection(
        varde_view::ShellDirection::Outward,
    ));
    assert!(near(line(&knobs(&plates)[0]).1, DVec3::Z));
}

/// A draft of the plate's front about the XY plane: its knob turns about
/// the front's bottom edge (where it meets the neutral plane), from up,
/// so the front leans in at the top; dragged, the angle is typed, not a
/// quarter turn or more.
#[test]
fn a_draft_s_knob_turns_its_face_about_its_hinge() {
    let mut plates = held(Document::example());
    let plate = plates.bodies[0];
    plates.doc.look(Look::StartDraft);
    let front = face_pick(
        &plates,
        plate,
        -DVec3::Y,
        20.0,
        DVec3::new(10.0, -20.0, 6.0),
    );
    click(&mut plates, front);
    let [knob] = knobs(&plates)[..] else {
        panic!("{:?}", knobs(&plates));
    };
    assert_eq!(knob.field, MotionField::Angle);
    let KnobPath::Arc {
        centre,
        axis,
        radial,
        radius,
    } = knob.path
    else {
        panic!("an arc: {knob:?}");
    };
    assert!(near(centre, DVec3::new(10.0, -20.0, 0.0)), "{centre}");
    assert!(near(radial, DVec3::Z), "{radial}");
    assert_eq!(radius, KnobRadius::World(6.0));
    // Turning on moves the knob into the plate, +y.
    assert!(near(axis.cross(radial), DVec3::Y), "{axis}");
    drag(&mut plates, 0, 5f64.to_radians());
    let FeatureKind::FaceDraft(draft) = drafted(&plates) else {
        panic!("a draft");
    };
    assert!((draft.angle.value - 5f64.to_radians()).abs() < 1e-12);
    drag(&mut plates, 0, 90f64.to_radians());
    assert!((knobs(&plates)[0].value - 5f64.to_radians()).abs() < 1e-12);
}

/// A scale of the plate about the origin, its middle: a slider of 100
/// pixels a factor of 1; dragged, the factor is typed.
#[test]
fn a_scale_s_knob_is_a_slider_of_its_factor() {
    let (mut plates, _) = plate();
    plates.doc.look(Look::StartScale);
    let [knob] = knobs(&plates)[..] else {
        panic!("{:?}", knobs(&plates));
    };
    assert_eq!(knob.field, MotionField::Factor);
    assert_eq!(knob.scale, KnobScale::Pixels(100.0));
    drag(&mut plates, 0, 1.5);
    let FeatureKind::Scale(scale) = drafted(&plates) else {
        panic!("a scale");
    };
    assert!(matches!(scale.factor, varde_document::ScaleFactor::Uniform(f) if f.value == 1.5));
}

/// An offset face's knob dragged through zero: its distance and side,
/// as its old handle's.
#[test]
fn an_offset_face_s_knob_sets_its_distance_and_side() {
    let mut plates = held(Document::example());
    let plate = plates.bodies[0];
    plates.doc.look(Look::StartOffsetFace);
    plates.answer();
    let top = face_pick(&plates, plate, DVec3::Z, 10.0, DVec3::new(20.0, 15.0, 10.0));
    click(&mut plates, top);
    plates.answer();
    let [knob] = knobs(&plates)[..] else {
        panic!("{:?}", knobs(&plates));
    };
    assert_eq!(knob.field, MotionField::Distance);
    drag(&mut plates, 0, -2.0);
    let FeatureKind::OffsetFace(offset) = drafted(&plates) else {
        panic!("an offset");
    };
    assert!(offset.inward && offset.distance.value == 2.0);
    assert_eq!(knobs(&plates)[0].value, -2.0);
    // Zero is no distance: nothing changes.
    drag(&mut plates, 0, 0.0);
    assert_eq!(knobs(&plates)[0].value, -2.0);
}

/// No knobs in a document that can't be changed, nor for an edited
/// chamfer, whose edge the model shown has chamfered away.
#[test]
fn no_knobs_read_only_or_editing() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartChamfer);
    click_edge(&mut plates, plate, FRONT);
    assert_eq!(knobs(&plates).len(), 1);
    plates.doc.read_only = Some("read only".to_owned());
    assert!(knobs(&plates).is_empty());
    plates.doc.read_only = None;
    plates.answer();
    plates.doc.update(varde_view::Edit::AcceptError);
    plates.answer();
    let (id, _) = plates.last_feature();
    plates.doc.look(Look::EditFeature(id));
    assert!(plates.doc.motion.is_some());
    assert!(knobs(&plates).is_empty());
}
