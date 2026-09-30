use varde_document::OriginPlane;
use varde_sketch::{Curve, Sketch};

use super::*;

/// A sketch of a 4 × 2 rectangle from the origin with a unit square hole
/// from (1, 0.5), and its profiles.
fn plate() -> Arc<Profiles> {
    let mut sketch = Sketch::default();
    let mut rectangle = |corners: [(f64, f64); 4]| {
        let ids = corners.map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        for (k, &start) in ids.iter().enumerate() {
            let end = ids[(k + 1) % 4];
            sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        }
    };
    rectangle([(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)]);
    rectangle([(1.0, 0.5), (2.0, 0.5), (2.0, 1.5), (1.0, 1.5)]);
    Arc::new(sketch.profiles().unwrap())
}

fn state_of<'a>(profiles: &'a Arc<Profiles>, picked: &'a BTreeSet<usize>) -> ExtrudeState<'a> {
    let field = |value| DistanceField {
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
    ExtrudeState {
        editing: None,
        candidates: vec![Candidate {
            feature,
            plane: Plane::Origin(OriginPlane::XZ),
            profiles,
        }],
        source: Some(feature),
        picked,
        missing: 0,
        extent: ExtentKind::OneSide,
        fields: [field(Some(10.0)), field(Some(3.0))],
        flip: false,
        operation: OperationKind::NewBody,
        targets: Vec::new(),
        grabbed: None,
        error: None,
        refused: None,
        checking: false,
        ready: true,
        editable: true,
        units: LengthUnit::Mm,
    }
}

#[test]
fn the_centre_weighs_the_loops_by_their_areas() {
    let profiles = plate();
    let outer = profiles
        .regions
        .iter()
        .find(|region| region.holes.len() == 1)
        .unwrap();
    let centre = centroid(std::iter::once(outer)).unwrap();
    // The 8 square rectangle about (2, 1) less the unit square about
    // (1.5, 1): (16 - 1.5) / 7 across.
    assert!(
        (centre - DVec2::new(14.5 / 7.0, 1.0)).length() < 1e-9,
        "{centre}"
    );
    // With the hole back, it's the rectangle's.
    let all = centroid(profiles.regions.iter()).unwrap();
    assert!((all - DVec2::new(2.0, 1.0)).length() < 1e-9, "{all}");
    assert_eq!(centroid(std::iter::empty()), None);
}

#[test]
fn the_handle_has_a_knob_per_distance_on_the_plane_s_normal() {
    let profiles = plate();
    let picked: BTreeSet<usize> = (0..profiles.regions.len()).collect();
    let mut state = state_of(&profiles, &picked);
    let handle = state.handle().unwrap();
    // On XZ, whose normal is -Y.
    assert!((handle.origin - DVec3::new(2.0, 0.0, 1.0)).length() < 1e-9);
    assert_eq!(handle.normal, DVec3::new(0.0, -1.0, 0.0));
    assert_eq!(handle.knobs, [(Distance::First, 10.0)]);
    state.flip = true;
    assert_eq!(state.handle().unwrap().knobs, [(Distance::First, -10.0)]);
    state.extent = ExtentKind::Symmetric;
    assert_eq!(state.handle().unwrap().knobs, [(Distance::First, 5.0)]);
    state.extent = ExtentKind::TwoSides;
    assert_eq!(
        state.handle().unwrap().knobs,
        [(Distance::First, -10.0), (Distance::Second, 3.0)]
    );
    state.extent = ExtentKind::ThroughAll;
    assert_eq!(state.handle(), None);
    // Nothing picked, no handle.
    let none = BTreeSet::new();
    assert_eq!(state_of(&profiles, &none).handle(), None);
    // Its placement's x axis is the normal.
    let placement = handle.placement();
    assert_eq!(
        placement.to_world(DVec2::new(2.0, 0.0)),
        handle.origin + handle.normal * 2.0
    );
}

#[test]
fn drags_snap_to_round_steps_of_the_units() {
    // 6 pixels of 0.1 mm: 0.6 mm, up to 1 mm.
    assert_eq!(snap_step(0.1, LengthUnit::Mm), Some(1.0));
    assert_eq!(snap_step(0.03, LengthUnit::Mm), Some(0.2));
    assert_eq!(snap_step(0.5, LengthUnit::Mm), Some(5.0));
    // In inches: 0.6 mm is 0.0236 in, up to 0.05 in.
    let step = snap_step(0.1, LengthUnit::In).unwrap();
    assert!((step - 0.05 * 25.4).abs() < 1e-12, "{step}");
    assert_eq!(snap_step(0.0, LengthUnit::Mm), None);
    assert_eq!(snap_step(f64::NAN, LengthUnit::Mm), None);
    assert_eq!(snap_step(f64::INFINITY, LengthUnit::Mm), None);
}

#[test]
fn the_panel_builds_with_why_ok_waits() {
    let profiles = plate();
    let picked = BTreeSet::from([0]);
    let mut state = state_of(&profiles, &picked);
    state.ready = false;
    state.checking = true;
    let _ = panel(&state);
    state.error = Some("the preview failed");
    let _ = panel(&state);
    state.refused = Some(ExtrudeError::Length);
    let _ = panel(&state);
}
