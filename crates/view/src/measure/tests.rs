use std::f64::consts::PI;

use varde_regen::Gap;

use super::*;
use crate::testing::Laid;

/// The labels and texts of `values`.
fn shown(values: &[Value]) -> Vec<(&str, &str)> {
    (values.iter())
        .map(|value| (value.label, value.shown.as_str()))
        .collect()
}

/// The labels and copied texts of `values`.
fn copied(values: &[Value]) -> Vec<(&str, &str)> {
    (values.iter())
        .map(|value| (value.label, value.copied.as_str()))
        .collect()
}

#[test]
fn a_point_shows_its_coordinates_in_the_design_s_units() {
    let point = Measure::Point([12.7, -25.4, 1.0 / 3.0]);
    assert_eq!(
        shown(&values(&point, LengthUnit::Mm)),
        [("X", "12.7 mm"), ("Y", "-25.4 mm"), ("Z", "0.333 mm")]
    );
    assert_eq!(
        shown(&values(&point, LengthUnit::In)),
        [("X", "0.5 in"), ("Y", "-1 in"), ("Z", "0.0131 in")]
    );
    // Copied at full precision, with the unit.
    assert_eq!(
        copied(&values(&point, LengthUnit::Mm))[2],
        ("Z", "0.3333333333333333 mm")
    );
}

#[test]
fn an_edge_shows_its_length_and_its_shape() {
    let line = Measure::Edge {
        length: 10.0,
        closed: false,
        shape: EdgeForm::Line {
            from: [0.0, 0.0, 0.0],
            to: [0.0, 0.0, 10.0],
        },
    };
    assert_eq!(
        shown(&values(&line, LengthUnit::Mm)),
        [("Length", "10 mm"), ("Direction", "0, 0, 1")]
    );
    let circle = Measure::Edge {
        length: 16.0 * PI,
        closed: true,
        shape: EdgeForm::Circle {
            centre: [0.0, 0.0, 10.0],
            axis: [0.0, 0.0, 1.0],
            radius: 8.0,
        },
    };
    assert_eq!(
        shown(&values(&circle, LengthUnit::Mm)),
        [
            ("Length", "50.265 mm"),
            ("Radius", "8 mm"),
            ("Diameter", "16 mm"),
            ("Centre", "0, 0, 10 mm"),
        ]
    );
    assert_eq!(
        copied(&values(&circle, LengthUnit::Mm))[3],
        ("Centre", "0, 0, 10 mm")
    );
    assert_eq!(edge_kind(&EdgeForm::Other), "Curved edge");
}

#[test]
fn a_face_shows_its_area_and_its_form() {
    let plane = Measure::Face {
        area: 645.16,
        summary: Summary::Plane {
            n: [0.0, 0.0, 1.0],
            d: 10.0,
        },
        half_angle: None,
        rectangle: None,
    };
    assert_eq!(
        shown(&values(&plane, LengthUnit::In)),
        [("Area", "1 in²"), ("Normal", "0, 0, 1")]
    );
    let cone = Measure::Face {
        area: 100.0,
        summary: Summary::Cone {
            apex: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            cos: 0.5,
            sin: 0.75f64.sqrt(),
        },
        half_angle: Some(PI / 6.0),
        rectangle: None,
    };
    assert_eq!(
        shown(&values(&cone, LengthUnit::Mm)),
        [("Area", "100 mm²"), ("Half-angle", "30°")]
    );
    let hole = Summary::Cylinder {
        point: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        radius: 8.0,
    };
    let wall = Measure::Face {
        area: 160.0 * PI,
        summary: hole,
        half_angle: None,
        rectangle: None,
    };
    assert_eq!(
        shown(&values(&wall, LengthUnit::Mm)),
        [
            ("Area", "502.655 mm²"),
            ("Radius", "8 mm"),
            ("Diameter", "16 mm")
        ]
    );
    assert_eq!(face_kind(&hole), "Cylindrical face");
}

#[test]
fn a_body_shows_its_volume_area_centre_and_box() {
    let body = Measure::Body {
        volume: 24000.0,
        area: 7600.0,
        centre: Some([0.0, 0.0, 5.0]),
        bounds: Some([[-30.0, -20.0, 0.0], [30.0, 20.0, 10.0]]),
    };
    assert_eq!(
        shown(&values(&body, LengthUnit::Mm)),
        [
            ("Volume", "24000 mm³"),
            ("Area", "7600 mm²"),
            ("Centre", "0, 0, 5 mm"),
            ("Box from", "-30, -20, 0 mm"),
            ("Box to", "30, 20, 10 mm"),
            ("Box size", "60, 40, 10 mm"),
        ]
    );
    assert_eq!(
        shown(&values(&body, LengthUnit::In))[0],
        ("Volume", "1.4646 in³")
    );
}

#[test]
fn between_two_picks_the_distance_its_parts_and_the_angle() {
    let between = Between {
        distance: Ok(Gap {
            distance: 10.0,
            points: [[1.0, 2.0, 10.0], [1.0, 2.0, 0.0]],
        }),
        angle: Some(PI),
    };
    assert_eq!(
        shown(&between_values(&between, LengthUnit::Mm)),
        [
            ("Distance", "10 mm"),
            ("ΔX", "0 mm"),
            ("ΔY", "0 mm"),
            ("ΔZ", "-10 mm"),
            ("Angle", "180°"),
        ]
    );
    assert_eq!(
        shown(&between_values(&between, LengthUnit::In))[0],
        ("Distance", "0.3937 in")
    );
    assert_eq!(
        copied(&between_values(&between, LengthUnit::In))[0],
        ("Distance", "0.3937007874015748 in")
    );
    // A distance too complex to measure leaves the angle.
    let failed = Between {
        distance: Err("too complex to measure".to_owned()),
        angle: Some(PI / 2.0),
    };
    assert_eq!(
        shown(&between_values(&failed, LengthUnit::Mm)),
        [("Angle", "90°")]
    );
}

/// The texts the panel of `state` shows, top to bottom.
fn panel_texts(state: &MeasureState<'_>) -> Vec<String> {
    let mut laid = Laid::new(panel(state), iced::Size::new(400.0, 2000.0));
    let mut texts = laid.texts();
    texts.sort_by(|a, b| {
        (a.bounds.y, a.bounds.x)
            .partial_cmp(&(b.bounds.y, b.bounds.x))
            .unwrap()
    });
    texts.into_iter().map(|text| text.text).collect()
}

#[test]
fn the_panel_names_the_picks_and_shows_their_values() {
    let index = crate::pick::tests::plate();
    let face = Measure::Face {
        area: 2400.0,
        summary: Summary::Plane {
            n: [0.0, 0.0, 1.0],
            d: 10.0,
        },
        half_angle: None,
        rectangle: None,
    };
    let mut state = MeasureState {
        picks: [None, None],
        between: None,
        units: LengthUnit::Mm,
        folded: [true; 2],
        index: &index,
        hover: None,
        points: [None; 2],
    };
    let texts = panel_texts(&state);
    assert!(
        texts.contains(&"Click a face, edge, point or body".to_owned()),
        "{texts:?}"
    );
    assert!(texts.contains(&"Close".to_owned()), "{texts:?}");
    assert!(!texts.contains(&"OK".to_owned()), "{texts:?}");

    // One pick: its own values, unfolded.
    state.picks[0] = Some(Picked {
        name: "Face of Body 1".to_owned(),
        outcome: Outcome::Measured(Ok(&face)),
    });
    let texts = panel_texts(&state);
    for text in [
        "Planar face of Body 1",
        "Area",
        "2400 mm²",
        "Normal",
        "0, 0, 1",
    ] {
        assert!(texts.contains(&text.to_owned()), "{text}: {texts:?}");
    }

    // Two: what's between them, theirs folded until unfolded.
    let between = Between {
        distance: Ok(Gap {
            distance: 10.0,
            points: [[0.0, 0.0, 10.0], [0.0, 0.0, 0.0]],
        }),
        angle: Some(PI),
    };
    state.picks[1] = Some(Picked {
        name: "Face of Body 1".to_owned(),
        outcome: Outcome::Measured(Ok(&face)),
    });
    state.between = Some(&between);
    let texts = panel_texts(&state);
    for text in ["Distance", "10 mm", "Angle", "180°"] {
        assert!(texts.contains(&text.to_owned()), "{text}: {texts:?}");
    }
    assert!(!texts.contains(&"2400 mm²".to_owned()), "{texts:?}");
    state.folded = [false, true];
    let texts = panel_texts(&state);
    assert_eq!(
        texts.iter().filter(|t| *t == "2400 mm²").count(),
        1,
        "{texts:?}"
    );

    // One the model doesn't have, and one waiting.
    state.picks[1] = Some(Picked {
        name: "Edge of Body 1".to_owned(),
        outcome: Outcome::Missing("edge not found"),
    });
    state.between = None;
    let texts = panel_texts(&state);
    assert!(texts.contains(&"Edge not found".to_owned()), "{texts:?}");
    state.picks[1] = Some(Picked {
        name: "Edge of Body 1".to_owned(),
        outcome: Outcome::Waiting,
    });
    let texts = panel_texts(&state);
    assert!(texts.contains(&"Measuring…".to_owned()), "{texts:?}");
}

#[test]
fn the_viewport_draws_the_distance_the_points_and_the_hovered_dots() {
    use varde_render::SketchLayer;
    let index = crate::pick::tests::plate();
    let palette = crate::Mode::Light.palette();
    let layer = |state: &MeasureState<'_>| -> SketchLayer {
        let measuring = crate::viewport::Measuring::new(state.clone());
        measuring.layers(&palette.scene, palette.sketching).1
    };
    let gap = |distance: f64| Between {
        distance: Ok(Gap {
            distance,
            points: [[0.0, 0.0, 10.0], [0.0, 0.0, 10.0 - distance]],
        }),
        angle: None,
    };
    let mut state = MeasureState {
        picks: [None, None],
        between: None,
        units: LengthUnit::Mm,
        folded: [true; 2],
        index: &index,
        hover: None,
        points: [Some(DVec3::new(30.0, 20.0, 10.0)), None],
    };
    let point_only = layer(&state);
    assert!(!point_only.is_empty());
    let camera = crate::pick::tests::camera(
        varde_render::View::Top,
        varde_render::Projection::Orthographic,
    );
    let label = |state: &MeasureState<'_>| {
        (crate::viewport::Measuring::new(state.clone()).label(&camera)).is_some()
    };
    assert!(!label(&state));
    // Apart: the segment and its label too.
    let apart = gap(10.0);
    state.between = Some(&apart);
    assert_ne!(layer(&state), point_only);
    assert!(label(&state));
    // Touching: nothing more, no label.
    let touching = gap(0.0);
    state.between = Some(&touching);
    assert_eq!(layer(&state), point_only);
    assert!(!label(&state));
    // Hovering the top: its corners' dots.
    let at = crate::pick::tests::shown(&camera, DVec3::new(20.0, 5.0, 10.0));
    state.hover = index.pick(&camera, crate::pick::tests::SIZE, at, crate::Picks::Faces);
    assert!(state.hover.is_some());
    assert_ne!(layer(&state), point_only);
}

/// A rectangle's width is the side nearer the horizontal, or for one
/// lying flat nearer X; the panel shows both after its area, and the
/// status bar's brief them in place of the area.
#[test]
fn a_rectangle_shows_its_width_and_height() {
    // A wall: 20 along Y, 10 up.
    assert_eq!(
        width_height([[0.0, 0.0, 10.0], [0.0, 20.0, 0.0]]),
        (20.0, 10.0)
    );
    // Leaning back: the side running level is still the width.
    assert_eq!(
        width_height([[0.0, 6.0, 8.0], [5.0, 0.0, 0.0]]),
        (5.0, 10.0)
    );
    // Lying flat: the side along X.
    assert_eq!(
        width_height([[0.0, -30.0, 0.0], [12.0, 0.0, 0.0]]),
        (12.0, 30.0)
    );

    let face = Measure::Face {
        area: 200.0,
        summary: Summary::Plane {
            n: [1.0, 0.0, 0.0],
            d: 0.0,
        },
        half_angle: None,
        rectangle: Some([[0.0, 0.0, 10.0], [0.0, 20.0, 0.0]]),
    };
    assert_eq!(
        shown(&values(&face, LengthUnit::Mm)),
        [
            ("Area", "200 mm²"),
            ("Width", "20 mm"),
            ("Height", "10 mm"),
            ("Normal", "1, 0, 0")
        ]
    );
    let inspected = Inspected {
        revision: 0,
        first: Ok(Probed {
            at: None,
            measure: Ok(face),
        }),
        second: None,
        between: None,
    };
    assert_eq!(
        shown(&brief(&inspected, LengthUnit::Mm)),
        [("Width", "20 mm"), ("Height", "10 mm")]
    );
}
