use std::f64::consts::{FRAC_PI_2, PI};

use varde_expr::LengthUnit;

use super::*;
use crate::testing::{self, at, typed};
use crate::{Inference, Target};

fn close(a: DVec2, b: DVec2) -> bool {
    a.abs_diff_eq(b, 1e-9)
}

#[test]
fn each_tool_offers_its_fields_once_its_shape_has_a_point() {
    assert!(fields(Tool::Line, 0).is_empty());
    assert_eq!(fields(Tool::Line, 1), [Field::Length, Field::Angle]);
    assert_eq!(fields(Tool::Circle, 1), [Field::Diameter]);
    // An arc's radius once its ends are placed.
    assert!(fields(Tool::Arc, 1).is_empty());
    assert_eq!(fields(Tool::Arc, 2), [Field::Radius]);
    assert_eq!(fields(Tool::Rectangle, 1), [Field::Width, Field::Height]);
    assert_eq!(fields(Tool::Polygon, 1), [Field::Sides, Field::Diameter]);
    assert!(fields(Tool::Point, 0).is_empty());
    assert!(fields(Tool::Dimension, 1).is_empty());
    // The Offset tool's distance, once it has its chain.
    let offset = testing::tool(Tool::Offset, &[], &[]);
    assert!(offset.fields().is_empty());
    let chain = [Id::X_AXIS];
    let picked = ActiveTool {
        picked: &chain,
        ..offset
    };
    assert_eq!(picked.fields(), [Field::Distance]);
    let design = testing::DESIGN;
    assert_eq!(
        read(&picked, Field::Distance, "2 cm", &design)
            .unwrap()
            .value,
        20.0
    );
    assert_eq!(
        read(&picked, Field::Distance, "0", &design)
            .unwrap_err()
            .kind,
        ErrorKind::NotPositive
    );
}

#[test]
fn fields_read_expressions_of_their_kind() {
    let placed = [at(0.0, 0.0)];
    let tool = testing::tool(Tool::Line, &placed, &[None]);
    let design = testing::DESIGN;
    let read = |field, text| read(&tool, field, text, &design);
    assert_eq!(read(Field::Length, "40 / 2").unwrap().value, 20.0);
    assert_eq!(read(Field::Length, "1 in").unwrap().value, 25.4);
    assert!((read(Field::Angle, "90").unwrap().value - FRAC_PI_2).abs() < 1e-12);
    let mismatch = read(Field::Length, "3 deg").unwrap_err();
    assert!(matches!(mismatch.kind, ErrorKind::Wrong { .. }));
    assert_eq!(
        read(Field::Angle, "0").unwrap_err().kind,
        ErrorKind::NotPositive
    );

    // Sides: a whole number, three to sixty-four.
    assert_eq!(read(Field::Sides, "2 * 4").unwrap().value, 8.0);
    assert_eq!(read(Field::Sides, "64").unwrap().value, 64.0);
    assert!(matches!(
        read(Field::Sides, "2").unwrap_err().kind,
        ErrorKind::TooSmall { .. }
    ));
    assert!(matches!(
        read(Field::Sides, "65").unwrap_err().kind,
        ErrorKind::TooLarge { .. }
    ));
    assert_eq!(
        read(Field::Sides, "5.5").unwrap_err().kind,
        ErrorKind::NotWhole
    );
    assert!(read(Field::Sides, "1e300").is_err());
}

#[test]
fn an_arc_s_radius_has_to_reach_across_its_chord() {
    let placed = [at(0.0, 0.0), at(10.0, 0.0)];
    let tool = testing::tool(Tool::Arc, &placed, &[None, None]);
    let design = testing::DESIGN;
    assert!(read(&tool, Field::Radius, "5", &design).is_ok());
    let short = read(&tool, Field::Radius, "4.9", &design).unwrap_err();
    assert_eq!(
        short.kind,
        ErrorKind::TooSmall {
            min: 5.0,
            unit: Some(LengthUnit::Mm.into())
        }
    );
    assert_eq!(short.span, Span::new(0, 3));
}

#[test]
fn a_line_s_values_hold_its_end_and_the_cursor_moves_the_rest() {
    let placed = [at(1.0, 1.0)];
    let mut tool = testing::tool(Tool::Line, &placed, &[None]);
    let end = |tool: &ActiveTool, x, y| match outline(tool, at(x, y)) {
        Some(Outline::Line { start, end }) => {
            assert_eq!(start, at(1.0, 1.0));
            end
        }
        other => panic!("{other:?}"),
    };
    // Nothing typed, to the cursor.
    assert_eq!(end(&tool, 4.0, 5.0), at(4.0, 5.0));

    // The length typed, towards the cursor.
    let length = [typed(Field::Length, "10")];
    tool.typed = &length;
    assert!(close(end(&tool, 4.0, 5.0), at(7.0, 9.0)));
    assert!(close(end(&tool, 1.0, -3.0), at(1.0, -9.0)));

    // The angle typed, as far along it as the cursor is.
    let angle = [typed(Field::Angle, "90")];
    tool.typed = &angle;
    assert!(close(end(&tool, 3.0, 6.0), at(1.0, 6.0)));
    // Behind the start is nowhere along it.
    assert!(close(end(&tool, 3.0, -6.0), at(1.0, 1.0)));

    // Both typed, wherever the cursor is.
    let both = [typed(Field::Angle, "180"), typed(Field::Length, "2")];
    tool.typed = &both;
    assert!(close(end(&tool, 30.0, 6.0), at(-1.0, 1.0)));
    let line = outline(&tool, at(0.0, 0.0)).unwrap();
    assert!((line.value(Field::Length).unwrap() - 2.0).abs() < 1e-12);
    assert!((line.value(Field::Angle).unwrap() - PI).abs() < 1e-12);
    // Angles measure from X counter-clockwise, all the way round.
    let down = Outline::Line {
        start: at(0.0, 0.0),
        end: at(0.0, -1.0),
    };
    assert!((down.value(Field::Angle).unwrap() - 1.5 * PI).abs() < 1e-12);
}

#[test]
fn a_level_line_s_angle_is_never_minus_zero() {
    // A rise of -0 (from -0 to 0) gives atan2 -0 rightwards, which would
    // show as "-0°", and -π leftwards.
    let right = Outline::Line {
        start: at(0.0, 0.0),
        end: at(2.0, -0.0),
    };
    let angle = right.value(Field::Angle).unwrap();
    assert_eq!(angle.to_bits(), 0.0f64.to_bits());
    let left = Outline::Line {
        start: at(2.0, 0.0),
        end: at(0.0, -0.0),
    };
    assert_eq!(left.value(Field::Angle), Some(PI));
}

#[test]
fn a_line_s_angle_is_below_a_full_turn() {
    // atan2(-1e-17, 1) + 2π rounds to 2π, which would show as "360°".
    for (x, rise) in [(1.0, -1e-17), (1e6, -1e-12), (1.0, -f64::MIN_POSITIVE)] {
        let line = Outline::Line {
            start: at(0.0, 0.0),
            end: at(x, rise),
        };
        let angle = line.value(Field::Angle).unwrap();
        assert!((0.0..TAU).contains(&angle), "{rise}: {angle}");
        assert_eq!(angle.to_bits(), 0.0f64.to_bits(), "{rise}");
    }
    // Just far enough below level to stay below a full turn.
    let below = Outline::Line {
        start: at(0.0, 0.0),
        end: at(1.0, -1e-15),
    };
    let angle = below.value(Field::Angle).unwrap();
    assert!(angle < TAU && angle > TAU - 1e-14, "{angle}");
    // An end that isn't a number has no angle.
    let lost = Outline::Line {
        start: at(0.0, 0.0),
        end: at(f64::NAN, 0.0),
    };
    assert_eq!(lost.value(Field::Angle), None);
}

#[test]
fn a_typed_angle_places_the_end_with_libm_s_bits() {
    // The end is saved, so it has to have the same bits natively and on
    // the web: libm's, not the platform's. At 9.2° glibc's sine is an ulp
    // off libm's (x86_64 Linux).
    let placed = [at(1.0, 1.0)];
    let mut tool = testing::tool(Tool::Line, &placed, &[None]);
    let both = [typed(Field::Angle, "9.2"), typed(Field::Length, "10")];
    tool.typed = &both;
    let angle = tool.value_in(Field::Angle).unwrap().value;
    let direction = DVec2::new(libm::cos(angle), libm::sin(angle));
    let line = outline(&tool, at(30.0, 6.0)).unwrap();
    let Outline::Line { start, end } = line else {
        panic!("{line:?}")
    };
    assert_eq!(end, start + direction * 10.0);
    let back = libm::atan2(end.y - start.y, end.x - start.x);
    assert_eq!(line.value(Field::Angle), Some(back));

    // As far along it as the cursor goes, the same.
    let angle = [typed(Field::Angle, "9.2")];
    tool.typed = &angle;
    let Some(Outline::Line { end, .. }) = outline(&tool, at(30.0, 6.0)) else {
        panic!()
    };
    let along = (at(30.0, 6.0) - start).dot(direction);
    assert_eq!(end, start + direction * along);
}

#[test]
fn a_circle_s_and_an_arc_s_values_hold_their_size() {
    let center = [at(0.0, 0.0)];
    let mut circle = testing::tool(Tool::Circle, &center, &[None]);
    let diameter = [typed(Field::Diameter, "8")];
    circle.typed = &diameter;
    assert_eq!(
        outline(&circle, at(1.0, 0.0)),
        Some(Outline::Circle {
            center: at(0.0, 0.0),
            radius: 4.0
        })
    );

    let ends = [at(-3.0, 0.0), at(3.0, 0.0)];
    let mut arc = testing::tool(Tool::Arc, &ends, &[None, None]);
    let radius = [typed(Field::Radius, "5")];
    arc.typed = &radius;
    let middle = |tool: &ActiveTool, x, y| {
        let outline = outline(tool, at(x, y)).unwrap();
        let Outline::Arc(points) = outline else {
            panic!("{outline:?}")
        };
        assert!((points.center.distance(points.start) - 5.0).abs() < 1e-9);
        assert!((points.center.distance(points.end) - 5.0).abs() < 1e-9);
        outline.aimed(at(x, y))
    };
    // Bulging towards the cursor: the short way round near the chord,
    // the long way far from it, either side.
    assert!(close(middle(&arc, 0.0, 1.5), at(0.0, 1.0)));
    assert!(close(middle(&arc, 0.0, 8.0), at(0.0, 9.0)));
    assert!(close(middle(&arc, 0.0, -1.5), at(0.0, -1.0)));
    // A radius short of the chord makes no arc.
    let short = [typed(Field::Radius, "2")];
    arc.typed = &short;
    assert_eq!(outline(&arc, at(0.0, 1.0)), None);
}

#[test]
fn a_rectangle_from_a_corner_or_its_centre() {
    let first = [at(1.0, 1.0)];
    let mut tool = testing::tool(Tool::Rectangle, &first, &[None]);
    let corners = |tool: &ActiveTool, x, y| match outline(tool, at(x, y)) {
        Some(Outline::Rectangle { corners, center }) => (corners, center),
        other => panic!("{other:?}"),
    };
    let (drawn, center) = corners(&tool, 4.0, -1.0);
    assert_eq!(
        drawn,
        [at(1.0, 1.0), at(4.0, 1.0), at(4.0, -1.0), at(1.0, -1.0)]
    );
    assert_eq!(center, None);
    let outline_at = outline(&tool, at(4.0, -1.0)).unwrap();
    assert_eq!(outline_at.value(Field::Width), Some(3.0));
    assert_eq!(outline_at.value(Field::Height), Some(2.0));
    assert_eq!(outline_at.construction(), None);

    // The width typed, the height the cursor's, which way the cursor is.
    let width = [typed(Field::Width, "10")];
    tool.typed = &width;
    let (drawn, _) = corners(&tool, -4.0, 3.0);
    assert_eq!(drawn[2], at(-9.0, 3.0));

    // From the centre, the cursor is at a corner.
    tool.centered = true;
    tool.typed = &[];
    let (drawn, center) = corners(&tool, 4.0, 3.0);
    assert_eq!(center, Some(at(1.0, 1.0)));
    assert_eq!(
        drawn,
        [at(-2.0, -1.0), at(4.0, -1.0), at(4.0, 3.0), at(-2.0, 3.0)]
    );
    let outline_at = outline(&tool, at(4.0, 3.0)).unwrap();
    assert_eq!(outline_at.value(Field::Width), Some(6.0));
    assert_eq!(
        outline_at.construction(),
        Some(vec![at(-2.0, -1.0), at(4.0, 3.0)])
    );
    let both = [typed(Field::Width, "4"), typed(Field::Height, "2")];
    tool.typed = &both;
    let (drawn, _) = corners(&tool, -40.0, 30.0);
    assert_eq!(
        drawn,
        [at(3.0, 0.0), at(-1.0, 0.0), at(-1.0, 2.0), at(3.0, 2.0)]
    );
}

#[test]
fn a_polygon_has_its_sides_on_its_circle() {
    let center = [at(0.0, 0.0)];
    let mut tool = testing::tool(Tool::Polygon, &center, &[None]);
    let Some(Outline::Polygon {
        radius, corners, ..
    }) = outline(&tool, at(0.0, 2.0))
    else {
        panic!()
    };
    assert_eq!(radius, 2.0);
    assert_eq!(corners.len(), DEFAULT_SIDES as usize);
    assert_eq!(corners[0], at(0.0, 2.0));
    for (k, corner) in corners.iter().enumerate() {
        assert!((corner.length() - 2.0).abs() < 1e-12);
        let next = corners[(k + 1) % corners.len()];
        // A hexagon's sides are its radius.
        assert!((corner.distance(next) - 2.0).abs() < 1e-12);
    }

    tool.sides = 4;
    let diameter = [typed(Field::Diameter, "2")];
    tool.typed = &diameter;
    let polygon = outline(&tool, at(5.0, 0.0)).unwrap();
    let Outline::Polygon { corners, .. } = &polygon else {
        panic!()
    };
    assert_eq!(corners.len(), 4);
    assert!(close(corners[1], at(0.0, 1.0)));
    assert_eq!(polygon.value(Field::Sides), Some(4.0));
    assert_eq!(polygon.value(Field::Diameter), Some(2.0));
    assert_eq!(
        polygon.construction().map(|circle| circle.len() > 4),
        Some(true)
    );
}

#[test]
fn a_click_is_held_where_the_values_typed_put_it() {
    let placed = [at(0.0, 0.0)];
    let mut tool = testing::tool(Tool::Line, &placed, &[None]);
    let click = ToolClick {
        at: at(3.0, 0.0),
        target: Some(Target::On(Id::X_AXIS)),
        inference: Some(Inference::Horizontal),
        hit: None,
        pixel: 0.1,
        double: false,
        reference: false,
    };
    // Nothing typed, as it snapped.
    assert_eq!(aim(&tool, click), Some(click));

    // A length typed takes it off what it snapped to, but not off the
    // way it runs.
    let length = [typed(Field::Length, "5")];
    tool.typed = &length;
    let aimed = aim(&tool, click).unwrap();
    assert_eq!(aimed.at, at(5.0, 0.0));
    assert_eq!(aimed.target, None);
    assert_eq!(aimed.inference, Some(Inference::Horizontal));
    // An angle typed says how it runs.
    let angle = [typed(Field::Angle, "90")];
    tool.typed = &angle;
    let aimed = aim(
        &tool,
        ToolClick {
            at: at(1.0, 4.0),
            ..click
        },
    )
    .unwrap();
    assert!(close(aimed.at, at(0.0, 4.0)));
    assert_eq!(aimed.inference, None);

    // A radius an arc can't have places nothing.
    let ends = [at(0.0, 0.0), at(10.0, 0.0)];
    let mut arc = testing::tool(Tool::Arc, &ends, &[None, None]);
    let short = [typed(Field::Radius, "1")];
    arc.typed = &short;
    assert_eq!(aim(&arc, click), None);
}

#[test]
fn a_spline_runs_through_the_points_placed_to_the_cursor() {
    let placed = [at(0.0, 0.0), at(4.0, 3.0), at(8.0, 0.0)];
    let tool = testing::tool(Tool::Spline, &placed, &[]);
    assert!(tool.fields().is_empty());
    let outline = outline(&tool, at(12.0, 2.0)).unwrap();
    let points = [placed.as_slice(), &[at(12.0, 2.0)]].concat();
    assert_eq!(
        outline,
        Outline::Spline {
            points: points.clone(),
            kind: SplineKind::Through
        }
    );
    let drawn = outline.polylines();
    assert_eq!(
        drawn,
        [flatten_spline(&points, SplineKind::Through, false).unwrap()]
    );
    assert_eq!(outline.construction(), None);
    assert_eq!(outline.aimed(at(12.0, 2.0)), at(12.0, 2.0));
    // By control points: its control polygon, and short of four, straight
    // between them.
    let control = ActiveTool {
        control: true,
        placed: &placed[..2],
        ..tool
    };
    let short = super::outline(&control, at(8.0, 0.0)).unwrap();
    assert_eq!(short.polylines(), [placed.to_vec()]);
    assert_eq!(short.construction(), Some(placed.to_vec()));
    // Nothing placed, nothing drawn.
    let empty = testing::tool(Tool::Spline, &[], &[]);
    assert_eq!(super::outline(&empty, at(1.0, 1.0)), None);
}

#[test]
fn typed_angles_at_the_edges_place_the_end_with_libm_s_bits() {
    let design = testing::DESIGN;
    let placed = [at(3.0, -2.0)];
    let tool = testing::tool(Tool::Line, &placed, &[None]);
    // Neither none nor a whole turn, nor under none, nor not a number.
    for text in ["0", "360", "-90", "1e400", "0/0", "inf"] {
        assert!(read(&tool, Field::Angle, text, &design).is_err(), "{text}");
    }
    for degrees in ["90", "180", "270", "0.000001", "359.999999"] {
        for length in ["0.001", "1", "1e6"] {
            let angle = read(&tool, Field::Angle, degrees, &design).unwrap();
            let size = read(&tool, Field::Length, length, &design).unwrap();
            let both = [(Field::Angle, angle.clone()), (Field::Length, size.clone())];
            let typing = ActiveTool {
                typed: &both,
                ..tool
            };
            let line = outline(&typing, at(30.0, 6.0)).unwrap();
            let Outline::Line { start, end } = line else {
                panic!("{line:?}")
            };
            let direction = DVec2::new(libm::cos(angle.value), libm::sin(angle.value));
            assert_eq!(end, start + direction * size.value, "{degrees} {length}");
            // The angle shown for it is the one typed, to its rounding.
            let shown = line.value(Field::Angle).unwrap();
            let slack = 1e-15 * design.max / size.value;
            assert!(
                (shown - angle.value).abs() < slack,
                "{degrees} {length}: {shown}"
            );
        }
    }
}
