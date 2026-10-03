use std::sync::Arc;

use varde_document::{OriginPlane, Plane};
use varde_sketch::Profiles;
use varde_sketch::{Curve, Sketch};

use super::*;
use crate::operation_panel::PANEL_WIDTH;
use crate::operation_panel::joined_into;

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
    let field = |value| TypedField {
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
            placement: OriginPlane::XZ.placement(),
            sketch: Box::leak(Box::default()),
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
        show_error: None,
        refused: None,
        held: None,
        checking: false,
        ready: true,
        accept: false,
        editable: true,
        units: LengthUnit::Mm,
        hover: None,
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

#[test]
fn a_snap_step_is_the_decimal_it_names() {
    // 6 pixels of 0.01 mm in metres, 6e-5 m: up to 1e-4 m, a tenth of a
    // millimetre, not the ulp under it that libm's `pow(10, -5)` gives.
    assert_eq!(snap_step(0.01, LengthUnit::M), Some(1e-4 * 1000.0));
    // Over 60 decades either way, the step is 1, 2 or 5 times a power of
    // ten, the nearest double to it, and the least such at or above what
    // 6 pixels come to; exactly at a step, that step.
    for k in -300..=300 {
        for (m, want) in [
            (1.0, "1"),
            (1.5, "2"),
            (2.0, "2"),
            (3.0, "5"),
            (5.0, "5"),
            (7.0, "10"),
        ] {
            let decade: f64 = format!("1e{k}").parse().unwrap();
            let least = m * decade;
            let step = snap_step(least / 6.0, LengthUnit::Mm).unwrap();
            let want: f64 = format!("{want}e{k}").parse().unwrap();
            // Divided by 6 and back, `least` can come out an ulp over.
            let least_back = least / 6.0 * 6.0;
            if least_back <= want {
                assert_eq!(step, want, "{m}e{k}");
            }
        }
    }
    // Where the step is past the largest double, none.
    assert_eq!(snap_step(f64::MAX / 6.0, LengthUnit::Mm), None);
    // 6 pixels come to 5.6e305 ft, up to 1e306 ft: 3e308 mm.
    assert_eq!(snap_step(2.8e307, LengthUnit::Ft), None);
}

#[test]
fn a_long_name_without_spaces_breaks_inside_the_panel() {
    use crate::testing::Laid;
    let profiles = plate();
    let picked = BTreeSet::from([0]);
    let long = "x".repeat(200);
    let error = format!("Feature 3: {long} doesn't touch it");
    let mut state = state_of(&profiles, &picked);
    state.operation = OperationKind::Cut;
    let size = iced::Size::new(400, 600);
    let max = iced::Size::new(size.width as f32, size.height as f32);
    let target = |name| BodyTarget {
        body: BodyId::NEW,
        name,
        included: true,
        holder: None,
    };
    let short = Laid::new(
        panel(&ExtrudeState {
            targets: vec![target("Body 1")],
            ..state.clone()
        }),
        max,
    )
    .pixels(size);
    state.editing = Some(&long);
    state.targets = vec![target(&long), target("Body 2")];
    state.error = Some(&error);
    let mut laid = Laid::new(panel(&state), max);
    assert_eq!(laid.node.size().width, PANEL_WIDTH);
    let shown = laid.texts();
    let find = |text: &str| {
        let mut found = shown.iter().filter(|shown| shown.text == text);
        found.next_back().unwrap_or_else(|| panic!("no {text:?}"))
    };
    let line = find("Body 2").bounds.height;
    // The message breaks into lines, each line inside the panel: 200
    // glyphs at 12 px take well over four widths of it.
    let text = find(&error);
    assert!(text.bounds.height >= 4.0 * line, "{text:?}");
    assert!(text.bounds.x + text.bounds.width <= PANEL_WIDTH, "{text:?}");
    // Nothing is drawn right of the panel and its shadow that the short
    // name doesn't draw there: the title and the body's row, one line
    // each as the picked rows are, are clipped.
    let pixels = laid.pixels(size);
    let shadow = 2.0 * crate::theme::CARD_SHADOW.blur_radius;
    let columns = (PANEL_WIDTH + shadow) as usize..size.width as usize;
    for y in 0..size.height as usize {
        for x in columns.clone() {
            let at = (y * size.width as usize + x) * 4;
            assert_eq!(pixels[at..at + 4], short[at..at + 4], "at ({x}, {y})");
        }
    }
}

/// Each text of `state`'s panel and where it's laid out.
fn texts_of(state: &ExtrudeState<'_>) -> Vec<crate::probe::Shown> {
    crate::testing::Laid::new(panel(state), iced::Size::new(400.0, 800.0)).texts()
}

fn found<'s>(shown: &'s [crate::probe::Shown], text: &str) -> &'s crate::probe::Shown {
    shown
        .iter()
        .find(|shown| shown.text == text)
        .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
}

#[test]
fn flip_is_an_icon_toggle_and_a_body_a_row_with_a_box() {
    let profiles = plate();
    let picked = BTreeSet::from([0]);
    let mut state = state_of(&profiles, &picked);
    state.operation = OperationKind::Cut;
    state.targets = vec![BodyTarget {
        body: BodyId::NEW,
        name: "Body 1",
        included: true,
        holder: None,
    }];
    let shown = texts_of(&state);
    // From the fields' edge, where the labels start: Flip's label right
    // of its icon's square button, the body's name right of its box and
    // its icon, in a row in a box.
    let edge = found(&shown, "Bodies").bounds.x;
    let (flip, body) = (found(&shown, "Flip"), found(&shown, "Body 1"));
    assert!(flip.bounds.x >= edge + 28.0, "{flip:?}");
    assert!(body.bounds.x >= edge + 15.0 + 16.0 + 16.0, "{body:?}");
}

#[test]
fn a_field_s_error_lines_up_under_its_input() {
    let profiles = plate();
    let picked = BTreeSet::from([0]);
    let mut state = state_of(&profiles, &picked);
    let error = varde_expr::Error {
        kind: varde_expr::ErrorKind::Empty,
        span: varde_expr::Span::new(0, 0),
    };
    state.fields[0].error = Some(&error);
    let shown = texts_of(&state);
    // The label over the field, the error under it, all from its left.
    let label = (shown.iter())
        .find(|shown| shown.text == "Distance")
        .unwrap();
    let error = found(&shown, "Enter a value");
    assert_eq!(error.bounds.x, label.bounds.x);
    assert!(error.bounds.y >= label.bounds.y + label.bounds.height + 28.0);
}

#[test]
fn the_panel_s_errors_read_as_sentences() {
    let profiles = plate();
    let picked = BTreeSet::from([0]);
    let mut state = state_of(&profiles, &picked);
    let error = varde_expr::Error {
        kind: varde_expr::ErrorKind::UnknownUnit("parsecs".to_owned()),
        span: varde_expr::Span::new(3, 10),
    };
    state.fields[0].error = Some(&error);
    state.error = Some("joining it to Body 1 leaves no clean solid");
    let shown = texts_of(&state);
    found(&shown, "Unknown unit 'parsecs'");
    found(&shown, "Joining it to Body 1 leaves no clean solid");

    // The extrude's own check refusing it shows in place of the preview's
    // error, as a sentence too.
    state.refused = Some(ExtrudeError::ThroughAll);
    let shown = texts_of(&state);
    found(&shown, "Only a cut can go through all");
}

#[test]
fn a_join_ticked_for_two_bodies_says_which_it_merges_into() {
    let profiles = plate();
    let picked = BTreeSet::from([0]);
    let mut state = state_of(&profiles, &picked);
    state.operation = OperationKind::Join;
    let target = |name, included| BodyTarget {
        body: BodyId::NEW,
        name,
        included,
        holder: None,
    };
    state.targets = vec![
        target("Body 1", false),
        target("Body 2", true),
        target("Body 3", true),
    ];
    assert_eq!(joined_into(state.operation, &state.targets), Some("Body 2"));
    found(&texts_of(&state), "Joined into Body 2");

    // One ticked merges nothing, nor does a cut.
    state.targets[2].included = false;
    assert_eq!(joined_into(state.operation, &state.targets), None);
    state.targets[2].included = true;
    state.operation = OperationKind::Cut;
    assert_eq!(joined_into(state.operation, &state.targets), None);
    let shown = texts_of(&state);
    assert!(!shown.iter().any(|shown| shown.text.starts_with("Joined")));

    // A body merged away before, just put back, isn't merged: it's in its
    // holder already, as its row says.
    state.operation = OperationKind::Join;
    state.targets[0].included = false;
    state.targets[2].holder = Some("Body 1");
    assert_eq!(joined_into(state.operation, &state.targets), None);
    let shown = texts_of(&state);
    assert!(!shown.iter().any(|shown| shown.text.starts_with("Joined")));
    found(&shown, "in Body 1");
}
