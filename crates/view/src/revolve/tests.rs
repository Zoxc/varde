use std::sync::Arc;

use varde_document::{OriginPlane, Plane};
use varde_sketch::Profiles;

use super::*;
use crate::testing;

/// A sketch of a rectangle from (2, -4) to (6, 4) and a construction line
/// from the origin to (0, 4), the line's id, and the profiles.
fn lathe() -> (Sketch, Id, Arc<Profiles>) {
    let mut sketch = Sketch::default();
    let corners = [(2.0, -4.0), (6.0, -4.0), (6.0, 4.0), (2.0, 4.0)]
        .map(|(x, y)| testing::point(&mut sketch, x, y));
    for k in 0..4 {
        testing::line(&mut sketch, corners[k], corners[(k + 1) % 4]);
    }
    let [start, end] = [(0.0, 0.0), (0.0, 4.0)].map(|(x, y)| testing::point(&mut sketch, x, y));
    let line = Curve::Line { start, end };
    let construction = sketch.add_curve(line, true).unwrap();
    let profiles = Arc::new(sketch.profiles().unwrap());
    (sketch, construction, profiles)
}

fn state_of<'a>(
    sketch: &'a Sketch,
    profiles: &'a Arc<Profiles>,
    picked: &'a BTreeSet<usize>,
) -> RevolveState<'a> {
    let field = |value| TypedField {
        params: crate::ParamsIn::NONE,
        text: "",
        error: None,
        value,
    };
    let feature = {
        let mut editor = varde_document::Editor::new(Default::default());
        let plane = Plane::Origin(OriginPlane::XY);
        editor.apply(editor.document().add_sketch(plane)).unwrap();
        editor.document().features()[0].id
    };
    RevolveState {
        editing: None,
        candidates: vec![Candidate {
            feature,
            placement: OriginPlane::XY.placement(),
            sketch,
            profiles,
        }],
        source: Some(feature),
        picked,
        missing: 0,
        axis: None,
        axis_missing: false,
        edge_ends: None,
        edge_body: None,
        index: crate::pick::empty_index(),
        resolution: varde_kernel::Tolerance::DEFAULT.resolution(),
        picking: RevolvePick::Regions,
        extent: TurnKind::Full,
        fields: [field(Some(1.0)), field(Some(0.5))],
        flip: false,
        operation: OperationKind::NewBody,
        targets: Vec::new(),
        error: None,
        show_error: None,
        refused: None,
        uncut: None,
        checking: false,
        ready: false,
        accept: false,
        editable: true,
        hover: None,
        grabbed: None,
    }
}

/// Each text of `state`'s panel and where it's laid out.
fn texts_of(state: &RevolveState<'_>) -> Vec<crate::probe::Shown> {
    crate::testing::Laid::new(panel(state), iced::Size::new(400.0, 800.0)).texts()
}

fn found<'s>(shown: &'s [crate::probe::Shown], text: &str) -> &'s crate::probe::Shown {
    shown
        .iter()
        .find(|shown| shown.text == text)
        .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
}

fn has(shown: &[crate::probe::Shown], text: &str) -> bool {
    shown.iter().any(|shown| shown.text == text)
}

#[test]
fn the_panel_names_the_profile_and_the_axis() {
    let (sketch, construction, profiles) = lathe();
    let none = BTreeSet::new();
    let mut state = state_of(&sketch, &profiles, &none);
    let shown = texts_of(&state);
    for text in [
        "New revolve",
        "Profile",
        "Click regions",
        "Axis",
        "Click a line, axis or edge",
    ] {
        found(&shown, text);
    }
    // A full turn has no angles, nor Flip.
    assert!(!has(&shown, "Angle"));
    assert!(!has(&shown, "Flip"));
    found(&shown, "Full 360°");

    let picked = BTreeSet::from([0]);
    state.picked = &picked;
    state.axis = Some(AxisLine::Curve(construction));
    let shown = texts_of(&state);
    found(&shown, "Region 1");
    found(&shown, "Line 5");
    // Picking regions, the profile asks for more; the axis, picked, doesn't.
    found(&shown, "Click regions");
    assert!(!has(&shown, "Click a line, axis or edge"));
    state.picking = RevolvePick::Axis;
    assert!(!has(&texts_of(&state), "Click regions"));
    state.picking = RevolvePick::Regions;
    state.axis = Some(AxisLine::SketchY);
    found(&texts_of(&state), "Y axis");
    state.axis = Some(AxisLine::SketchX);
    found(&texts_of(&state), "X axis");
}

#[test]
fn each_turn_shows_its_angles() {
    let (sketch, _, profiles) = lathe();
    let picked = BTreeSet::from([0]);
    let mut state = state_of(&sketch, &profiles, &picked);
    state.extent = TurnKind::OneSide;
    let shown = texts_of(&state);
    found(&shown, "Angle");
    found(&shown, "Flip");
    state.extent = TurnKind::Symmetric;
    let shown = texts_of(&state);
    found(&shown, "Angle");
    assert!(!has(&shown, "Flip"));
    state.extent = TurnKind::TwoSides;
    let shown = texts_of(&state);
    found(&shown, "Side 1");
    found(&shown, "Side 2");
    found(&shown, "Flip");
}

#[test]
fn the_panel_s_errors_read_as_sentences() {
    let (sketch, _, profiles) = lathe();
    let picked = BTreeSet::from([0]);
    let mut state = state_of(&sketch, &profiles, &picked);
    state.extent = TurnKind::OneSide;
    let error = varde_expr::Error {
        kind: varde_expr::ErrorKind::Empty,
        span: varde_expr::Span::new(0, 0),
    };
    state.fields[0].error = Some(&error);
    state.error = Some("its outline crosses the axis");
    let shown = texts_of(&state);
    let label = found(&shown, "Angle");
    let refused = found(&shown, "Enter a value");
    // Under its field, which is under its label.
    assert_eq!(refused.bounds.x, label.bounds.x);
    assert!(refused.bounds.y > label.bounds.y + 28.0, "{refused:?}");
    found(&shown, "Revolve fails");
    found(&shown, "Its outline crosses the axis");
    // The revolve's own check refusing it shows in place of the
    // preview's error.
    state.refused = Some(RevolveError::Turn);
    let shown = texts_of(&state);
    found(&shown, "Its two sides come to over a turn");
    assert!(!has(&shown, "Its outline crosses the axis"));

    // What an edited revolve lost: its regions listed, its axis said.
    state.missing = 2;
    state.axis_missing = true;
    let shown = texts_of(&state);
    let rows = shown.iter().filter(|shown| shown.text == "Missing region");
    assert_eq!(rows.count(), 2);
    assert!(!has(&shown, "2 regions weren't found"));
    found(&shown, "The axis line wasn't found");
}

#[test]
fn an_axis_line_is_from_its_start_to_its_end() {
    let (sketch, construction, _) = lathe();
    let line = axis_line(&sketch, AxisLine::Curve(construction));
    assert_eq!(line, Some((DVec2::ZERO, DVec2::new(0.0, 4.0))));
    assert_eq!(
        axis_line(&sketch, AxisLine::SketchX),
        Some((DVec2::ZERO, DVec2::X))
    );
    // A circle isn't one, nor an id the sketch lacks.
    assert_eq!(axis_of(&sketch, Id::X_AXIS), Some(AxisLine::SketchX));
    assert_eq!(
        axis_of(&sketch, construction),
        Some(AxisLine::Curve(construction))
    );
    assert_eq!(axis_of(&sketch, Id::ORIGIN), None);
    assert_eq!(axis_line(&sketch, AxisLine::Curve(Id::ORIGIN)), None);
}
