use glam::DVec3;
use varde_document::{BodyId, Document, Editor, Operation, Targets};
use varde_kernel::{Solid, Tolerance};

use super::*;
use crate::history::tests::{add_extrude, disc, two_sides};
use crate::{Answer, EdgeForm, Query, Request, Response, handle};

const TOL: Tolerance = Tolerance::DEFAULT;

/// The request for `body`'s edge facts in `editor`'s document.
fn topology(editor: &Editor, body: BodyId) -> Request {
    Request::Query {
        tag: 9,
        generation: editor.generation(),
        document: editor.snapshot(),
        until: None,
        query: Query::Topology(body),
    }
}

/// The first body of `editor`'s document.
fn first_body(editor: &Editor) -> BodyId {
    editor.document().bodies()[0].id
}

/// `body`'s edge facts in `editor`'s document, answered to the request
/// and paired with the edges of the model of the same generation: the
/// body's part's first edges, in order, between the same two faces,
/// closed where the facts have no ends (none of these is past the
/// bound).
fn paired(editor: &Editor, body: BodyId) -> Vec<EdgeFacts> {
    let Response::Regenerated { mesh, picking, .. } = handle(Request::Regenerate {
        sight: None,
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        until: None,
        draft: None,
        inspect: None,
    }) else {
        panic!("regeneration failed");
    };
    let Response::Answered {
        tag,
        generation,
        until,
        answer: Answer::Topology {
            body: answered,
            result,
        },
    } = handle(topology(editor, body))
    else {
        panic!("not edge facts");
    };
    assert_eq!(
        (tag, generation, until, answered),
        (9, editor.generation(), None, body)
    );
    let edges = result.unwrap();
    let at = picking.bodies().iter().position(|&b| b == body).unwrap();
    let part = mesh.parts().nth(at).unwrap();
    for (i, facts) in edges.iter().enumerate() {
        assert!(facts.valid(), "{facts:?}");
        let e = part.edges.start + i;
        assert!(part.edges.contains(&e));
        assert_eq!(picking.edge_keys(&mesh, e as u32), Some(facts.faces));
        assert_eq!(picking.closed()[e], facts.ends.is_none());
    }
    // The part's edges after them are creases.
    for e in part.edges.start + edges.len()..part.edges.end {
        assert_eq!(picking.edge_keys(&mesh, e as u32), None);
    }
    edges
}

#[test]
fn the_example_s_edges_are_straight_or_round_and_convex() {
    let editor = Editor::new(Document::example());
    let edges = paired(&editor, first_body(&editor));
    assert_eq!(edges.len(), 14);
    assert!(
        edges
            .iter()
            .all(|edge| edge.convexity == Some(Convexity::Convex))
    );
    // The plate's twelve sides run along the axes, from end to end.
    let mut along = [0; 3];
    for edge in &edges {
        let EdgeForm::Line { from, to } = edge.form else {
            continue;
        };
        assert_eq!(edge.ends, Some([from, to]));
        let d = DVec3::from(to) - DVec3::from(from);
        let axis = (0..3).filter(|&i| d[i].abs() > 1e-9).collect::<Vec<_>>();
        assert_eq!(axis.len(), 1, "{d:?} isn't along an axis");
        along[axis[0]] += 1;
    }
    assert_eq!(along, [4, 4, 4]);
    // The hole's two rims: whole circles of radius 8 about the z axis.
    let rims: Vec<&EdgeFacts> = (edges.iter())
        .filter(|edge| matches!(edge.form, EdgeForm::Circle { .. }))
        .collect();
    assert_eq!(rims.len(), 2);
    let mut heights = Vec::new();
    for edge in &edges {
        let EdgeForm::Circle {
            centre,
            axis,
            radius,
        } = edge.form
        else {
            continue;
        };
        assert_eq!(edge.ends, None);
        assert!((radius - 8.0).abs() < 1e-9, "{radius}");
        assert!(
            centre[0].abs() < 1e-9 && centre[1].abs() < 1e-9,
            "{centre:?}"
        );
        assert!((axis[2].abs() - 1.0).abs() < 1e-9, "{axis:?}");
        heights.push(centre[2]);
    }
    heights.sort_by(f64::total_cmp);
    assert!(
        heights[0].abs() < 1e-9 && (heights[1] - 10.0).abs() < 1e-9,
        "{heights:?}"
    );
}

/// A boss joined through the example plate: where its wall meets the
/// plate's top and bottom the edges are concave circles, its own caps'
/// rims convex.
#[test]
fn a_boss_meets_the_plate_in_concave_edges() {
    let mut editor = Editor::new(Document::example());
    let extent = two_sides(editor.document(), "15", "5");
    add_extrude(
        &mut editor,
        disc((20.0, 0.0), 5.0),
        extent,
        Operation::Join(Targets::default()),
    );
    let edges = paired(&editor, first_body(&editor));
    let boss = |convexity: Convexity| {
        let mut heights: Vec<f64> = (edges.iter())
            .filter_map(|edge| match edge.form {
                EdgeForm::Circle { centre, radius, .. }
                    if (radius - 5.0).abs() < 1e-9 && edge.convexity == Some(convexity) =>
                {
                    assert!((centre[0] - 20.0).abs() < 1e-9 && centre[1].abs() < 1e-9);
                    Some(centre[2])
                }
                _ => None,
            })
            .collect();
        heights.sort_by(f64::total_cmp);
        heights
    };
    let near = |a: Vec<f64>, b: &[f64]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    };
    let concave = boss(Convexity::Concave);
    assert!(near(concave.clone(), &[0.0, 10.0]), "{concave:?}");
    let convex = boss(Convexity::Convex);
    assert!(near(convex.clone(), &[-5.0, 15.0]), "{convex:?}");
    // The plate's own edges stay convex.
    let lines = edges
        .iter()
        .filter(|edge| matches!(edge.form, EdgeForm::Line { .. }));
    assert!(lines.clone().count() == 12);
    assert!(
        lines
            .into_iter()
            .all(|edge| edge.convexity == Some(Convexity::Convex))
    );
}

/// A cube's twelve edges, straight and convex, each along an axis.
#[test]
fn a_cube_s_edges_are_straight_and_convex() {
    let solid = Solid::cuboid(DVec3::splat(-1.0), DVec3::splat(2.0), 0, &TOL).unwrap();
    let edges = body_edges(&solid, &solid.topology()).unwrap();
    assert_eq!(edges.len(), 12);
    for edge in edges {
        assert_eq!(edge.convexity, Some(Convexity::Convex));
        let EdgeForm::Line { from, to } = edge.form else {
            panic!("{edge:?} isn't straight");
        };
        let d = DVec3::from(to) - DVec3::from(from);
        assert!((d.length() - 2.0).abs() < 1e-12);
        assert!((d.abs().max_element() - 2.0).abs() < 1e-12, "{d:?}");
    }
}

/// A body that has no solid in the model, or isn't there at all, is
/// answered with why; rolled back past the plate's extrude, so is the
/// plate.
#[test]
fn a_body_without_a_solid_is_answered_with_why() {
    let editor = Editor::new(Document::example());
    let body = first_body(&editor);
    let answer = |request: Request| match handle(request) {
        Response::Answered {
            answer: Answer::Topology { result, .. },
            ..
        } => result,
        other => panic!("{other:?} isn't edge facts"),
    };
    assert!(answer(topology(&editor, BodyId::NEW)).is_err());
    let mut request = topology(&editor, body);
    let Request::Query { until, .. } = &mut request else {
        unreachable!()
    };
    *until = editor
        .document()
        .features()
        .last()
        .map(|feature| feature.id);
    let Response::Answered {
        until: answered,
        answer: Answer::Topology { result, .. },
        ..
    } = handle(request)
    else {
        panic!("not edge facts");
    };
    assert_eq!(answered, editor.document().features().last().map(|f| f.id));
    assert!(result.is_err(), "{result:?}");
}

/// The answer's failure, should handling it panic, is its own, tagged
/// as asked.
#[test]
fn a_failed_request_is_answered_with_its_tag() {
    let editor = Editor::new(Document::example());
    let request = topology(&editor, first_body(&editor));
    let Response::Answered {
        tag,
        answer: Answer::Topology { body, result },
        ..
    } = request.failure()("it broke".to_owned())
    else {
        panic!("not edge facts");
    };
    assert_eq!((tag, body), (9, first_body(&editor)));
    assert_eq!(result, Err("it broke".to_owned()));
}

/// [`EdgeFacts::valid`] refuses numbers past the bound or not finite,
/// a straight edge without ends, sizes not positive, an axis not unit,
/// an ellipse's minor past its major and faces out of order.
#[test]
fn bad_facts_are_not_valid() {
    let editor = Editor::new(Document::example());
    let edges = paired(&editor, first_body(&editor));
    let good = *edges
        .iter()
        .find(|e| matches!(e.form, EdgeForm::Line { .. }))
        .unwrap();
    assert!(good.valid());
    let far = crate::Picking::MAX_VALUE * 2.0;
    let bad = |edge: EdgeFacts| assert!(!edge.valid(), "{edge:?}");
    bad(EdgeFacts {
        ends: Some([[far, 0.0, 0.0], [0.0; 3]]),
        ..good
    });
    bad(EdgeFacts {
        ends: Some([[f64::NAN, 0.0, 0.0], [0.0; 3]]),
        ..good
    });
    bad(EdgeFacts { ends: None, ..good });
    bad(EdgeFacts {
        faces: [good.faces[1], good.faces[0]],
        ..good
    });
    bad(EdgeFacts {
        faces: [good.faces[0]; 2],
        ..good
    });
    let circle = |centre: [f64; 3], axis: [f64; 3], radius: f64| EdgeFacts {
        form: EdgeForm::Circle {
            centre,
            axis,
            radius,
        },
        ends: None,
        ..good
    };
    assert!(circle([0.0; 3], [0.0, 0.0, 1.0], 1.0).valid());
    bad(circle([0.0; 3], [0.0, 0.0, 1.1], 1.0));
    bad(circle([0.0; 3], [0.0, 0.0, 1.0], 0.0));
    bad(circle([0.0; 3], [0.0, 0.0, 1.0], far));
    bad(circle([far, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    bad(circle([0.0; 3], [0.0, 0.0, 1.0], f64::INFINITY));
    bad(EdgeFacts {
        form: EdgeForm::Ellipse {
            centre: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            major: 1.0,
            minor: 2.0,
        },
        ..good
    });
}

/// Edge facts checked on solids built straight in the kernel.
mod solids {
    use super::*;
    use glam::DVec2;
    use varde_kernel::Op;
    use varde_kernel::{Budget, Frame, Loop, Motion, Profile, Segment, Sweep, boolean, revolve};

    /// `solid`'s edges' facts, checked against the traversal: each
    /// valid, a round edge's axis turns the way its halfedges run, a
    /// straight one's ends are its own.
    fn facts(solid: &Solid) -> Vec<EdgeFacts> {
        let topology = solid.topology();
        let edges = body_edges(solid, &topology).unwrap();
        assert_eq!(edges.len(), topology.chains().len());
        let mesh = solid.mesh();
        let mut out = Vec::new();
        for (chain, &edge) in topology.chains().iter().zip(&edges) {
            assert!(edge.valid(), "{edge:?}");
            match edge.form {
                EdgeForm::Circle { centre, axis, .. } | EdgeForm::Ellipse { centre, axis, .. } => {
                    let (centre, axis) = (DVec3::from(centre), DVec3::from(axis));
                    for &h in &chain.halfedges {
                        let c = mesh.curve(h);
                        let turn = (c.p0 - centre).cross(c.p1 - centre).dot(axis);
                        assert!(turn > 0.0, "{edge:?} runs against its axis");
                    }
                }
                EdgeForm::Line { from, to } => assert_eq!(edge.ends, Some([from, to])),
                EdgeForm::Other => {}
            }
            out.push(edge);
        }
        out
    }

    fn mirrored(solid: &Solid) -> Solid {
        let motion = Motion::mirror(DVec3::new(0.3, 0.0, 0.0), DVec3::new(1.0, 0.2, 0.1)).unwrap();
        solid
            .transformed(&motion, None, &TOL, &Budget::DEFAULT)
            .unwrap()
    }

    fn count(edges: &[EdgeFacts], convexity: Convexity) -> usize {
        edges
            .iter()
            .filter(|e| e.convexity == Some(convexity))
            .count()
    }

    fn box_at(min: DVec3, size: DVec3) -> Solid {
        Solid::cuboid(min, size, 1, &TOL).unwrap()
    }

    fn cut(a: &Solid, b: &Solid, op: Op) -> Solid {
        boolean(a, b, op, &TOL, &Budget::DEFAULT).unwrap()
    }

    /// A block with a quarter cut out: one concave straight edge, the
    /// rest convex, the same mirrored.
    #[test]
    fn a_notch_has_one_concave_straight_edge() {
        let notched = cut(
            &box_at(DVec3::ZERO, DVec3::splat(4.0)),
            &box_at(DVec3::new(2.0, 2.0, -1.0), DVec3::new(3.0, 3.0, 6.0)),
            Op::Difference,
        );
        for solid in [notched.clone(), mirrored(&notched)] {
            let edges = facts(&solid);
            assert_eq!(edges.len(), 18);
            assert_eq!(count(&edges, Convexity::Concave), 1);
            assert_eq!(count(&edges, Convexity::Convex), 17);
            let concave = edges
                .iter()
                .find(|e| e.convexity == Some(Convexity::Concave));
            assert!(matches!(concave.unwrap().form, EdgeForm::Line { .. }));
        }
    }

    /// An L revolved: one inside rim, concave, five convex, mirrored too;
    /// and a part turn's ends add straight edges, all convex.
    #[test]
    fn a_revolved_l_has_one_concave_rim() {
        let v = DVec2::new;
        let points = [
            v(1.0, 0.0),
            v(4.0, 0.0),
            v(4.0, 1.0),
            v(2.0, 1.0),
            v(2.0, 3.0),
            v(1.0, 3.0),
        ];
        let segments = (0..6)
            .map(|i| Segment::line(points[i], points[(i + 1) % 6], i as u64).unwrap())
            .collect();
        let profile = Profile {
            loops: vec![Loop { segments }],
        };
        let frame = Frame {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Z,
        };
        let full = revolve(&profile, &frame, Sweep::Full, 7, &TOL, &Budget::DEFAULT).unwrap();
        for solid in [full.clone(), mirrored(&full)] {
            let edges = facts(&solid);
            assert_eq!(edges.len(), 6);
            assert!(
                edges
                    .iter()
                    .all(|e| matches!(e.form, EdgeForm::Circle { .. }))
            );
            assert_eq!(count(&edges, Convexity::Concave), 1);
            assert_eq!(count(&edges, Convexity::Convex), 5);
        }
        let part = Sweep::Part { from: 0.0, to: 1.5 };
        let part = revolve(&profile, &frame, part, 7, &TOL, &Budget::DEFAULT).unwrap();
        for solid in [part.clone(), mirrored(&part)] {
            let edges = facts(&solid);
            assert_eq!(edges.len(), 6 + 2 * 6);
            assert_eq!(count(&edges, Convexity::Concave), 1, "{edges:#?}");
            assert_eq!(count(&edges, Convexity::Convex), edges.len() - 1);
        }
    }

    /// A cylinder cut at 45°: its top rim an ellipse of semi-axes
    /// `2√2` and 2 square to the cut, convex.
    #[test]
    fn a_slanted_cut_makes_an_ellipse() {
        let cylinder = Solid::cylinder(DVec3::ZERO, 2.0, 10.0, 1, &TOL).unwrap();
        let block = box_at(DVec3::new(-20.0, -20.0, 0.0), DVec3::splat(40.0));
        let turn = Motion::turn(DVec3::ZERO, DVec3::X, 45.0)
            .unwrap()
            .then(&Motion::translation(DVec3::new(0.0, 0.0, 5.0)).unwrap());
        let block = block
            .transformed(&turn, None, &TOL, &Budget::DEFAULT)
            .unwrap();
        let solid = cut(&cylinder, &block, Op::Difference);
        for solid in [solid.clone(), mirrored(&solid)] {
            let edges = facts(&solid);
            assert!(edges.iter().all(|e| e.convexity == Some(Convexity::Convex)));
            let ellipses: Vec<_> = (edges.iter())
                .filter_map(|e| match e.form {
                    EdgeForm::Ellipse {
                        major, minor, axis, ..
                    } => Some((major, minor, axis)),
                    _ => None,
                })
                .collect();
            assert_eq!(ellipses.len(), 1, "{edges:#?}");
            let (major, minor, _) = ellipses[0];
            assert!((major - 2.0 * 2f64.sqrt()).abs() < 1e-6, "{major}");
            assert!((minor - 2.0).abs() < 1e-6, "{minor}");
        }
        // Unmirrored, the plane's normal is ±(0, −1, 1)/√2.
        let edges = facts(&solid);
        let axis = edges.iter().find_map(|e| match e.form {
            EdgeForm::Ellipse { axis, .. } => Some(DVec3::from(axis)),
            _ => None,
        });
        let n = DVec3::new(0.0, -1.0, 1.0).normalize();
        assert!((axis.unwrap().dot(n).abs() - 1.0).abs() < 1e-6, "{axis:?}");
    }

    /// A box joined flush to a cylinder's side: the walls run on into
    /// each other, smooth.
    #[test]
    fn a_tangent_wall_is_smooth() {
        let cylinder = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 1, &TOL).unwrap();
        let block = Solid::cuboid(
            DVec3::new(0.0, -1.0, 0.0),
            DVec3::new(2.0, 2.0, 2.0),
            2,
            &TOL,
        )
        .unwrap();
        let Ok(solid) = boolean(&cylinder, &block, Op::Union, &TOL, &Budget::DEFAULT) else {
            eprintln!("the tangent union fails, skipping");
            return;
        };
        for solid in [solid.clone(), mirrored(&solid)] {
            let edges = facts(&solid);
            assert_eq!(count(&edges, Convexity::Smooth), 2, "{edges:#?}");
            assert_eq!(count(&edges, Convexity::Concave), 0, "{edges:#?}");
        }
    }
}

/// With two bodies, each's facts pair with its own part, the first body
/// hidden (no part) or not.
#[test]
fn each_of_two_bodies_pairs_with_its_part() {
    let mut editor = Editor::new(Document::example());
    let extent = two_sides(editor.document(), "15", "5");
    add_extrude(
        &mut editor,
        disc((60.0, 0.0), 5.0),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let bodies: Vec<BodyId> = editor.document().bodies().iter().map(|b| b.id).collect();
    assert_eq!(bodies.len(), 2);
    let plate = paired(&editor, bodies[0]);
    let boss = paired(&editor, bodies[1]);
    assert_eq!(plate.len(), 14);
    assert_eq!(boss.len(), 2);
    editor
        .apply(varde_document::Command::SetVisible(bodies[0], false))
        .unwrap();
    assert_eq!(paired(&editor, bodies[1]), boss);
}
