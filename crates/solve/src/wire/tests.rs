use std::collections::BTreeSet;

use varde_sketch::{Add, Constraint, Dimension, Measure, Side};

use super::*;
use crate::tests::{Drawn, analyse_at, drag, drawn, propose_on};

/// `request` posted and decoded, then answered by `solver` as the worker
/// does, and the reply decoded as the page does.
fn through(solver: &mut Solver, request: &Request, with_sketch: bool) -> Option<Response> {
    let posted = decode_request(&encode_request(request, with_sketch)).unwrap();
    let tag = posted.tag();
    assert_eq!(tag, request.tag());
    let response = posted.answer(solver);
    decode_reply(&encode_reply(tag, response.as_ref()), request).unwrap()
}

/// `reply` encoded, as a worker could send it.
fn encoded(reply: &Reply<Sketch, Analysis>) -> Vec<u8> {
    postcard::to_stdvec(reply).unwrap()
}

/// Whether decoding `reply` to `asked` fails it with an error.
fn fails(reply: &Reply<Sketch, Analysis>, asked: &Request) -> bool {
    matches!(
        decode_reply(&encoded(reply), asked),
        Ok(Some(Response::Failed { tag, .. })) if tag == asked.tag()
    )
}

fn constrain(drawn: &Drawn, constraint: Constraint) -> SketchEdit {
    SketchEdit::constrain(&drawn.sketch, vec![constraint])
}

#[test]
fn parameters_past_their_limits_are_refused() {
    let drawn = drawn();
    let long = "x".repeat(MAX_NAME_LEN + 1);
    let many: Vec<(String, String)> = (0..=MAX_PARAMS)
        .map(|i| (format!("p{i}"), "1".to_owned()))
        .collect();
    let long_text = "1".repeat(MAX_LEN + 1);
    let lists = [
        vec![(long.as_str(), "1")],
        vec![("p", long_text.as_str())],
        many.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect(),
    ];
    for list in lists {
        let posted: Posted<&Sketch, &SketchEdit, Vec<(&str, &str)>> = Posted::Analyse {
            revision: Revision::from(1),
            sketch: &drawn.sketch,
            units: LengthUnit::Mm,
            params: list,
        };
        let bytes = postcard::to_stdvec(&posted).unwrap();
        assert!(matches!(decode_request(&bytes), Err(Error::TooMany { .. })));
    }
}

#[test]
fn a_dimension_naming_a_parameter_is_checked_with_the_parameters_sent() {
    let drawn = drawn();
    let params = Params::evaluate([("width", "4 mm")], LengthUnit::Mm);
    let ask = Measure::Length(drawn.line).ask(&design(LengthUnit::Mm, &params));
    let mut sketch = (*drawn.sketch).clone();
    sketch
        .add_dimension(Dimension {
            measure: Measure::Length(drawn.line),
            value: Value::new("width", &ask).unwrap(),
            driving: true,
            label: DVec2::ZERO,
            side: Side::Positive,
        })
        .unwrap();
    let sketch = Arc::new(sketch);
    let mut request = analyse_at(3, &sketch);
    // Without the parameter the sketch is refused; with it, it's taken.
    assert!(matches!(
        decode_request(&encode_request(&request, true)),
        Err(Error::Sketch(_))
    ));
    if let Request::Analyse { params: p, .. } = &mut request {
        *p = Arc::new(params);
    }
    decode_request(&encode_request(&request, true)).unwrap();
}

#[test]
fn requests_round_trip() {
    let drawn = drawn();
    let edit = constrain(&drawn, Constraint::Fix(drawn.start));
    let mut request = propose_on(2, &drawn.sketch, edit.clone());
    let params = Arc::new(Params::evaluate(
        [("width", "2 * depth"), ("depth", "5"), ("bad", "nope")],
        LengthUnit::In,
    ));
    if let Request::Propose {
        units, params: p, ..
    } = &mut request
    {
        *units = LengthUnit::In;
        *p = Arc::clone(&params);
    }
    let Posted::Propose {
        base,
        sketch,
        edit: decoded,
        units,
        params: resolved,
    } = decode_request(&encode_request(&request, true)).unwrap()
    else {
        panic!("not a proposal");
    };
    assert_eq!(
        (base, &sketch, decoded, units),
        (Revision::from(2), &*drawn.sketch, edit, LengthUnit::In)
    );
    // Resolved again on the worker's side, errors and all.
    assert_eq!(resolved, *params);

    let request = drag(4, &drawn.sketch, drawn.end, DVec2::new(1.0, 2.0));
    for with_sketch in [true, false] {
        let Posted::Drag {
            session,
            sketch,
            points,
            radii,
            units,
        } = decode_request(&encode_request(&request, with_sketch)).unwrap()
        else {
            panic!("not a drag");
        };
        assert_eq!(session, 4);
        assert_eq!(
            sketch.as_ref().map(|(sketch, _)| sketch),
            with_sketch.then_some(&*drawn.sketch)
        );
        assert_eq!(points, [(drawn.end, DVec2::new(1.0, 2.0))]);
        assert!(radii.is_empty());
        assert_eq!(units, LengthUnit::Mm);
    }

    let Posted::Analyse {
        revision, sketch, ..
    } = decode_request(&encode_request(&analyse_at(9, &drawn.sketch), false)).unwrap()
    else {
        panic!("not an analysis");
    };
    assert_eq!((revision, &sketch), (Revision::from(9), &*drawn.sketch));
}

#[test]
fn responses_round_trip() {
    let drawn = drawn();
    let (mut solver, mut native) = (Solver::default(), Solver::default());
    for (request, with_sketch) in [
        (
            propose_on(
                1,
                &drawn.sketch,
                constrain(&drawn, Constraint::Fix(drawn.start)),
            ),
            true,
        ),
        (
            propose_on(
                2,
                &drawn.sketch,
                constrain(&drawn, Constraint::Horizontal(drawn.line)),
            ),
            true,
        ),
        (
            drag(3, &drawn.sketch, drawn.end, DVec2::new(12.0, 4.0)),
            true,
        ),
        // The worker has the session now.
        (
            drag(3, &drawn.sketch, drawn.end, DVec2::new(13.0, 4.0)),
            false,
        ),
        (analyse_at(4, &drawn.sketch), true),
    ] {
        // As natively, where a drag step always has its sketch.
        let expected = native.handle(request.clone());
        let got = through(&mut solver, &request, with_sketch);
        assert_eq!(format!("{got:?}"), format!("{expected:?}"));
        assert!(got.is_some());
    }
}

#[test]
fn an_unmoved_drag_step_is_no_response() {
    let drawn = drawn();
    let mut solver = Solver::default();
    // A circle can't be dragged to no size.
    let none = Request::Drag {
        session: 1,
        sketch: Arc::clone(&drawn.sketch),
        points: Vec::new(),
        radii: vec![(drawn.circle, -1.0)],
        units: LengthUnit::Mm,
        params: Arc::default(),
    };
    assert!(through(&mut solver, &none, true).is_none());
    // A worker without the session, as a new one is, can't step it.
    let near = drag(2, &drawn.sketch, drawn.end, DVec2::new(1.0, 1.0));
    assert!(through(&mut solver, &near, false).is_none());
}

#[test]
fn a_failure_round_trips() {
    let drawn = drawn();
    let asked = analyse_at(3, &drawn.sketch);
    let failed = Response::Failed {
        tag: asked.tag(),
        error: "internal error: on purpose".to_owned(),
    };
    let decoded = decode_reply(&encode_reply(asked.tag(), Some(&failed)), &asked).unwrap();
    assert!(matches!(
        decoded,
        Some(Response::Failed { tag, error }) if tag == asked.tag() && error == "internal error: on purpose"
    ));
}

#[test]
fn a_reply_to_another_request_is_an_error() {
    let drawn = drawn();
    let asked = analyse_at(3, &drawn.sketch);
    let reply = Reply::Analysed {
        revision: Revision::from(4),
        analysis: Analysis::default(),
    };
    assert_eq!(
        decode_reply(&encoded(&reply), &asked).unwrap_err(),
        Error::Answers(Tag::Analyse(4.into()))
    );
    let reply = Reply::<Sketch, Analysis>::Unmoved { session: 3 };
    assert!(matches!(
        decode_reply(&encoded(&reply), &asked),
        Err(Error::Answers(Tag::Drag(3)))
    ));
    let asked = propose_on(3, &drawn.sketch, SketchEdit::Delete(Vec::new()));
    let reply = Reply::Analysed {
        revision: Revision::from(3),
        analysis: Analysis::default(),
    };
    assert!(matches!(
        decode_reply(&encoded(&reply), &asked),
        Err(Error::Answers(_))
    ));
}

#[test]
fn an_accepted_sketch_must_pass_its_check() {
    let drawn = drawn();
    let asked = propose_on(1, &drawn.sketch, SketchEdit::Delete(Vec::new()));
    let accepted = |sketch: Sketch| Reply::Accepted {
        base: Revision::from(1),
        analysis: varde_sketch::analyse(&sketch),
        sketch,
    };
    assert!(!fails(&accepted((*drawn.sketch).clone()), &asked));
    for value in [f64::NAN, f64::INFINITY, 2.0 * MAX] {
        let mut sketch = (*drawn.sketch).clone();
        sketch.points[0].at.x = value;
        assert!(fails(&accepted(sketch), &asked));
    }
    let mut sketch = (*drawn.sketch).clone();
    sketch.next_id = 0;
    assert!(fails(&accepted(sketch), &asked));
}

#[test]
fn an_analysis_must_name_items_of_its_sketch() {
    let drawn = drawn();
    let asked = analyse_at(1, &drawn.sketch);
    let good = varde_sketch::analyse(&drawn.sketch);
    let analysed = |analysis: Analysis| Reply::<Sketch, Analysis>::Analysed {
        revision: Revision::from(1),
        analysis,
    };
    assert!(!fails(&analysed(good.clone()), &asked));

    // More degrees of freedom than variables.
    let mut analysis = good.clone();
    analysis.freedom = 2 * 3 + 1 + 1;
    assert!(fails(&analysed(analysis), &asked));
    // A constraint fixed, or a point redundant.
    let mut analysis = good.clone();
    analysis.fixed.insert(drawn.horizontal);
    assert!(fails(&analysed(analysis), &asked));
    let mut analysis = good.clone();
    analysis.redundant.insert(drawn.start);
    assert!(fails(&analysed(analysis), &asked));
    // An id the sketch doesn't have.
    let mut bigger = (*drawn.sketch).clone();
    let extra = bigger.add_point(DVec2::ZERO).unwrap();
    let mut analysis = good;
    analysis.fixed.insert(extra);
    assert!(fails(&analysed(analysis), &asked));
}

#[test]
fn an_analysis_counts_points_parameters_on_splines_as_variables() {
    // A spline through two fit points and a point on it: seven
    // variables, the point's parameter along it among them.
    let mut sketch = Sketch::default();
    let fit = [DVec2::ZERO, DVec2::new(10.0, 5.0)].map(|at| sketch.add_point(at).unwrap());
    let spline = Curve::Spline(varde_sketch::Spline::through(fit.to_vec(), false));
    let spline = sketch.add_curve(spline, false).unwrap();
    let point = sketch.add_point(DVec2::new(4.0, 2.0)).unwrap();
    sketch
        .add_constraint(Constraint::PointOnCurve {
            point,
            curve: spline,
        })
        .unwrap();
    let asked = analyse_at(1, &Arc::new(sketch));
    let analysed = |freedom| Reply::<Sketch, Analysis>::Analysed {
        revision: Revision::from(1),
        analysis: Analysis {
            freedom,
            ..Analysis::default()
        },
    };
    assert!(!fails(&analysed(7), &asked));
    assert!(fails(&analysed(8), &asked));
}

#[test]
fn an_analysis_counts_the_parameters_of_offsets_from_splines_as_variables() {
    // A spline through two fit points and two points offset from it,
    // held equally far: ten variables, two parameters along it among
    // them.
    let mut sketch = Sketch::default();
    let fit = [DVec2::ZERO, DVec2::new(10.0, 5.0)].map(|at| sketch.add_point(at).unwrap());
    let spline = Curve::Spline(varde_sketch::Spline::through(fit.to_vec(), false));
    let spline = sketch.add_curve(spline, false).unwrap();
    let [a, b] =
        [DVec2::new(3.0, 3.0), DVec2::new(6.0, 5.0)].map(|at| sketch.add_point(at).unwrap());
    sketch
        .add_constraint(Constraint::EqualOffset {
            a: [spline, a],
            b: [spline, b],
        })
        .unwrap();
    let asked = analyse_at(1, &Arc::new(sketch));
    let analysed = |freedom| Reply::<Sketch, Analysis>::Analysed {
        revision: Revision::from(1),
        analysis: Analysis {
            freedom,
            ..Analysis::default()
        },
    };
    assert!(!fails(&analysed(10), &asked));
    assert!(fails(&analysed(11), &asked));
}

#[test]
fn a_rejection_must_name_items_of_the_sketch_the_edit_makes() {
    let drawn = drawn();
    // Two new constraints: the edit's own ids are the sketch's next ones.
    let mut add = Add::new(&drawn.sketch);
    add.constraints.push(Constraint::Horizontal(drawn.line));
    add.constraints.push(Constraint::Fix(drawn.start));
    let edit = SketchEdit::Add(add);
    let applied = edit
        .apply(&drawn.sketch, &design(LengthUnit::Mm, Params::EMPTY))
        .unwrap();
    let new = applied.constraints[1].id;
    let past = {
        let mut more = applied.clone();
        more.add_constraint(Constraint::Fix(drawn.end)).unwrap()
    };
    let asked = propose_on(1, &drawn.sketch, edit);
    let rejected = |involved: BTreeSet<Id>| Reply::<Sketch, Analysis>::Rejected {
        base: Revision::from(1),
        why: Rejected::Redundant { involved },
    };
    assert!(!fails(&rejected([drawn.horizontal, new].into()), &asked));
    assert!(fails(&rejected([drawn.horizontal, past].into()), &asked));
    assert!(fails(&rejected([drawn.start].into()), &asked));
    // Naming nothing is always fine.
    let unsolved = Reply::<Sketch, Analysis>::Rejected {
        base: Revision::from(1),
        why: Rejected::Unsolved(varde_sketch::Failure::OutOfTime),
    };
    assert!(!fails(&unsolved, &asked));
}

#[test]
fn a_drag_solution_must_fit_its_session() {
    let drawn = drawn();
    let asked = drag(1, &drawn.sketch, drawn.end, DVec2::ZERO);
    let dragged = |points: Vec<DVec2>, radii: Vec<f64>| Reply::<Sketch, Analysis>::Dragged {
        session: 1,
        points,
        radii,
    };
    let places: Vec<_> = drawn.sketch.points.iter().map(|point| point.at).collect();
    let Ok(Some(Response::Dragged { solution, .. })) =
        decode_reply(&encoded(&dragged(places.clone(), vec![3.0])), &asked)
    else {
        panic!("refused");
    };
    assert_eq!(*solution, *drawn.sketch);

    assert!(fails(&dragged(places[1..].to_vec(), vec![3.0]), &asked));
    assert!(fails(&dragged(places.clone(), Vec::new()), &asked));
    assert!(fails(&dragged(places.clone(), vec![3.0, 3.0]), &asked));
    assert!(fails(&dragged(places.clone(), vec![0.0]), &asked));
    assert!(fails(&dragged(places.clone(), vec![f64::NAN]), &asked));
    let mut far = places;
    far[0].y = f64::NEG_INFINITY;
    assert!(fails(&dragged(far, vec![3.0]), &asked));
}

#[test]
fn a_request_whose_sketch_fails_its_check_is_an_error() {
    let drawn = drawn();
    let mut sketch = (*drawn.sketch).clone();
    sketch.points[1].at.y = f64::NAN;
    let sketch = Arc::new(sketch);
    for request in [
        propose_on(1, &sketch, SketchEdit::Delete(Vec::new())),
        drag(1, &sketch, drawn.end, DVec2::ZERO),
        analyse_at(1, &sketch),
    ] {
        assert!(matches!(
            decode_request(&encode_request(&request, true)),
            Err(Error::Sketch(_))
        ));
    }
}

#[test]
fn a_request_out_of_bounds_is_an_error() {
    let drawn = drawn();
    let bad = [f64::NAN, f64::INFINITY, -2.0 * MAX];
    for value in bad {
        let request = drag(1, &drawn.sketch, drawn.end, DVec2::new(0.0, value));
        assert!(matches!(
            decode_request(&encode_request(&request, false)),
            Err(Error::Value(_))
        ));
        let radius = Request::Drag {
            session: 1,
            sketch: Arc::clone(&drawn.sketch),
            points: Vec::new(),
            radii: vec![(drawn.circle, value)],
            units: LengthUnit::Mm,
            params: Arc::default(),
        };
        assert!(matches!(
            decode_request(&encode_request(&radius, false)),
            Err(Error::Value(_))
        ));
        let moved = SketchEdit::Move {
            points: vec![(drawn.end, DVec2::new(value, 0.0))],
            radii: Vec::new(),
        };
        assert!(matches!(
            decode_request(&encode_request(&propose_on(1, &drawn.sketch, moved), true)),
            Err(Error::Value(_))
        ));
        let mut add = Add::new(&drawn.sketch);
        add.point(DVec2::new(value, 0.0)).unwrap();
        assert!(matches!(
            decode_request(&encode_request(
                &propose_on(1, &drawn.sketch, SketchEdit::Add(add)),
                true
            )),
            Err(Error::Value(_))
        ));
    }

    let many = Request::Drag {
        session: 1,
        sketch: Arc::clone(&drawn.sketch),
        points: vec![(drawn.end, DVec2::ZERO); MAX_POINTS + 1],
        radii: Vec::new(),
        units: LengthUnit::Mm,
        params: Arc::default(),
    };
    assert_eq!(
        decode_request(&encode_request(&many, false)).unwrap_err(),
        Error::TooMany {
            count: MAX_POINTS + 1,
            limit: MAX_POINTS
        }
    );
    let construction = SketchEdit::SetConstruction {
        ids: vec![drawn.line; MAX_CURVES + 1],
        construction: true,
    };
    assert!(matches!(
        decode_request(&encode_request(
            &propose_on(1, &drawn.sketch, construction),
            true
        )),
        Err(Error::TooMany { .. })
    ));
}

#[test]
fn bytes_after_the_end_are_an_error() {
    let drawn = drawn();
    let mut bytes = encode_request(&analyse_at(1, &drawn.sketch), true);
    bytes.push(0);
    assert!(matches!(decode_request(&bytes), Err(Error::Request(_))));

    let asked = analyse_at(1, &drawn.sketch);
    let response = Solver::default().handle(asked.clone());
    let mut bytes = encode_reply(asked.tag(), response.as_ref());
    bytes.push(0);
    assert!(matches!(decode_reply(&bytes, &asked), Err(Error::Reply(_))));
}

/// A little deterministic noise, for the malformed bytes below.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u8 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 56) as u8
    }
}

#[test]
fn malformed_bytes_are_refused_never_a_panic() {
    let drawn = drawn();
    let mut solver = Solver::default();
    let edit = constrain(&drawn, Constraint::Fix(drawn.start));
    let (dimensioned, id) = dimensioned(&drawn, "2 * 6");
    let value = Value::new(
        "3 in",
        &Measure::Length(drawn.line).ask(&design(LengthUnit::Mm, Params::EMPTY)),
    )
    .unwrap();
    let requests = [
        propose_on(1, &drawn.sketch, edit),
        drag(2, &drawn.sketch, drawn.end, DVec2::new(12.0, 4.0)),
        analyse_at(3, &drawn.sketch),
        propose_on(4, &dimensioned, SketchEdit::SetDimension { id, value }),
    ];
    let mut noise = Noise(7);
    for request in &requests {
        let posted = encode_request(request, true);
        let response = solver.handle(request.clone());
        let reply = encode_reply(request.tag(), response.as_ref());
        for bytes in [&posted, &reply] {
            // Every prefix, and each byte changed in turn.
            for end in 0..bytes.len() {
                let _ = decode_request(&bytes[..end]);
                let _ = decode_reply(&bytes[..end], request);
            }
            for at in 0..bytes.len() {
                let mut changed = bytes.clone();
                changed[at] ^= noise.next() | 1;
                let _ = decode_request(&changed);
                let _ = decode_reply(&changed, request);
            }
        }
        for len in 0..64 {
            let bytes: Vec<u8> = (0..len).map(|_| noise.next()).collect();
            let _ = decode_request(&bytes);
            let _ = decode_reply(&bytes, request);
        }
    }
    assert!(decode_request(&[]).is_err());
    assert!(decode_reply(&[], &requests[0]).is_err());
}

#[test]
fn errors_display() {
    for error in [
        Error::Answers(Tag::Drag(1)),
        Error::TooMany { count: 2, limit: 1 },
        Error::Value(f64::NAN),
        Error::Items,
        Error::Request(DecodeError::new("no")),
        Error::Reply(DecodeError::new("no")),
    ] {
        assert!(!error.to_string().is_empty());
    }
}

/// `drawn`'s sketch with its line's length a driving dimension of `text`
/// millimetres, and the dimension.
fn dimensioned(drawn: &Drawn, text: &str) -> (Arc<Sketch>, Id) {
    let mut sketch = (*drawn.sketch).clone();
    let measure = Measure::Length(drawn.line);
    let value = Value::new(text, &measure.ask(&design(LengthUnit::Mm, Params::EMPTY))).unwrap();
    let id = sketch
        .add_dimension(Dimension {
            measure,
            value,
            driving: true,
            label: DVec2::ZERO,
            side: Side::Positive,
        })
        .unwrap();
    (Arc::new(sketch), id)
}

#[test]
fn dimensions_and_their_edits_cross() {
    let drawn = drawn();
    let (sketch, id) = dimensioned(&drawn, "5 + 5");
    let value = Value::new(
        "12",
        &Measure::Length(drawn.line).ask(&design(LengthUnit::Mm, Params::EMPTY)),
    )
    .unwrap();
    let mut solver = Solver::default();
    for edit in [
        SketchEdit::SetDimension { id, value },
        SketchEdit::SetDriving { id, driving: false },
        SketchEdit::MoveLabel {
            id,
            label: DVec2::new(1.0, -2.0),
        },
    ] {
        let request = propose_on(1, &sketch, edit.clone());
        let Posted::Propose {
            edit: decoded,
            sketch: sent,
            ..
        } = decode_request(&encode_request(&request, true)).unwrap()
        else {
            panic!("not a proposal");
        };
        assert_eq!((&decoded, &sent), (&edit, &*sketch));
        let response = solver.handle(request.clone());
        assert!(
            matches!(response, Some(Response::Accepted { .. })),
            "{edit:?}"
        );
        let reply = encode_reply(request.tag(), response.as_ref());
        assert!(matches!(
            decode_reply(&reply, &request),
            Ok(Some(Response::Accepted { .. }))
        ));
    }
}

#[test]
fn a_sketch_s_values_are_checked_in_the_units_sent() {
    let drawn = drawn();
    let (sketch, _) = dimensioned(&drawn, "5 + 5");
    let mut inches = propose_on(1, &sketch, SketchEdit::Delete(Vec::new()));
    assert!(decode_request(&encode_request(&inches, true)).is_ok());
    if let Request::Propose { units, .. } = &mut inches {
        *units = LengthUnit::In;
    }
    assert!(matches!(
        decode_request(&encode_request(&inches, true)),
        Err(Error::Sketch(SketchError::Value(_)))
    ));
    // Nor is a solution taken whose values its units don't give.
    let reply = Reply::Accepted {
        base: Revision::from(1),
        analysis: varde_sketch::analyse(&sketch),
        sketch: (*sketch).clone(),
    };
    assert!(fails(&reply, &inches));
}

#[test]
fn a_dimension_edit_out_of_bounds_is_an_error() {
    let drawn = drawn();
    let (sketch, id) = dimensioned(&drawn, "10");
    let refused =
        |edit: SketchEdit| decode_request(&encode_request(&propose_on(1, &sketch, edit), true));
    for value in [f64::NAN, f64::INFINITY] {
        let edit = SketchEdit::SetDimension {
            id,
            value: Value {
                text: "1".to_owned(),
                value,
            },
        };
        assert!(matches!(refused(edit), Err(Error::Value(_))));
        let edit = SketchEdit::MoveLabel {
            id,
            label: DVec2::new(value, 0.0),
        };
        assert!(matches!(refused(edit), Err(Error::Value(_))));
        let edit = SketchEdit::Offset {
            chain: vec![sketch.curves[0].id],
            distance: Value {
                text: "1".to_owned(),
                value,
            },
            side: Side::Positive,
        };
        assert!(matches!(refused(edit), Err(Error::Value(_))));
        let bad = Value {
            text: "1".to_owned(),
            value,
        };
        let good = Value {
            text: "1".to_owned(),
            value: 1.0,
        };
        let lines = [drawn.line, drawn.line];
        let edit = SketchEdit::Fillet {
            at: drawn.start,
            lines,
            radius: bad.clone(),
        };
        assert!(matches!(refused(edit), Err(Error::Value(_))));
        for setback in [
            Setback::Equal(bad.clone()),
            Setback::Two(good.clone(), bad.clone()),
            Setback::Angle(bad.clone(), good.clone()),
        ] {
            let edit = SketchEdit::Chamfer {
                at: drawn.start,
                lines,
                setback,
            };
            assert!(matches!(refused(edit), Err(Error::Value(_))));
        }
    }
    let many = SketchEdit::Offset {
        chain: vec![sketch.curves[0].id; MAX_CURVES + 1],
        distance: Value {
            text: "1".to_owned(),
            value: 1.0,
        },
        side: Side::Positive,
    };
    assert!(matches!(refused(many), Err(Error::TooMany { .. })));
    let long = SketchEdit::SetDimension {
        id,
        value: Value {
            text: "1".repeat(varde_expr::MAX_LEN + 1),
            value: 1.0,
        },
    };
    assert!(matches!(refused(long), Err(Error::TooMany { .. })));

    let dimension = sketch.dimension(id).unwrap().dimension.clone();
    let mut add = Add::new(&sketch);
    add.dimensions = vec![dimension.clone(); MAX_DIMENSIONS + 1];
    assert!(matches!(
        refused(SketchEdit::Add(add)),
        Err(Error::TooMany { .. })
    ));
    let mut add = Add::new(&sketch);
    add.dimensions.push(Dimension {
        label: DVec2::new(0.0, -2.0 * MAX),
        ..dimension
    });
    assert!(matches!(
        refused(SketchEdit::Add(add)),
        Err(Error::Value(_))
    ));
}

#[test]
fn a_driving_rejection_must_name_dimensions_it_involves() {
    let drawn = drawn();
    let (sketch, id) = dimensioned(&drawn, "10");
    // A second length of the same line: redundant, and new.
    let mut add = Add::new(&sketch);
    add.dimensions
        .push(sketch.dimension(id).unwrap().dimension.clone());
    let edit = SketchEdit::Add(add);
    let new = edit
        .apply(&sketch, &design(LengthUnit::Mm, Params::EMPTY))
        .unwrap()
        .dimensions[1]
        .id;
    let asked = propose_on(1, &sketch, edit);
    let Some(Response::Rejected { why, .. }) = Solver::default().handle(asked.clone()) else {
        panic!("accepted");
    };
    let Rejected::Driving {
        dimensions,
        involved,
    } = why.clone()
    else {
        panic!("{why:?}");
    };
    assert_eq!(dimensions, BTreeSet::from([new]));
    assert_eq!(involved, BTreeSet::from([id, new]));
    let rejected =
        |dimensions: BTreeSet<Id>, involved: BTreeSet<Id>| Reply::<Sketch, Analysis>::Rejected {
            base: Revision::from(1),
            why: Rejected::Driving {
                dimensions,
                involved,
            },
        };
    assert!(!fails(
        &rejected(dimensions.clone(), involved.clone()),
        &asked
    ));
    // Dimensions it doesn't involve, none, or not dimensions.
    assert!(fails(&rejected(dimensions.clone(), [id].into()), &asked));
    assert!(fails(&rejected(BTreeSet::new(), involved.clone()), &asked));
    assert!(fails(
        &rejected([drawn.horizontal].into(), [drawn.horizontal, id].into()),
        &asked
    ));
}

#[test]
fn a_rejection_may_name_a_chamfer_whose_corner_implies_equations() {
    let drawn = drawn();
    // A line up from the first's start, and a chamfer on their corner.
    let mut sketch = (*drawn.sketch).clone();
    let up = sketch.add_point(DVec2::new(0.0, 5.0)).unwrap();
    let side = sketch
        .add_curve(
            Curve::Line {
                start: up,
                end: drawn.start,
            },
            false,
        )
        .unwrap();
    let design = design(LengthUnit::Mm, Params::EMPTY);
    let ask = Measure::Length(drawn.line).ask(&design);
    let chamfer = SketchEdit::Chamfer {
        at: drawn.start,
        lines: [drawn.line, side],
        setback: Setback::Equal(Value::new("1", &ask).unwrap()),
    };
    let sketch = Arc::new(chamfer.apply(&sketch, &design).unwrap());
    let cut = sketch.curves.last().unwrap().id;
    let asked = propose_on(1, &sketch, SketchEdit::Delete(Vec::new()));
    let rejected = |involved: BTreeSet<Id>| Reply::<Sketch, Analysis>::Rejected {
        base: Revision::from(1),
        why: Rejected::Redundant { involved },
    };
    assert!(!fails(&rejected([drawn.horizontal, cut].into()), &asked));
    assert!(fails(&rejected([drawn.horizontal, side].into()), &asked));
}
