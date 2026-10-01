use glam::{DVec2, Vec3};
use varde_document::{Command, Document, Editor, Operation, OriginPlane, Plane, Sketch};
use varde_kernel::{Solid, Tolerance};
use varde_sketch::{CIRCLE_SEGMENTS, Constraint, Curve};

use super::*;
use crate::history::tests::example_extrude;

fn regenerate(editor: &Editor, exclude: Option<FeatureId>) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude,
        draft: None,
    }
}

/// A sketch on the XZ plane holding a line from
/// (0, 0) to (2, 1), a construction circle, and a quarter arc, and the
/// sketch's id.
pub(crate) fn sketched() -> (Editor, FeatureId) {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::ZERO).unwrap();
    let end = sketch.add_point(DVec2::new(2.0, 1.0)).unwrap();
    sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    let center = sketch.add_point(DVec2::new(5.0, 5.0)).unwrap();
    let radius = 1.0;
    sketch
        .add_curve(Curve::Circle { center, radius }, true)
        .unwrap();
    let arc_start = sketch.add_point(DVec2::new(6.0, 5.0)).unwrap();
    let arc_end = sketch.add_point(DVec2::new(5.0, 6.0)).unwrap();
    sketch
        .add_curve(
            Curve::Arc {
                center,
                start: arc_start,
                end: arc_end,
            },
            false,
        )
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    (editor, feature)
}

#[test]
fn regenerate_tessellates_the_snapshot() {
    let (editor, _) = sketched();
    let Response::Regenerated {
        generation,
        exclude,
        draft,
        mesh,
        sketches,
        unsolved,
        failed,
        bodies,
    } = handle(regenerate(&editor, None))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(exclude, None);
    assert_eq!(draft, None);
    // Sketches make no bodies.
    assert!(
        evaluate(editor.document(), &mut Cache::default())
            .bodies
            .is_empty()
    );
    assert_eq!(*mesh, RenderMesh::default());
    assert_eq!(sketches.ends().len(), 2);
    assert!(unsolved.is_empty());
    assert!(failed.is_empty());
    assert!(bodies.is_empty());
}

fn cuboid(min: f64, size: f64) -> Solid {
    Solid::cuboid(
        glam::DVec3::splat(min),
        glam::DVec3::splat(size),
        0,
        &Tolerance::DEFAULT,
    )
    .unwrap()
}

/// `solids` as the history's solids of `bodies`, filed under keys of
/// their own.
fn made(solids: impl IntoIterator<Item = (BodyId, Solid)>) -> Evaluation {
    let bodies = solids
        .into_iter()
        .enumerate()
        .map(|(k, (body, solid))| BodySolid {
            body,
            solid: Arc::new(solid),
            key: Keyer::new("test").number(k as u64).finish(),
        })
        .collect();
    Evaluation {
        bodies,
        ..Evaluation::default()
    }
}

/// The example plate ("Body 1", shown) and, if `hidden_too`, a second
/// extrude of it making "Body 2", hidden.
fn with_bodies(hidden_too: bool) -> Document {
    let mut editor = Editor::new(Document::example());
    if hidden_too {
        let extrude = Extrude {
            operation: Operation::NewBody(BodyId::NEW),
            ..example_extrude(editor.document())
        };
        editor
            .apply(editor.document().add_extrude(extrude))
            .unwrap();
        let body = editor.document().bodies()[1].id;
        editor.apply(Command::SetVisible(body, false)).unwrap();
    }
    editor.document().clone()
}

#[test]
fn solids_are_drawn_into_one_mesh() {
    let document = with_bodies(true);
    let body = document.bodies()[0].id;
    let (a, b) = (cuboid(0.0, 1.0), cuboid(3.0, 2.0));
    let display = Display::new(&Tolerance::DEFAULT);
    let mut both = a.tessellate(&display).unwrap();
    both.append(&b.tessellate(&display).unwrap()).unwrap();
    let evaluation = made([(body, a), (body, b)]);
    let mesh = tessellate(&document, &evaluation, &mut Cache::default()).unwrap();
    assert_eq!(*mesh, both);
    assert_eq!(mesh.triangle_count(), 24);
    assert_eq!(
        mesh.bounds().unwrap(),
        varde_kernel::Aabb {
            min: Vec3::ZERO,
            max: Vec3::splat(5.0)
        }
    );
    let none = tessellate(&document, &made([]), &mut Cache::default()).unwrap();
    assert_eq!(*none, RenderMesh::default());
}

#[test]
fn only_the_solids_of_shown_bodies_are_drawn() {
    let (both, one) = (with_bodies(true), with_bodies(false));
    let ids = |document: &Document| -> Vec<BodyId> {
        document.bodies().iter().map(|body| body.id).collect()
    };
    let [shown, hidden] = ids(&both)[..] else {
        panic!("two bodies");
    };
    assert!(!both.body(hidden).unwrap().visible);
    assert_eq!(ids(&one), [shown]);

    let solids = made([(shown, cuboid(0.0, 1.0)), (hidden, cuboid(3.0, 2.0))]);
    let display = Display::new(&Tolerance::DEFAULT);
    let alone = solids.bodies[0].solid.tessellate(&display).unwrap();
    // Hidden, or not in the document at all.
    let mut cache = Cache::default();
    assert_eq!(*tessellate(&both, &solids, &mut cache).unwrap(), alone);
    assert_eq!(*tessellate(&one, &solids, &mut cache).unwrap(), alone);
}

/// Solids are drawn to the document's tolerance: a coarser one gives a
/// cylinder fewer triangles.
#[test]
fn solids_are_drawn_to_the_document_s_tolerance() {
    let mut editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let tolerance = Tolerance::DEFAULT;
    let cylinder = Solid::cylinder(glam::DVec3::ZERO, 10.0, 1.0, 0, &tolerance).unwrap();
    let solids = made([(body, cylinder)]);
    // One cache: the mesh is filed by the tolerance too.
    let mut cache = Cache::default();
    let fine = tessellate(editor.document(), &solids, &mut cache).unwrap();
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    editor.apply(Command::SetTolerance(coarse)).unwrap();
    let drawn = tessellate(editor.document(), &solids, &mut cache).unwrap();
    assert_eq!(
        *drawn,
        solids.bodies[0]
            .solid
            .tessellate(&Display::new(&coarse))
            .unwrap()
    );
    assert!(drawn.triangle_count() < fine.triangle_count());
}

/// Adds a sketch on XY that doesn't solve, as a file could hold: a line
/// between two fixed points held horizontal, though they're at different
/// heights. Its id.
pub(crate) fn unsolvable(editor: &mut Editor) -> FeatureId {
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::ZERO).unwrap();
    let end = sketch.add_point(DVec2::new(4.0, 1.0)).unwrap();
    sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    for constraint in [
        Constraint::Fix(start),
        Constraint::Fix(end),
        Constraint::HorizontalPoints(start, end),
    ] {
        sketch.add_constraint(constraint).unwrap();
    }
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    feature
}

#[test]
fn sketches_that_do_not_solve_are_named() {
    let (mut editor, solved) = sketched();
    let unsolved = unsolvable(&mut editor);
    let Response::Regenerated {
        unsolved: named, ..
    } = handle(regenerate(&editor, None))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(named, [unsolved]);
    assert_ne!(solved, unsolved);
}

#[test]
fn regenerate_flattens_the_sketches() {
    let (editor, feature) = sketched();
    let Response::Regenerated { mesh, sketches, .. } = handle(regenerate(&editor, None)) else {
        panic!("regeneration failed");
    };
    assert_eq!(mesh.triangle_count(), 0);
    assert_eq!(
        *sketches,
        flatten_sketches(editor.document(), None).unwrap()
    );
    assert_eq!(sketches.ends().len(), 2);

    // The sketch being edited is left out, and the answer says so.
    let Response::Regenerated {
        exclude, sketches, ..
    } = handle(regenerate(&editor, Some(feature)))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(exclude, Some(feature));
    assert_eq!(*sketches, RenderLines::default());
}

#[test]
fn sketches_are_placed_on_their_planes() {
    let (editor, _) = sketched();
    let lines = flatten_sketches(editor.document(), None).unwrap();
    // The construction circle is left out.
    let polylines: Vec<_> = lines.polylines().collect();
    assert_eq!(polylines.len(), 2);
    // On XZ, the sketch's y is the world's Z.
    assert_eq!(polylines[0], [[0.0; 3], [2.0, 0.0, 1.0]]);
    let arc = polylines[1];
    assert_eq!(arc.len(), CIRCLE_SEGMENTS / 4 + 1);
    assert_eq!(arc[0], [6.0, 0.0, 5.0]);
    assert_eq!(arc[arc.len() - 1], [5.0, 0.0, 6.0]);
    for point in arc {
        let offset = Vec3::from(*point) - Vec3::new(5.0, 0.0, 5.0);
        assert!((offset.length() - 1.0).abs() < 1e-6, "{point:?}");
        assert_eq!(offset.y, 0.0);
    }
}

#[test]
fn hidden_and_excluded_sketches_are_left_out() {
    let (mut editor, feature) = sketched();
    // A second sketch, on XY, with a line.
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let second = editor.document().features()[1].id;
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::new(1.0, 2.0)).unwrap();
    let end = sketch.add_point(DVec2::new(3.0, 4.0)).unwrap();
    sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    editor
        .apply(Command::SetSketch {
            feature: second,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let only_second = [[[1.0, 2.0, 0.0], [3.0, 4.0, 0.0]]];

    let lines = flatten_sketches(editor.document(), Some(feature)).unwrap();
    assert_eq!(lines.polylines().collect::<Vec<_>>(), only_second);
    assert_eq!(
        flatten_sketches(editor.document(), None)
            .unwrap()
            .ends()
            .len(),
        3
    );

    editor
        .apply(Command::SetFeatureVisible(feature, false))
        .unwrap();
    let lines = flatten_sketches(editor.document(), None).unwrap();
    assert_eq!(lines.polylines().collect::<Vec<_>>(), only_second);
    let lines = flatten_sketches(editor.document(), Some(second)).unwrap();
    assert_eq!(lines, RenderLines::default());
}

#[test]
fn sketches_far_out_stay_within_the_lines_bound() {
    // The largest arc a checked sketch can hold, from one corner of the
    // coordinate limit round the far side of another.
    let max = f64::from(varde_document::MAX_COORD);
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::YZ)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let center = sketch.add_point(DVec2::new(max, max)).unwrap();
    let start = sketch.add_point(DVec2::new(-max, -max)).unwrap();
    let end = sketch.add_point(DVec2::new(-max, -max + 1.0)).unwrap();
    sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let lines = flatten_sketches(editor.document(), None).unwrap();
    assert_eq!(lines.points().len(), CIRCLE_SEGMENTS + 1);
}

#[test]
fn lines_are_drawn_without_the_ends_a_chamfer_cuts_off() {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let corner = sketch.add_point(DVec2::ZERO).unwrap();
    let along = sketch.add_point(DVec2::new(20.0, 0.0)).unwrap();
    let up = sketch.add_point(DVec2::new(0.0, 10.0)).unwrap();
    let a = sketch
        .add_curve(
            Curve::Line {
                start: corner,
                end: along,
            },
            false,
        )
        .unwrap();
    let b = sketch
        .add_curve(
            Curve::Line {
                start: up,
                end: corner,
            },
            false,
        )
        .unwrap();
    // A chamfer cutting 1 off each.
    let start = sketch.add_point(DVec2::new(1.0, 0.0)).unwrap();
    let end = sketch.add_point(DVec2::new(0.0, 1.0)).unwrap();
    let chamfer = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    sketch.curve_mut(chamfer).unwrap().corner = Some(varde_sketch::Corner {
        a,
        b,
        at: corner,
        equal: true,
    });
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let lines = flatten_sketches(editor.document(), None).unwrap();
    assert_eq!(
        lines.polylines().collect::<Vec<_>>(),
        [
            [[1.0, 0.0, 0.0], [20.0, 0.0, 0.0]],
            [[0.0, 10.0, 0.0], [0.0, 1.0, 0.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        ]
    );
}

fn regenerate_with(editor: &Editor, draft: Option<Draft>) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft,
    }
}

/// The model of a regeneration that worked.
struct Answer {
    draft: Option<Drafted>,
    mesh: Arc<RenderMesh>,
    failed: Vec<(FeatureId, String)>,
    bodies: Vec<(BodyId, varde_kernel::Aabb)>,
}

fn answered(response: Response) -> Answer {
    let Response::Regenerated {
        draft,
        mesh,
        failed,
        bodies,
        ..
    } = response
    else {
        panic!("regeneration failed: {response:?}");
    };
    Answer {
        draft,
        mesh,
        failed,
        bodies,
    }
}

#[test]
fn the_example_plate_regenerates_and_draws() {
    let editor = Editor::new(Document::example());
    let answer = answered(handle(regenerate(&editor, None)));
    assert!(answer.failed.is_empty(), "{:?}", answer.failed);
    assert!(answer.mesh.triangle_count() > 0);
    assert!(!answer.mesh.edges().is_empty());
    let body = editor.document().bodies()[0].id;
    let bounds = varde_kernel::Aabb {
        min: Vec3::new(-30.0, -20.0, 0.0),
        max: Vec3::new(30.0, 20.0, 10.0),
    };
    assert_eq!(answer.bodies, [(body, bounds)]);
    assert_eq!(answer.mesh.bounds(), Some(bounds));
}

/// A draft of revision `revision` making a new body from the example's
/// regions, `text` long, flipped.
fn new_body_draft(document: &Document, revision: u64, text: &str) -> Draft {
    Draft {
        revision,
        feature: None,
        extrude: Extrude {
            extent: varde_document::Extent::OneSide(crate::history::tests::length(document, text)),
            flip: true,
            operation: Operation::NewBody(BodyId::NEW),
            ..example_extrude(document)
        },
    }
}

#[test]
fn a_draft_is_answered_as_if_applied() {
    let editor = Editor::new(Document::example());
    let draft = new_body_draft(editor.document(), 7, "3");
    let answer = answered(handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(
        answer.draft,
        Some(Drafted {
            revision: 7,
            error: None,
            // A new body isn't tested for touching.
            touched: None,
        })
    );
    assert!(answer.failed.is_empty());
    let [(first, _), (_, below)] = answer.bodies[..] else {
        panic!("two bodies");
    };
    assert_eq!(first, editor.document().bodies()[0].id);
    assert_eq!((below.min.z, below.max.z), (-3.0, 0.0));
    assert_eq!(answer.mesh.bounds().unwrap().min.z, -3.0);

    // An extrude edited: the body it makes changes.
    let feature = editor.document().features()[1].id;
    let draft = Draft {
        revision: 8,
        feature: Some(feature),
        extrude: Extrude {
            flip: false,
            ..new_body_draft(editor.document(), 0, "20").extrude
        },
    };
    let answer = answered(handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(answer.draft.unwrap().error, None);
    let [(_, bounds)] = answer.bodies[..] else {
        panic!("one body");
    };
    assert_eq!((bounds.min.z, bounds.max.z), (0.0, 20.0));
}

#[test]
fn a_failing_draft_leaves_the_model_as_it_was() {
    let editor = Editor::new(Document::example());
    let committed = answered(handle(regenerate(&editor, None)));
    // A join taking out the only body it touches.
    let body = editor.document().bodies()[0].id;
    let join = Draft {
        extrude: Extrude {
            operation: Operation::Join(varde_document::Targets {
                excluded: vec![body],
            }),
            ..new_body_draft(editor.document(), 0, "3").extrude
        },
        ..new_body_draft(editor.document(), 3, "3")
    };
    // One the document refuses: its sketch is the extrude.
    let mut refused = new_body_draft(editor.document(), 4, "3");
    refused.extrude.sketch = editor.document().features()[1].id;
    // One editing a feature that isn't an extrude.
    let sketch = Draft {
        feature: Some(editor.document().features()[0].id),
        ..new_body_draft(editor.document(), 5, "3")
    };
    for (draft, error, touched) in [
        (
            join,
            "it doesn't touch any body not taken out of it".to_owned(),
            // Tested, touching nothing.
            Some(vec![]),
        ),
        (
            sketch,
            "the draft's feature isn't an extrude".to_owned(),
            // Refused before any tool: not tested.
            None,
        ),
        (
            refused.clone(),
            {
                let command = editor.document().add_extrude(refused.extrude.clone());
                let mut probe = Editor::new(editor.document().clone());
                probe.apply(command).unwrap_err().to_string()
            },
            None,
        ),
    ] {
        let revision = draft.revision;
        let answer = answered(handle(regenerate_with(&editor, Some(draft))));
        assert_eq!(
            answer.draft,
            Some(Drafted {
                revision,
                error: Some(error),
                touched,
            })
        );
        assert_eq!(answer.mesh, committed.mesh);
        assert_eq!(answer.bodies, committed.bodies);
        assert!(answer.failed.is_empty());
    }
}

#[test]
fn dragging_a_draft_reruns_only_the_draft() {
    let editor = Editor::new(Document::example());
    let mut regenerator = Regenerator::default();
    let draft = new_body_draft(editor.document(), 1, "3");
    answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
    let (_, before) = regenerator.cache().counts();
    let draft = new_body_draft(editor.document(), 2, "4");
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(answer.draft.unwrap().revision, 2);
    // The draft's solid and its mesh.
    assert_eq!(regenerator.cache().counts().1, before + 2);
}

#[test]
fn a_cut_draft_lists_what_it_touches_and_is_answered_from_the_cache() {
    // The pocket's sketch committed, its cut a draft.
    let mut editor = Editor::new(Document::example());
    let mut probe = Editor::new(editor.document().clone());
    let pocket = crate::history::tests::add_pocket(&mut probe);
    let [.., sketch, cut] = probe.document().features() else {
        unreachable!()
    };
    let (
        FeatureKind::Sketch {
            plane,
            sketch: drawn,
        },
        FeatureKind::Extrude(extrude),
    ) = (&sketch.kind, &cut.kind)
    else {
        unreachable!()
    };
    assert_eq!(cut.id, pocket);
    editor.apply(editor.document().add_sketch(*plane)).unwrap();
    let feature = editor.document().features()[2].id;
    assert_eq!(feature, sketch.id);
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(drawn.clone()),
        })
        .unwrap();
    let body = editor.document().bodies()[0].id;
    let mut regenerator = Regenerator::default();
    let committed = answered(regenerator.handle(regenerate(&editor, None)));
    let (_, before) = regenerator.cache().counts();

    let mut draft = Draft {
        revision: 1,
        feature: None,
        extrude: extrude.clone(),
    };
    let cut = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(
        cut.draft,
        Some(Drafted {
            revision: 1,
            error: None,
            touched: Some(vec![body]),
        })
    );
    assert_ne!(cut.mesh, committed.mesh);
    // Only the draft's tool, whether it touches the plate, the cut and
    // its mesh were worked out: the rest was found.
    let (_, worked) = regenerator.cache().counts();
    assert_eq!(worked, before + 4);

    // Taking the plate out: the tool is found, whether it touches isn't
    // asked, and the model's mesh is the committed one's, kept in the
    // scene slot: nothing is worked out.
    let mut out = draft.clone();
    out.revision = 2;
    out.extrude.operation = Operation::Cut(varde_document::Targets {
        excluded: vec![body],
    });
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(out))));
    let drafted = answer.draft.unwrap();
    assert_eq!(drafted.touched, Some(vec![]));
    assert!(drafted.error.is_some());
    assert!(Arc::ptr_eq(&answer.mesh, &committed.mesh));
    assert_eq!(regenerator.cache().counts().1, worked);

    // Putting it back finds whether it touches and the cut, kept while
    // it was out, and the scene the cut made: nothing is worked out.
    draft.revision = 3;
    let back = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(back.draft.unwrap().error, None);
    assert!(Arc::ptr_eq(&back.mesh, &cut.mesh));
    assert_eq!(regenerator.cache().counts().1, worked);

    // Dragging the pocket deeper: only its tool, touching, cut and mesh.
    draft.revision = 4;
    draft.extrude.extent = crate::history::tests::two_sides(editor.document(), "5", "1");
    let deeper = answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(deeper.draft.unwrap().error, None);
    assert_eq!(regenerator.cache().counts().1, worked + 4);
}

/// The model's mesh is joined once per scene: requests whose shown bodies
/// and tolerance didn't change are answered with the same `Arc`, which
/// the renderer then doesn't upload again; a changed scene gets a new
/// one, with the right content.
#[test]
fn an_unchanged_model_is_answered_with_the_same_mesh() {
    let mut editor = Editor::new(Document::example());
    let mut regenerator = Regenerator::default();
    let ask = |regenerator: &mut Regenerator, request| answered(regenerator.handle(request));
    let first = ask(&mut regenerator, regenerate(&editor, None)).mesh;
    assert_eq!(regenerator.cache().joins(), 1);
    let plate = editor.document().bodies()[0].id;
    let display = Display::new(&editor.document().tolerance());
    let drawn = |evaluation: &Evaluation| {
        let mut mesh = RenderMesh::default();
        for made in &evaluation.bodies {
            mesh.append(&made.solid.tessellate(&display).unwrap())
                .unwrap();
        }
        mesh
    };
    assert_eq!(
        *first,
        drawn(&evaluate(editor.document(), &mut Cache::default()))
    );

    // The same request again.
    let again = ask(&mut regenerator, regenerate(&editor, None)).mesh;
    assert!(Arc::ptr_eq(&again, &first));

    // A new sketch no body depends on, at a new generation, then edited,
    // and left out of the lines.
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let after = ask(&mut regenerator, regenerate(&editor, None)).mesh;
    assert!(Arc::ptr_eq(&after, &first));
    let mut drawn_sketch = Sketch::default();
    let start = drawn_sketch.add_point(DVec2::ZERO).unwrap();
    let end = drawn_sketch.add_point(DVec2::new(1.0, 2.0)).unwrap();
    drawn_sketch
        .add_curve(Curve::Line { start, end }, false)
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn_sketch),
        })
        .unwrap();
    let edited = ask(&mut regenerator, regenerate(&editor, Some(sketch))).mesh;
    assert!(Arc::ptr_eq(&edited, &first));
    // A sketch hidden.
    editor
        .apply(Command::SetFeatureVisible(sketch, false))
        .unwrap();
    let hidden_sketch = ask(&mut regenerator, regenerate(&editor, None)).mesh;
    assert!(Arc::ptr_eq(&hidden_sketch, &first));
    assert_eq!(regenerator.cache().joins(), 1);

    // A draft that fails: the committed model's mesh.
    let join = Draft {
        extrude: Extrude {
            operation: Operation::Join(varde_document::Targets {
                excluded: vec![plate],
            }),
            ..new_body_draft(editor.document(), 0, "3").extrude
        },
        ..new_body_draft(editor.document(), 1, "3")
    };
    let failing = ask(&mut regenerator, regenerate_with(&editor, Some(join)));
    assert!(failing.draft.unwrap().error.is_some());
    assert!(Arc::ptr_eq(&failing.mesh, &first));
    assert_eq!(regenerator.cache().joins(), 1);

    // A draft that works: a new mesh with the new body in it, and the
    // committed one found again after it.
    let draft = new_body_draft(editor.document(), 2, "3");
    let mut probe = Editor::new(editor.document().clone());
    probe
        .apply(probe.document().add_extrude(draft.extrude.clone()))
        .unwrap();
    let drafted = ask(&mut regenerator, regenerate_with(&editor, Some(draft))).mesh;
    assert!(!Arc::ptr_eq(&drafted, &first));
    assert_eq!(
        *drafted,
        drawn(&evaluate(probe.document(), &mut Cache::default()))
    );
    assert_eq!(regenerator.cache().joins(), 2);
    let back = ask(&mut regenerator, regenerate(&editor, None)).mesh;
    assert!(Arc::ptr_eq(&back, &first));
    assert_eq!(regenerator.cache().joins(), 2);

    // The plate hidden: an empty mesh, the same one asked again.
    editor.apply(Command::SetVisible(plate, false)).unwrap();
    let hidden = ask(&mut regenerator, regenerate(&editor, None)).mesh;
    assert!(!Arc::ptr_eq(&hidden, &first));
    assert_eq!(*hidden, RenderMesh::default());
    let still = ask(&mut regenerator, regenerate(&editor, None)).mesh;
    assert!(Arc::ptr_eq(&still, &hidden));
    assert_eq!(regenerator.cache().joins(), 3);

    // Shown again, right after: the scene before is found.
    editor.apply(Command::SetVisible(plate, true)).unwrap();
    let (_, worked) = regenerator.cache().counts();
    let shown = ask(&mut regenerator, regenerate(&editor, None)).mesh;
    assert!(Arc::ptr_eq(&shown, &first));
    assert_eq!(regenerator.cache().counts().1, worked);
    assert_eq!(regenerator.cache().joins(), 3);

    // A coarser tolerance: a new mesh, drawn to it.
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    editor.apply(Command::SetTolerance(coarse)).unwrap();
    let coarser = ask(&mut regenerator, regenerate(&editor, None)).mesh;
    assert!(!Arc::ptr_eq(&coarser, &first));
    let display = Display::new(&coarse);
    let evaluation = evaluate(editor.document(), &mut Cache::default());
    assert_eq!(
        *coarser,
        evaluation.bodies[0].solid.tessellate(&display).unwrap()
    );
    assert_eq!(regenerator.cache().joins(), 4);
}

/// A scene found keeps its bodies' meshes in the cache, so the next
/// scene that changes one body joins the others' without drawing them
/// again.
#[test]
fn a_scene_found_keeps_its_bodies_meshes() {
    let mut editor = Editor::new(Document::example());
    let draft = new_body_draft(editor.document(), 0, "3");
    editor
        .apply(editor.document().add_extrude(draft.extrude))
        .unwrap();
    let mut regenerator = Regenerator::default();
    let first = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    for _ in 0..3 {
        let again = answered(regenerator.handle(regenerate(&editor, None))).mesh;
        assert!(Arc::ptr_eq(&again, &first));
    }
    // The second body hidden: the plate's mesh is found, not drawn.
    let second = editor.document().bodies()[1].id;
    editor.apply(Command::SetVisible(second, false)).unwrap();
    let (_, worked) = regenerator.cache().counts();
    let plate = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    assert_eq!(regenerator.cache().counts().1, worked);
    assert_eq!(regenerator.cache().joins(), 2);
    let evaluation = evaluate(editor.document(), &mut Cache::default());
    let display = Display::new(&editor.document().tolerance());
    assert_eq!(
        *plate,
        evaluation.bodies[0].solid.tessellate(&display).unwrap()
    );
}

/// The scene's key is the shown bodies' meshes in order: the same solid
/// shown twice, or two bodies swapped, is another scene; a hidden body in
/// the middle is left out of it.
#[test]
fn the_scene_key_holds_each_shown_body_in_order() {
    let document = with_bodies(true);
    let [shown, hidden] = [document.bodies()[0].id, document.bodies()[1].id];
    let solids = [cuboid(0.0, 1.0), cuboid(3.0, 2.0), cuboid(9.0, 1.0)];
    // Each solid with a key of its own, as evaluating gives them.
    let scene = |bodies: &[(BodyId, usize)]| Evaluation {
        bodies: bodies
            .iter()
            .map(|&(body, solid)| BodySolid {
                body,
                solid: Arc::new(solids[solid].clone()),
                key: Keyer::new("test").number(solid as u64).finish(),
            })
            .collect(),
        ..Evaluation::default()
    };
    let display = Display::new(&Tolerance::DEFAULT);
    let joined = |shown: &[usize]| {
        let mut mesh = RenderMesh::default();
        for &solid in shown {
            mesh.append(&solids[solid].tessellate(&display).unwrap())
                .unwrap();
        }
        mesh
    };
    let mut cache = Cache::default();
    let one = tessellate(&document, &scene(&[(shown, 0)]), &mut cache).unwrap();
    assert_eq!(*one, joined(&[0]));
    let twice = tessellate(&document, &scene(&[(shown, 0), (shown, 0)]), &mut cache).unwrap();
    assert_eq!(*twice, joined(&[0, 0]));
    // A hidden body between two shown ones, and the two swapped.
    let ab = scene(&[(shown, 0), (hidden, 2), (shown, 1)]);
    let ab = tessellate(&document, &ab, &mut cache).unwrap();
    assert_eq!(*ab, joined(&[0, 1]));
    let ba = scene(&[(shown, 1), (hidden, 2), (shown, 0)]);
    let ba = tessellate(&document, &ba, &mut cache).unwrap();
    assert_eq!(*ba, joined(&[1, 0]));
    // Without the hidden body, the same scene as with it.
    let without = tessellate(&document, &scene(&[(shown, 1), (shown, 0)]), &mut cache).unwrap();
    assert!(Arc::ptr_eq(&without, &ba));
    assert_eq!(cache.joins(), 4);
    // No bodies: one empty mesh, found again.
    let none = tessellate(&document, &scene(&[]), &mut cache).unwrap();
    let again = tessellate(&document, &scene(&[(hidden, 2)]), &mut cache).unwrap();
    assert!(Arc::ptr_eq(&none, &again));
    assert_eq!(*none, RenderMesh::default());
    assert_eq!(cache.joins(), 5);
}

/// Without drafts, the scene slot holds the last two scenes used,
/// whatever the requests in between: a third scene pushes out the least
/// recently used.
#[test]
fn the_scene_slot_holds_the_last_two_scenes_used() {
    let document = with_bodies(false);
    let body = document.bodies()[0].id;
    let scenes = [
        made([(body, cuboid(0.0, 1.0))]),
        made([]),
        Evaluation {
            bodies: vec![BodySolid {
                body,
                solid: Arc::new(cuboid(2.0, 1.0)),
                key: Keyer::new("test").number(7).finish(),
            }],
            ..Evaluation::default()
        },
    ];
    let mut cache = Cache::default();
    let ask = |cache: &mut Cache, scene: usize| {
        cache.begin();
        tessellate(&document, &scenes[scene], cache).unwrap()
    };
    let a = ask(&mut cache, 0);
    let b = ask(&mut cache, 1);
    for _ in 0..3 {
        assert!(Arc::ptr_eq(&ask(&mut cache, 0), &a));
        assert!(Arc::ptr_eq(&ask(&mut cache, 1), &b));
    }
    assert_eq!(cache.joins(), 2);
    // A third scene: `a`, used least recently, goes.
    ask(&mut cache, 2);
    assert!(Arc::ptr_eq(&ask(&mut cache, 1), &b));
    let later = ask(&mut cache, 0);
    assert!(!Arc::ptr_eq(&later, &a));
    assert_eq!(*later, *a);
    assert_eq!(cache.joins(), 4);
}

/// However long a draft is dragged, the committed model's scene stays:
/// each revision's scene takes the place of the one before, so putting
/// the draft away finds the committed mesh, and committing the draft
/// finds the last revision's.
#[test]
fn a_dragged_draft_leaves_the_committed_scene_held() {
    let mut editor = Editor::new(Document::example());
    let mut regenerator = Regenerator::default();
    let committed = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    let mut last = None;
    for (revision, depth) in [(0, "3"), (1, "4"), (2, "5"), (3, "6")] {
        let draft = new_body_draft(editor.document(), revision, depth);
        let answer = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
        assert_eq!(answer.draft.unwrap().error, None);
        assert!(!Arc::ptr_eq(&answer.mesh, &committed));
        last = Some((draft, answer.mesh));
    }
    assert_eq!(regenerator.cache().joins(), 5);
    // Put away: the committed mesh, not joined again.
    let away = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    assert!(Arc::ptr_eq(&away, &committed));
    assert_eq!(regenerator.cache().joins(), 5);
    // Taken up again at the last depth, then committed: its mesh both
    // times, and the committed scene before it is still held.
    let (draft, dragged) = last.unwrap();
    let mut again = draft.clone();
    again.revision = 4;
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(again))));
    assert!(Arc::ptr_eq(&answer.mesh, &dragged));
    let before = editor.clone();
    editor
        .apply(editor.document().add_extrude(draft.extrude))
        .unwrap();
    let done = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    assert!(Arc::ptr_eq(&done, &dragged));
    assert_eq!(regenerator.cache().joins(), 5);
    // The committed scene is a model's now: a new draft on top takes the
    // place of the scene before the commit, not of it.
    let next = new_body_draft(editor.document(), 5, "2");
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(next))));
    assert!(!Arc::ptr_eq(&answer.mesh, &done));
    let after = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    assert!(Arc::ptr_eq(&after, &done));
    assert_eq!(regenerator.cache().joins(), 6);
    // Undone: the scene before the commit was pushed out, so it's joined
    // again, with the right content.
    let undone = answered(regenerator.handle(regenerate(&before, None))).mesh;
    assert_eq!(*undone, *committed);
    assert_eq!(regenerator.cache().joins(), 7);
}
