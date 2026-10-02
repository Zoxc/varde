use glam::{DVec2, Vec3};
use varde_document::{
    Command, Document, Editor, Extent, Extrude, Operation, OriginPlane, Plane, Sketch,
};
use varde_kernel::{Solid, Tolerance};
use varde_sketch::{CIRCLE_SEGMENTS, Constraint, Curve};

use super::*;
use crate::history::tests::{example_extrude, plate_below};

fn regenerate(editor: &Editor, exclude: Option<FeatureId>) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude,
        draft: None,
        inspect: None,
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
        picking,
        sketches,
        unsolved,
        failed,
        touched,
        merged,
        placements,
        bodies,
        inspected,
    } = handle(regenerate(&editor, None))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(exclude, None);
    assert_eq!(draft, None);
    assert_eq!(inspected, None);
    // Sketches make no bodies.
    assert!(
        evaluate(editor.document(), &mut Cache::default())
            .bodies
            .is_empty()
    );
    assert_eq!(*mesh, RenderMesh::default());
    assert_eq!(*picking, Picking::default());
    assert_eq!(sketches.ends().len(), 2);
    assert!(unsolved.is_empty());
    assert!(failed.is_empty());
    assert!(touched.is_empty());
    assert!(merged.is_empty());
    assert!(placements.is_empty());
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
            .apply(editor.document().add_feature(extrude.into()))
            .unwrap();
        let body = editor.document().bodies()[1].id;
        editor.apply(Command::SetVisible(body, false)).unwrap();
    }
    editor.document().clone()
}

/// A scene [`tessellate_picking`] draws: its mesh and the body of each of
/// its parts.
struct Shown {
    mesh: Arc<RenderMesh>,
    bodies: Vec<BodyId>,
}

fn shown_scene(
    document: &Document,
    evaluation: &Evaluation,
    cache: &mut Cache,
) -> Result<Shown, varde_kernel::MeshError> {
    let (mesh, picking) = tessellate_picking(document, evaluation, cache)?;
    assert_eq!(picking.bodies().len(), mesh.part_ends().len());
    assert_eq!(tessellate(document, evaluation, cache)?, mesh);
    Ok(Shown {
        mesh,
        bodies: picking.bodies().to_vec(),
    })
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
    let scene = shown_scene(&document, &evaluation, &mut Cache::default()).unwrap();
    assert_eq!(scene.bodies, [body, body]);
    let mesh = scene.mesh;
    assert_eq!(*mesh, both);
    assert_eq!(mesh.triangle_count(), 24);
    assert_eq!(
        mesh.bounds().unwrap(),
        varde_kernel::Aabb {
            min: Vec3::ZERO,
            max: Vec3::splat(5.0)
        }
    );
    let none = shown_scene(&document, &made([]), &mut Cache::default()).unwrap();
    assert_eq!(*none.mesh, RenderMesh::default());
    assert!(none.bodies.is_empty());
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
    for document in [&both, &one] {
        let scene = shown_scene(document, &solids, &mut cache).unwrap();
        assert_eq!(*scene.mesh, alone);
        assert_eq!(scene.bodies, [shown]);
    }
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
    let fine = shown_scene(editor.document(), &solids, &mut cache)
        .unwrap()
        .mesh;
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    editor.apply(Command::SetTolerance(coarse)).unwrap();
    let drawn = shown_scene(editor.document(), &solids, &mut cache)
        .unwrap()
        .mesh;
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
        flatten_sketches(editor.document(), &[], None).unwrap()
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
    let lines = flatten_sketches(editor.document(), &[], None).unwrap();
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

    let lines = flatten_sketches(editor.document(), &[], Some(feature)).unwrap();
    assert_eq!(lines.polylines().collect::<Vec<_>>(), only_second);
    assert_eq!(
        flatten_sketches(editor.document(), &[], None)
            .unwrap()
            .ends()
            .len(),
        3
    );

    editor
        .apply(Command::SetFeatureVisible(feature, false))
        .unwrap();
    let lines = flatten_sketches(editor.document(), &[], None).unwrap();
    assert_eq!(lines.polylines().collect::<Vec<_>>(), only_second);
    let lines = flatten_sketches(editor.document(), &[], Some(second)).unwrap();
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
    let lines = flatten_sketches(editor.document(), &[], None).unwrap();
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
    let lines = flatten_sketches(editor.document(), &[], None).unwrap();
    assert_eq!(
        lines.polylines().collect::<Vec<_>>(),
        [
            [[1.0, 0.0, 0.0], [20.0, 0.0, 0.0]],
            [[0.0, 10.0, 0.0], [0.0, 1.0, 0.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        ]
    );
}

pub(crate) fn regenerate_with(editor: &Editor, draft: Option<Draft>) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: draft.map(Box::new),
        inspect: None,
    }
}

/// The model of a regeneration that worked.
pub(crate) struct Answer {
    pub(crate) draft: Option<Drafted>,
    pub(crate) mesh: Arc<RenderMesh>,
    pub(crate) parts: Vec<BodyId>,
    pub(crate) failed: Vec<(FeatureId, String)>,
    pub(crate) bodies: Vec<(BodyId, varde_kernel::Aabb)>,
    pub(crate) sketches: Arc<RenderLines>,
    pub(crate) placements: Vec<(FeatureId, varde_document::Placement)>,
}

pub(crate) fn answered(response: Response) -> Answer {
    let Response::Regenerated {
        draft,
        mesh,
        picking,
        failed,
        bodies,
        sketches,
        placements,
        ..
    } = response
    else {
        panic!("regeneration failed: {response:?}");
    };
    let parts = picking.bodies().to_vec();
    assert_eq!(parts.len(), mesh.part_ends().len());
    Answer {
        draft,
        mesh,
        parts,
        failed,
        bodies,
        sketches,
        placements,
    }
}

#[test]
fn the_example_plate_regenerates_and_draws() {
    let editor = Editor::new(Document::example());
    let answer = answered(handle(regenerate(&editor, None)));
    assert!(answer.failed.is_empty(), "{:?}", answer.failed);
    assert!(answer.mesh.triangle_count() > 0);
    assert!(answer.mesh.edge_count() > 0);
    let body = editor.document().bodies()[0].id;
    let bounds = varde_kernel::Aabb {
        min: Vec3::new(-30.0, -20.0, 0.0),
        max: Vec3::new(30.0, 20.0, 10.0),
    };
    assert_eq!(answer.bodies, [(body, bounds)]);
    assert_eq!(answer.parts, [body]);
    assert_eq!(answer.mesh.bounds(), Some(bounds));
}

impl Draft {
    /// The extrude it is.
    pub(crate) fn extrude(&self) -> &Extrude {
        match &self.kind {
            FeatureKind::Extrude(extrude) => extrude,
            _ => panic!("not an extrude's draft"),
        }
    }

    /// The same, to change.
    pub(crate) fn extrude_mut(&mut self) -> &mut Extrude {
        match &mut self.kind {
            FeatureKind::Extrude(extrude) => extrude,
            _ => panic!("not an extrude's draft"),
        }
    }
}

/// A draft of revision `revision` making a new body from the example's
/// regions, `text` long, flipped.
fn new_body_draft(document: &Document, revision: u64, text: &str) -> Draft {
    Draft {
        revision,
        feature: None,
        kind: Extrude {
            extent: varde_document::Extent::OneSide(crate::history::tests::length(document, text)),
            flip: true,
            operation: Operation::NewBody(BodyId::NEW),
            ..example_extrude(document)
        }
        .into(),
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
    let [(first, _), (new, below)] = answer.bodies[..] else {
        panic!("two bodies");
    };
    assert_eq!(first, editor.document().bodies()[0].id);
    assert_eq!(answer.parts, [first, new]);
    assert_eq!((below.min.z, below.max.z), (-3.0, 0.0));
    assert_eq!(answer.mesh.bounds().unwrap().min.z, -3.0);

    // An extrude edited: the body it makes changes.
    let feature = editor.document().features()[1].id;
    let draft = Draft {
        revision: 8,
        feature: Some(feature),
        kind: Extrude {
            flip: false,
            ..new_body_draft(editor.document(), 0, "20").extrude().clone()
        }
        .into(),
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
        kind: Extrude {
            operation: Operation::Join(varde_document::Targets {
                excluded: vec![body],
            }),
            ..new_body_draft(editor.document(), 0, "3").extrude().clone()
        }
        .into(),
        ..new_body_draft(editor.document(), 3, "3")
    };
    // One the document refuses: its sketch is the extrude.
    let mut refused = new_body_draft(editor.document(), 4, "3");
    refused.extrude_mut().sketch = editor.document().features()[1].id;
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
            {
                let FeatureKind::Sketch { .. } = &editor.document().features()[0].kind else {
                    panic!("the example's first feature is its sketch");
                };
                varde_document::EditError::SketchKind.to_string()
            },
            // Refused before any tool: not tested.
            None,
        ),
        (
            refused.clone(),
            {
                let command = editor.document().add_feature(refused.kind.clone());
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

/// An intersect draft of the example's regions, below the plate (flush
/// on its bottom face, leaving nothing) and then above it (leaving its
/// lower half): the error comes and goes, also when answered from the
/// cache, and the failing drafts leave the plate as it was.
#[test]
fn a_draft_leaving_nothing_fails_each_time_it_is_dragged_there() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let mut regenerator = Regenerator::default();
    let committed = answered(regenerator.handle(regenerate(&editor, None)));
    let intersect = |revision, flip| {
        let mut draft = new_body_draft(editor.document(), revision, "5");
        draft.extrude_mut().operation = Operation::Intersect(varde_document::Targets::default());
        draft.extrude_mut().flip = flip;
        draft
    };
    let emptied = crate::message::emptied(crate::message::Doing::Intersecting, "Body 1");
    for (revision, flip) in [(1, true), (2, false), (3, true), (4, false)] {
        let answer =
            answered(regenerator.handle(regenerate_with(&editor, Some(intersect(revision, flip)))));
        let drafted = answer.draft.unwrap();
        assert_eq!(drafted.revision, revision);
        assert_eq!(drafted.touched, Some(vec![body]));
        assert!(answer.failed.is_empty(), "{:?}", answer.failed);
        let [(_, bounds)] = answer.bodies[..] else {
            panic!("one body");
        };
        if flip {
            assert_eq!(drafted.error.as_ref(), Some(&emptied));
            assert_eq!(answer.bodies, committed.bodies);
            assert_eq!(answer.mesh, committed.mesh);
        } else {
            assert_eq!(drafted.error, None);
            assert_eq!((bounds.min.z, bounds.max.z), (0.0, 5.0));
        }
    }
}

/// With the default budget, and with none: what the request before used
/// is always kept.
#[test]
fn dragging_a_draft_reruns_only_the_draft() {
    for mut regenerator in [Regenerator::default(), Regenerator::with_budget(0)] {
        let editor = Editor::new(Document::example());
        let draft = new_body_draft(editor.document(), 1, "3");
        answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
        let (_, before) = regenerator.cache().counts();
        let draft = new_body_draft(editor.document(), 2, "4");
        let answer = answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
        assert_eq!(answer.draft.unwrap().revision, 2);
        // The draft's solid and its mesh.
        assert_eq!(regenerator.cache().counts().1, before + 2);
    }
}

/// The example with the pocket's sketch committed, the pocket's cut as a
/// draft of revision 1, and the plate.
fn pocket_drafted() -> (Editor, Draft, BodyId) {
    drafted(crate::history::tests::add_pocket)
}

/// The example with the sketch of the extrude `add` adds committed, the
/// extrude as a draft of revision 1, and the plate.
fn drafted(add: impl FnOnce(&mut Editor) -> FeatureId) -> (Editor, Draft, BodyId) {
    let mut editor = Editor::new(Document::example());
    let mut probe = Editor::new(editor.document().clone());
    let added = add(&mut probe);
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
    assert_eq!(cut.id, added);
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
    let draft = Draft {
        revision: 1,
        feature: None,
        kind: extrude.clone().into(),
    };
    (editor, draft, body)
}

#[test]
fn a_cut_draft_lists_what_it_touches_and_is_answered_from_the_cache() {
    // Without a budget, only the scene the cut made, used two requests
    // before, is joined again when the plate is put back.
    for (mut regenerator, rejoined) in [
        (Regenerator::default(), 0),
        (Regenerator::with_budget(0), 1),
    ] {
        a_cut_draft_is_answered_from_the_cache(&mut regenerator, rejoined);
    }
}

fn a_cut_draft_is_answered_from_the_cache(regenerator: &mut Regenerator, rejoined: usize) {
    let (editor, mut draft, body) = pocket_drafted();
    let committed = answered(regenerator.handle(regenerate(&editor, None)));
    let (_, before) = regenerator.cache().counts();

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
    // asked, and the model's mesh is the committed scene's, which is
    // never evicted: nothing is worked out.
    let mut out = draft.clone();
    out.revision = 2;
    out.extrude_mut().operation = Operation::Cut(varde_document::Targets {
        excluded: vec![body],
    });
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(out))));
    let drafted = answer.draft.unwrap();
    assert_eq!(drafted.touched, Some(vec![]));
    assert!(drafted.error.is_some());
    assert!(Arc::ptr_eq(&answer.mesh, &committed.mesh));
    assert_eq!(regenerator.cache().counts().1, worked);

    // Putting it back finds whether it touches and the cut, kept while
    // it was out, and, within the budget, the scene the cut made: nothing
    // is worked out but, without a budget, the cut's mesh and the scene.
    draft.revision = 3;
    let joins = regenerator.cache().joins();
    let back = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(back.draft.unwrap().error, None);
    assert_eq!(*back.mesh, *cut.mesh);
    assert_eq!(Arc::ptr_eq(&back.mesh, &cut.mesh), rejoined == 0);
    assert_eq!(regenerator.cache().joins(), joins + rejoined);
    assert_eq!(regenerator.cache().counts().1, worked + rejoined);
    let worked = worked + rejoined;

    // Dragging the pocket deeper: only its tool, touching, cut and mesh.
    draft.revision = 4;
    draft.extrude_mut().extent = crate::history::tests::two_sides(editor.document(), "5", "1");
    let deeper = answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(deeper.draft.unwrap().error, None);
    assert_eq!(regenerator.cache().counts().1, worked + 4);
}

/// A hole drafted tangent to the plate's hole and dragged deeper twice:
/// whether the tool touches the plate is worked out again for each
/// tool, and says it does, so the plate is listed and the cut decided by
/// its boolean, never failed by the touch test. Inside the hole the cut
/// is a no-op; outside it the holes would meet along a line, which the
/// boolean refuses.
#[test]
fn a_tangent_hole_dragged_reruns_its_touch_test_which_holds() {
    let refused = "cutting it from Body 1 leaves no clean solid";
    for (distance, error) in [(5.0, None), (11.0, Some(refused))] {
        let (editor, mut draft, body) =
            drafted(|editor| crate::history::tests::add_drilled(editor, distance, 0.7, 3.0));
        let mut regenerator = Regenerator::default();
        let committed = answered(regenerator.handle(regenerate(&editor, None)));
        for (revision, depth) in [(1, "20"), (2, "21"), (3, "22")] {
            draft.revision = revision;
            draft.extrude_mut().extent =
                crate::history::tests::two_sides(editor.document(), depth, "20");
            let (_, before) = regenerator.cache().counts();
            let answer =
                answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
            let drafted = answer.draft.unwrap();
            assert_eq!(drafted.revision, revision);
            assert_eq!(drafted.touched, Some(vec![body]));
            assert!(answer.failed.is_empty(), "{:?}", answer.failed);
            let (_, worked) = regenerator.cache().counts();
            match error {
                // The tool, whether it touches, the cut and its mesh.
                None => {
                    assert_eq!(drafted.error, None);
                    assert_eq!(worked, before + 4);
                }
                // The tool, whether it touches and the refused cut.
                Some(error) => {
                    let message = drafted.error.unwrap();
                    assert!(message.starts_with(error), "{message}");
                    assert_eq!(worked, before + 3);
                    assert_eq!(answer.mesh, committed.mesh);
                }
            }
        }
    }
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
        kind: Extrude {
            operation: Operation::Join(varde_document::Targets {
                excluded: vec![plate],
            }),
            ..new_body_draft(editor.document(), 0, "3").extrude().clone()
        }
        .into(),
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
        .apply(probe.document().add_feature(draft.kind.clone()))
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
        .apply(editor.document().add_feature(draft.kind))
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
    let one = shown_scene(&document, &scene(&[(shown, 0)]), &mut cache).unwrap();
    assert_eq!(*one.mesh, joined(&[0]));
    assert_eq!(one.bodies, [shown]);
    let twice = shown_scene(&document, &scene(&[(shown, 0), (shown, 0)]), &mut cache).unwrap();
    assert_eq!(*twice.mesh, joined(&[0, 0]));
    assert_eq!(twice.bodies, [shown, shown]);
    // A hidden body between two shown ones, and the two swapped.
    let ab = scene(&[(shown, 0), (hidden, 2), (shown, 1)]);
    let ab = shown_scene(&document, &ab, &mut cache).unwrap();
    assert_eq!(*ab.mesh, joined(&[0, 1]));
    let ba = scene(&[(shown, 1), (hidden, 2), (shown, 0)]);
    let ba = shown_scene(&document, &ba, &mut cache).unwrap();
    assert_eq!(*ba.mesh, joined(&[1, 0]));
    assert_eq!(ba.bodies, [shown, shown]);
    // Without the hidden body, the same scene as with it.
    let without = shown_scene(&document, &scene(&[(shown, 1), (shown, 0)]), &mut cache).unwrap();
    assert!(Arc::ptr_eq(&without.mesh, &ba.mesh));
    assert_eq!(cache.joins(), 4);
    // No bodies: one empty mesh, found again.
    let none = shown_scene(&document, &scene(&[]), &mut cache).unwrap();
    let again = shown_scene(&document, &scene(&[(hidden, 2)]), &mut cache).unwrap();
    assert!(Arc::ptr_eq(&none.mesh, &again.mesh));
    assert_eq!(*none.mesh, RenderMesh::default());
    assert!(none.bodies.is_empty());
    assert_eq!(cache.joins(), 5);
}

/// The scene key holds the shown bodies' ids too: the same solid shown as
/// another body is another scene, whose part is that body's.
#[test]
fn the_scene_key_holds_the_bodies_ids() {
    let mut document = with_bodies(true);
    let [first, second] = [document.bodies()[0].id, document.bodies()[1].id];
    let mut editor = Editor::new(document);
    editor.apply(Command::SetVisible(second, true)).unwrap();
    document = editor.document().clone();
    let as_body = |body| Evaluation {
        bodies: vec![BodySolid {
            body,
            solid: Arc::new(cuboid(0.0, 1.0)),
            key: Keyer::new("test").finish(),
        }],
        ..Evaluation::default()
    };
    let mut cache = Cache::default();
    let a = shown_scene(&document, &as_body(first), &mut cache).unwrap();
    let b = shown_scene(&document, &as_body(second), &mut cache).unwrap();
    assert_eq!((a.bodies, b.bodies), (vec![first], vec![second]));
    assert_eq!(a.mesh, b.mesh);
    assert_eq!(cache.joins(), 2);
}

/// Scenes are held like any other result: within the budget, a scene
/// used long ago is found again; without one, a scene the request before
/// didn't use, and that isn't the committed one, goes.
#[test]
fn scenes_are_held_within_the_budget() {
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
    let ask = |cache: &mut Cache, scene: usize, drafted: bool| {
        cache.begin();
        tessellate_scene(&document, &scenes[scene], drafted, cache)
            .unwrap()
            .mesh
    };
    let mut cache = Cache::default();
    let a = ask(&mut cache, 0, false);
    let b = ask(&mut cache, 1, false);
    ask(&mut cache, 2, false);
    for _ in 0..3 {
        for (scene, mesh) in [(0, &a), (1, &b)] {
            assert!(Arc::ptr_eq(&ask(&mut cache, scene, false), mesh));
        }
    }
    assert_eq!(cache.joins(), 3);

    // Without a budget: the scene the request before used and the
    // committed one stay, any other goes.
    let mut cache = Cache::with_budget(0);
    let a = ask(&mut cache, 0, false);
    let b = ask(&mut cache, 1, true);
    assert!(Arc::ptr_eq(&ask(&mut cache, 1, true), &b));
    assert!(Arc::ptr_eq(&ask(&mut cache, 0, false), &a));
    assert_eq!(cache.joins(), 2);
    ask(&mut cache, 2, false);
    ask(&mut cache, 2, false);
    let later = ask(&mut cache, 0, false);
    assert!(!Arc::ptr_eq(&later, &a));
    assert_eq!(*later, *a);
    assert_eq!(cache.joins(), 4);
}

/// However long a draft is dragged, the committed model's scene stays,
/// even without a budget, so putting the draft away finds the committed
/// mesh; within the budget, committing the draft finds the last
/// revision's, and undoing the commit the scene before.
#[test]
fn a_dragged_draft_leaves_the_committed_scene_held() {
    let editor = Editor::new(Document::example());
    let mut regenerator = Regenerator::with_budget(0);
    let committed = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    for (revision, depth) in [(0, "3"), (1, "4"), (2, "5"), (3, "6")] {
        let draft = new_body_draft(editor.document(), revision, depth);
        answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
    }
    let away = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    assert!(Arc::ptr_eq(&away, &committed));
    assert_eq!(regenerator.cache().joins(), 5);
    a_dragged_draft_is_found_within_the_budget();
}

fn a_dragged_draft_is_found_within_the_budget() {
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
        .apply(editor.document().add_feature(draft.kind))
        .unwrap();
    let done = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    assert!(Arc::ptr_eq(&done, &dragged));
    assert_eq!(regenerator.cache().joins(), 5);
    // A new draft on top, put away: the committed model's scene.
    let next = new_body_draft(editor.document(), 5, "2");
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(next))));
    assert!(!Arc::ptr_eq(&answer.mesh, &done));
    let after = answered(regenerator.handle(regenerate(&editor, None))).mesh;
    assert!(Arc::ptr_eq(&after, &done));
    assert_eq!(regenerator.cache().joins(), 6);
    // Undone: the scene before the commit, still held.
    let undone = answered(regenerator.handle(regenerate(&before, None))).mesh;
    assert!(Arc::ptr_eq(&undone, &committed));
    assert_eq!(regenerator.cache().joins(), 6);
}

/// A draft whose scene is an older one still held (here the plate alone,
/// after the second body was shown) doesn't make that one the committed
/// scene: the committed model's scene stays held however the draft is
/// dragged, even without a budget, and putting it away finds it.
#[test]
fn a_draft_finding_the_older_scene_leaves_the_committed_one_held() {
    let document = with_bodies(false);
    let body = document.bodies()[0].id;
    let solid = |k: u64, min: f64| BodySolid {
        body,
        solid: Arc::new(cuboid(min, 1.0)),
        key: Keyer::new("test").number(k).finish(),
    };
    let scene = |bodies: Vec<BodySolid>| Evaluation {
        bodies,
        ..Evaluation::default()
    };
    let older = scene(vec![solid(0, 0.0)]);
    let committed = scene(vec![solid(0, 0.0), solid(1, 3.0)]);
    for mut cache in [Cache::default(), Cache::with_budget(0)] {
        let mut ask = |evaluation: &Evaluation, drafted: bool| {
            cache.begin();
            tessellate_scene(&document, evaluation, drafted, &mut cache)
                .unwrap()
                .mesh
        };
        ask(&older, false);
        let held = ask(&committed, false);
        // A draft that happens to give the older scene, then two revisions
        // with scenes of their own.
        ask(&older, true);
        ask(&scene(vec![solid(2, 6.0)]), true);
        ask(&scene(vec![solid(3, 9.0)]), true);
        assert!(Arc::ptr_eq(&ask(&committed, false), &held));
    }
}

/// A join that fails (a mesh past `RenderMesh`'s limits) isn't kept and
/// leaves the scenes held as they were; the same scene asked again is
/// joined again.
#[test]
fn a_failed_join_is_not_kept() {
    let key = |k: u64| Keyer::new("test").number(k).finish();
    let mut cache = Cache::default();
    let empty = || Scene {
        mesh: Arc::new(RenderMesh::default()),
        picking: Arc::new(Picking::default()),
    };
    let mesh = |cache: &mut Cache, k: u64, drafted: bool| {
        cache
            .scene(key(k), drafted, |_| Ok::<_, ()>(empty()))
            .unwrap()
            .mesh
    };
    let a = mesh(&mut cache, 0, false);
    let b = mesh(&mut cache, 1, true);
    for drafted in [false, true] {
        let mut tried = false;
        let failed = cache.scene(key(2), drafted, |_| {
            tried = true;
            Err::<Scene, _>("too large")
        });
        assert_eq!(failed.err(), Some("too large"));
        assert!(tried);
    }
    assert_eq!(cache.joins(), 2);
    assert!(Arc::ptr_eq(&mesh(&mut cache, 0, false), &a));
    assert!(Arc::ptr_eq(&mesh(&mut cache, 1, true), &b));
    let mut tried = false;
    let _ = cache.scene(key(2), false, |_| {
        tried = true;
        Ok::<_, ()>(empty())
    });
    assert!(tried);
    assert_eq!(cache.joins(), 3);
}

/// A body removed, the removal undone and redone, and the document
/// replaced, each answered with the mesh of the bodies then shown.
#[test]
fn removed_bodies_undo_and_replace_draw_what_is_shown() {
    let mut editor = Editor::new(Document::example());
    let extrude = Extrude {
        operation: Operation::NewBody(BodyId::NEW),
        ..new_body_draft(editor.document(), 0, "3").extrude().clone()
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    let two = editor.document().clone();
    let mut regenerator = Regenerator::default();
    let mut ask = |editor: &Editor| answered(regenerator.handle(regenerate(editor, None))).mesh;
    let drawn = |document: &Document| {
        let display = Display::new(&document.tolerance());
        let mut mesh = RenderMesh::default();
        for made in &evaluate(document, &mut Cache::default()).bodies {
            mesh.append(&made.solid.tessellate(&display).unwrap())
                .unwrap();
        }
        mesh
    };
    let both = ask(&editor);
    assert_eq!(*both, drawn(&two));
    let second = editor.document().bodies()[1].id;
    editor.apply(Command::RemoveBody(second)).unwrap();
    let one = ask(&editor);
    assert_eq!(*one, drawn(editor.document()));
    assert_ne!(*one, *both);
    editor.undo();
    assert!(Arc::ptr_eq(&ask(&editor), &both));
    editor.redo();
    assert!(Arc::ptr_eq(&ask(&editor), &one));
    editor.apply(Command::Replace(Box::default())).unwrap();
    assert_eq!(*ask(&editor), RenderMesh::default());
    editor
        .apply(Command::Replace(Box::new(two.clone())))
        .unwrap();
    assert_eq!(*ask(&editor), drawn(&two));
}

/// Switching a draft cut's operation to join and back to cut finds
/// everything the first cut worked out.
#[test]
fn switching_the_operation_away_and_back_finds_it() {
    let (editor, mut draft, _) = pocket_drafted();
    let mut regenerator = Regenerator::default();
    answered(regenerator.handle(regenerate(&editor, None)));
    let cut = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(cut.draft.unwrap().error, None);
    draft.revision = 2;
    let operation = std::mem::replace(
        &mut draft.extrude_mut().operation,
        Operation::Join(varde_document::Targets::default()),
    );
    let join = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(join.draft.unwrap().error, None);
    assert_ne!(*join.mesh, *cut.mesh);
    let (_, worked) = regenerator.cache().counts();
    let joins = regenerator.cache().joins();
    draft.revision = 3;
    draft.extrude_mut().operation = operation;
    let again = answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(again.draft.unwrap().error, None);
    assert!(Arc::ptr_eq(&again.mesh, &cut.mesh));
    assert_eq!(regenerator.cache().counts().1, worked);
    assert_eq!(regenerator.cache().joins(), joins);
}

/// The example plate made 6 mm thick: an edit with a cut after it.
fn with_pocket() -> (Editor, FeatureId) {
    let mut editor = Editor::new(Document::example());
    crate::history::tests::add_pocket(&mut editor);
    let plate = editor.document().features()[1].id;
    (editor, plate)
}

fn set_depth(editor: &mut Editor, plate: FeatureId, depth: &str) {
    let extent = Extent::OneSide(crate::history::tests::length(editor.document(), depth));
    crate::history::tests::set_extrude(editor, plate, |extrude| extrude.extent = extent);
}

/// Undoing an edit, and redoing it, work out nothing: the plate, the cut
/// after it and their meshes are found as each state left them.
#[test]
fn undo_and_redo_find_what_they_had() {
    let (mut editor, plate) = with_pocket();
    let mut regenerator = Regenerator::default();
    let before = answered(regenerator.handle(regenerate(&editor, None)));
    assert!(before.failed.is_empty());
    set_depth(&mut editor, plate, "6");
    let (_, worked) = regenerator.cache().counts();
    let after = answered(regenerator.handle(regenerate(&editor, None)));
    // The plate, whether the pocket touches it, the cut, and its mesh.
    assert_eq!(regenerator.cache().counts().1, worked + 4);
    let (_, worked) = regenerator.cache().counts();
    let joins = regenerator.cache().joins();
    for (mesh, step) in [
        (&before.mesh, Editor::undo as fn(&mut Editor)),
        (&after.mesh, Editor::redo),
        (&before.mesh, Editor::undo),
    ] {
        step(&mut editor);
        let answer = answered(regenerator.handle(regenerate(&editor, None)));
        assert!(Arc::ptr_eq(&answer.mesh, mesh));
    }
    assert_eq!(regenerator.cache().counts().1, worked);
    assert_eq!(regenerator.cache().joins(), joins);
}

/// An edit undone after several requests in between (drafts of the
/// plate's depth dragged, then committed) finds what it had.
#[test]
fn an_edit_undone_after_several_requests_finds_it() {
    let (mut editor, plate) = with_pocket();
    let mut regenerator = Regenerator::default();
    let first = answered(regenerator.handle(regenerate(&editor, None)));
    let extrude = |editor: &Editor, depth: &str| Extrude {
        extent: Extent::OneSide(crate::history::tests::length(editor.document(), depth)),
        ..example_extrude(editor.document())
    };
    for (revision, depth) in [(1, "7"), (2, "8"), (3, "9")] {
        let draft = Draft {
            revision,
            feature: Some(plate),
            kind: extrude(&editor, depth).into(),
        };
        let answer = answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
        assert_eq!(answer.draft.unwrap().error, None);
    }
    set_depth(&mut editor, plate, "9");
    answered(regenerator.handle(regenerate(&editor, None)));
    let (_, worked) = regenerator.cache().counts();
    let joins = regenerator.cache().joins();
    editor.undo();
    let undone = answered(regenerator.handle(regenerate(&editor, None)));
    assert!(Arc::ptr_eq(&undone.mesh, &first.mesh));
    assert_eq!(regenerator.cache().counts().1, worked);
    assert_eq!(regenerator.cache().joins(), joins);
}

/// Meshes of `n` triangles, each filed under its own key in its own
/// request (`ask`), to fill a cache with entries of one size.
fn mesh_of(n: usize) -> RenderMesh {
    let mut mesh = RenderMesh::default();
    let one = cuboid(0.0, 1.0)
        .tessellate(&Display::new(&Tolerance::DEFAULT))
        .unwrap();
    for _ in 0..n {
        mesh.append(&one).unwrap();
    }
    mesh
}

/// `mesh` as a body's drawing, with no picking tables.
fn drawn(mesh: &RenderMesh) -> Drawn {
    Drawn {
        mesh: mesh.clone(),
        faces: Vec::new(),
        closed: Vec::new(),
        tangents: Vec::new(),
        snaps: Vec::new(),
        corners: Vec::new(),
    }
}

/// What a mesh costs a cache.
fn bytes_of(mesh: &RenderMesh) -> usize {
    let mut cache = Cache::default();
    ask_meshes(&mut cache, &[0], mesh);
    cache.bytes()
}

/// Asks `cache`, in a request of its own, for the meshes filed under
/// `keys`; whether each was found.
fn ask_meshes(cache: &mut Cache, keys: &[u64], mesh: &RenderMesh) -> Vec<bool> {
    cache.begin();
    let found = keys
        .iter()
        .map(|&k| {
            let mut found = true;
            let key = Keyer::new("test").number(k).finish();
            cache
                .mesh(key, || {
                    found = false;
                    Ok::<_, ()>(drawn(mesh))
                })
                .unwrap();
            found
        })
        .collect();
    let (total, _) = cache.audit();
    assert_eq!(cache.bytes(), total);
    found
}

/// The least recently used go first, and only once over the budget: with
/// room for one request's worth besides the one before, A → B → A finds
/// A's, A → B → C → A doesn't; without a budget, as before the cache was
/// bounded by size, only what the request before used is found.
#[test]
fn a_small_budget_evicts_the_oldest_first() {
    let mesh = mesh_of(1);
    let budget = 2 * bytes_of(&mesh);
    let mut cache = Cache::with_budget(budget);
    assert_eq!(ask_meshes(&mut cache, &[0], &mesh), [false]);
    assert_eq!(ask_meshes(&mut cache, &[1], &mesh), [false]);
    assert_eq!(ask_meshes(&mut cache, &[0], &mesh), [true]);
    assert_eq!(ask_meshes(&mut cache, &[1], &mesh), [true]);
    assert_eq!(ask_meshes(&mut cache, &[2], &mesh), [false]);
    // Over the budget: 0, used least recently, goes, and is filed again;
    // the next request evicts 1, and finds 2.
    assert_eq!(ask_meshes(&mut cache, &[0], &mesh), [false]);
    assert_eq!(cache.len(), 3);
    assert_eq!(ask_meshes(&mut cache, &[2], &mesh), [true]);
    assert_eq!(cache.len(), 2);
    assert!(cache.bytes() <= budget);
    assert_eq!(ask_meshes(&mut cache, &[0, 2], &mesh), [true, true]);
    assert_eq!(ask_meshes(&mut cache, &[1], &mesh), [false]);

    let mut cache = Cache::with_budget(0);
    assert_eq!(ask_meshes(&mut cache, &[0, 1], &mesh), [false, false]);
    assert_eq!(ask_meshes(&mut cache, &[1], &mesh), [true]);
    assert_eq!(ask_meshes(&mut cache, &[0, 1], &mesh), [false, true]);
    assert_eq!(ask_meshes(&mut cache, &[2], &mesh), [false]);
    assert_eq!(ask_meshes(&mut cache, &[1, 2], &mesh), [false, true]);
    assert_eq!(cache.len(), 2);
}

/// Which results go doesn't depend on the map's order: the same requests
/// to two caches (each hashing its own way) find the same results.
#[test]
fn eviction_is_deterministic() {
    let mesh = mesh_of(1);
    let budget = 4 * bytes_of(&mesh);
    let run = || {
        let mut cache = Cache::with_budget(budget);
        let mut found = Vec::new();
        for request in 0..40u64 {
            let keys: Vec<u64> = (0..request % 4 + 1)
                .map(|k| (request * 7 + k * 13) % 23)
                .collect();
            found.extend(ask_meshes(&mut cache, &keys, &mesh));
        }
        found
    };
    let first = run();
    assert!(first.iter().any(|&found| found) && first.iter().any(|&found| !found));
    for _ in 0..4 {
        assert_eq!(run(), first);
    }
}

/// The byte count stays the slots' sizes added up through inserts and
/// evictions, and comes back within the budget at a request's start once
/// nothing but what the request before used is over it.
#[test]
fn the_cache_counts_its_bytes() {
    let meshes = [mesh_of(1), mesh_of(3), mesh_of(2)];
    let budget = 3 * bytes_of(&meshes[1]);
    let mut cache = Cache::with_budget(budget);
    for request in 0..60u64 {
        let keys: Vec<u64> = (0..(request % 4 + 1))
            .map(|k| (request * 5 + k * 3) % 17)
            .collect();
        let mesh = &meshes[(request % 3) as usize];
        // At the start of the request: within the budget, or nothing but
        // what the request before used is left.
        cache.begin();
        let (total, protected) = cache.audit();
        assert_eq!(cache.bytes(), total);
        assert!(total <= budget || total == protected, "{request}");
        ask_meshes_unbegun(&mut cache, &keys, mesh);
    }
    // Through a regenerator too, with every kind of result.
    let (mut editor, plate) = with_pocket();
    let mut regenerator = Regenerator::with_budget(1 << 16);
    for depth in ["6", "7", "6", "8", "9"] {
        set_depth(&mut editor, plate, depth);
        answered(regenerator.handle(regenerate(&editor, None)));
        let (total, _) = regenerator.cache().audit();
        assert_eq!(regenerator.cache().bytes(), total);
        assert!(total > 0);
    }
}

/// [`ask_meshes`] within the request already begun.
fn ask_meshes_unbegun(cache: &mut Cache, keys: &[u64], mesh: &RenderMesh) {
    for &k in keys {
        let key = Keyer::new("test").number(k).finish();
        cache.mesh(key, || Ok::<_, ()>(drawn(mesh))).unwrap();
    }
    let (total, _) = cache.audit();
    assert_eq!(cache.bytes(), total);
}

/// The bytes one request holds on the example with a pocket and on a
/// plate with many holes, printed to size the budget against.
#[test]
fn the_budget_holds_several_requests() {
    let (editor, _) = with_pocket();
    let mut regenerator = Regenerator::default();
    answered(regenerator.handle(regenerate(&editor, None)));
    let pocket = regenerator.cache().bytes();

    const HOLES: u32 = 6;
    let mut editor = Editor::new(Document::default());
    let ten = Extent::OneSide(crate::history::tests::length(editor.document(), "10"));
    crate::history::tests::add_extrude(
        &mut editor,
        crate::history::tests::rectangle((0.0, 0.0), (100.0, 100.0)),
        ten,
        Operation::NewBody(BodyId::NEW),
    );
    crate::history::tests::add_extrude(
        &mut editor,
        |sketch| {
            for i in 0..HOLES {
                for j in 0..HOLES {
                    let at = |k: u32| 100.0 * (f64::from(k) + 0.5) / f64::from(HOLES);
                    let center = (at(i), at(j));
                    crate::history::tests::disc(center, 2.0)(sketch);
                }
            }
        },
        Extent::ThroughAll,
        Operation::Cut(varde_document::Targets::default()),
    );
    let mut regenerator = Regenerator::default();
    let holes = answered(regenerator.handle(regenerate(&editor, None)));
    assert!(holes.failed.is_empty(), "{:?}", holes.failed);
    let plate = regenerator.cache().bytes();
    println!(
        "one request: pocket {pocket} B, {} holes {plate} B",
        HOLES * HOLES
    );
    assert!(plate.saturating_mul(8) < cache::BUDGET);
}

/// Whether `document`'s second feature is an extrude, as the example's
/// plate is, whose regions drafts are made of.
fn drafts_from(document: &Document) -> bool {
    (document.features().get(1)).is_some_and(|f| matches!(f.kind, FeatureKind::Extrude(_)))
}

/// A small pseudo-random generator, so the churn below is the same on
/// every run.
struct Churn(u64);

impl Churn {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// Long random runs of drafts, edits, undos and redos, tolerance and
/// visibility changes, features added (their ids reused after an undo)
/// and removed, and the document replaced, through regenerators with
/// small budgets and the default one: every answer is the one a fresh
/// regenerator gives. Throughout, the cache holds at most its budget
/// besides what the request before and this one used and the committed
/// scene the request began with; a request asked
/// again works out nothing, and the committed model asked again after
/// drafts joins no scene.
#[test]
fn churn_answers_as_a_fresh_cache_would() {
    use crate::history::tests::{add_extrude, add_pocket, disc, length, rectangle, two_sides};
    let coarse = Tolerance::new(1e-2).unwrap();
    for (seed, budget) in [(1, 0), (2, 40_000), (3, 200_000), (4, cache::BUDGET)] {
        let mut churn = Churn(0x9e37_79b9_7f4a_7c15 ^ seed);
        let mut regenerator = Regenerator::with_budget(budget);
        let mut editor = Editor::new(Document::example());
        let mut revision = 0;
        let mut last: Option<Request> = None;
        // The generation of the last request without a draft, if only
        // drafts of that same document were asked since.
        let mut committed: Option<varde_document::Generation> = None;
        for step in 0..120 {
            let plate = editor.document().features().get(1).map(|f| f.id);
            let mut repeat = false;
            let mut draft = None;
            match churn.below(12) {
                0 | 1 => {
                    revision += 1;
                    let text = ["3", "4", "5", "12"][churn.below(4) as usize];
                    if drafts_from(editor.document()) {
                        let mut new = new_body_draft(editor.document(), revision, text);
                        new.extrude_mut().flip = churn.below(2) == 0;
                        new.extrude_mut().operation = match churn.below(4) {
                            0 => Operation::NewBody(BodyId::NEW),
                            1 => Operation::Join(varde_document::Targets::default()),
                            2 => Operation::Cut(varde_document::Targets::default()),
                            _ => Operation::Intersect(varde_document::Targets::default()),
                        };
                        if churn.below(3) == 0
                            && let Some(plate) = plate
                        {
                            new.feature = Some(plate);
                            new.extrude_mut().operation =
                                example_extrude(editor.document()).operation;
                        }
                        draft = Some(new);
                    }
                }
                2 => {
                    if let Some(plate) = plate
                        && matches!(
                            editor.document().feature(plate).map(|f| &f.kind),
                            Some(FeatureKind::Extrude(Extrude {
                                operation: Operation::NewBody(_),
                                ..
                            }))
                        )
                    {
                        let depth = ["6", "8", "10"][churn.below(3) as usize];
                        set_depth(&mut editor, plate, depth);
                    }
                }
                3 | 4 => editor.undo(),
                5 => editor.redo(),
                6 => {
                    let tolerance = if editor.document().tolerance() == coarse {
                        Tolerance::DEFAULT
                    } else {
                        coarse
                    };
                    let _ = editor.apply(Command::SetTolerance(tolerance));
                }
                7 => {
                    if let Some(body) = editor.document().bodies().first() {
                        let (id, visible) = (body.id, body.visible);
                        editor.apply(Command::SetVisible(id, !visible)).unwrap();
                    }
                }
                8 => {
                    if editor.document().bodies().is_empty() {
                        continue;
                    }
                    let kind = churn.below(3);
                    if kind == 0 {
                        add_pocket(&mut editor);
                    } else if kind == 1 && drafts_from(editor.document()) {
                        // A join taking its only body out: it fails.
                        crate::history::tests::add_failing(&mut editor);
                    } else {
                        let extent = two_sides(editor.document(), "20", "20");
                        let center = [(10.0, 0.0), (-20.0, 10.0)][churn.below(2) as usize];
                        add_extrude(
                            &mut editor,
                            disc(center, 3.0),
                            extent,
                            Operation::Cut(varde_document::Targets::default()),
                        );
                    }
                }
                9 => {
                    if let Some(last) = editor.document().features().last() {
                        let removal = Command::RemoveFeature(last.id);
                        let _ = editor.apply(removal);
                    }
                }
                10 => {
                    let replacement = match churn.below(3) {
                        0 => Document::example(),
                        1 => with_pocket().0.document().clone(),
                        _ => {
                            let mut other = Editor::new(Document::default());
                            let ten = Extent::OneSide(length(other.document(), "10"));
                            add_extrude(
                                &mut other,
                                rectangle((-30.0, -20.0), (30.0, 20.0)),
                                ten,
                                Operation::NewBody(BodyId::NEW),
                            );
                            other.document().clone()
                        }
                    };
                    editor
                        .apply(Command::Replace(Box::new(replacement)))
                        .unwrap();
                }
                _ => repeat = last.is_some(),
            }
            let request = match (&last, repeat) {
                (Some(last), true) => last.clone(),
                _ => regenerate_with(&editor, draft),
            };
            let Request::Regenerate {
                generation,
                draft: ref asked,
                ..
            } = request
            else {
                unreachable!("a regeneration");
            };
            let (_, worked) = regenerator.cache().counts();
            let joins = regenerator.cache().joins();
            // What begins the request may not evict: what the request
            // before used, which is protected below too, and the
            // committed scene, which this request may replace.
            let scene = regenerator.cache().committed_bytes();
            let cached = regenerator.handle(request.clone());
            let fresh = Regenerator::default().handle(request.clone());
            assert_eq!(format!("{cached:?}"), format!("{fresh:?}"), "step {step}");
            let (total, protected) = regenerator.cache().audit();
            assert_eq!(regenerator.cache().bytes(), total);
            let bound = budget.saturating_add(protected).saturating_add(scene);
            assert!(total <= bound, "step {step}: {total} B, budget {budget} B");
            if repeat {
                assert_eq!(regenerator.cache().counts().1, worked, "step {step}");
            }
            if repeat || (asked.is_none() && committed == Some(generation)) {
                assert_eq!(regenerator.cache().joins(), joins, "step {step}");
            }
            committed = match (asked, committed) {
                (None, _) => Some(generation),
                (Some(_), Some(c)) if c == generation => committed,
                _ => None,
            };
            last = Some(request);
        }
    }
}

/// Many bodies, each shown and hidden in turn so that every request
/// joins a new scene: with no budget the cache holds no more than the
/// requests it protects, however many go by; with room for a few, it
/// stays within that room besides them.
#[test]
fn many_bodies_stay_within_the_budget() {
    use crate::history::tests::{add_extrude, disc, length};
    let mut editor = Editor::new(Document::default());
    for k in 0..8 {
        let extent = Extent::OneSide(length(editor.document(), "5"));
        let center = (20.0 * f64::from(k), 0.0);
        add_extrude(
            &mut editor,
            disc(center, 8.0),
            extent,
            Operation::NewBody(BodyId::NEW),
        );
    }
    let bodies: Vec<BodyId> = editor.document().bodies().iter().map(|b| b.id).collect();
    assert_eq!(bodies.len(), 8);
    let mut probe = Regenerator::default();
    answered(probe.handle(regenerate(&editor, None)));
    let one = probe.cache().bytes();
    for budget in [0, one / 2, 2 * one] {
        let mut editor = Editor::new(editor.document().clone());
        let mut regenerator = Regenerator::with_budget(budget);
        for (step, body) in bodies.iter().cycle().take(40).enumerate() {
            let visible = editor.document().body(*body).unwrap().visible;
            editor.apply(Command::SetVisible(*body, !visible)).unwrap();
            let scene = regenerator.cache().committed_bytes();
            let cached = regenerator.handle(regenerate(&editor, None));
            let fresh = Regenerator::default().handle(regenerate(&editor, None));
            assert_eq!(format!("{cached:?}"), format!("{fresh:?}"), "step {step}");
            let (total, protected) = regenerator.cache().audit();
            assert!(total <= budget + protected + scene, "step {step}");
            // The protected set is two requests' worth at most.
            assert!(
                total <= budget + 3 * one,
                "step {step}: {total} B, one {one} B"
            );
        }
    }
}

/// A join draft touching both the example plate and a plate under it,
/// which merges them: dragging it works out only its tool, whether it
/// touches each plate, the tool's union with the plates' (found again)
/// and the merged body's mesh; taking the lower plate out and putting it
/// back finds the merge, kept while it was out: with the default budget
/// everything is found, and with none only the merged body's mesh, which
/// the request before didn't use, is worked out again.
#[test]
fn a_join_draft_merging_two_bodies_reworks_one_boolean_when_dragged() {
    for (mut regenerator, remeshed) in [
        (Regenerator::default(), 0),
        (Regenerator::with_budget(0), 1),
    ] {
        a_join_draft_merging_two_bodies(&mut regenerator, remeshed);
    }
}

fn a_join_draft_merging_two_bodies(regenerator: &mut Regenerator, remeshed: usize) {
    use crate::history::tests::{add_extrude, disc, plate_below, two_sides};
    use varde_document::Targets;

    // The disc's sketch committed, its join a draft.
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let mut probe = Editor::new(editor.document().clone());
    let extent = two_sides(editor.document(), "15", "5");
    let join = Operation::Join(Targets::default());
    add_extrude(&mut probe, disc((20.0, 0.0), 5.0), extent, join);
    let [.., sketch, joined] = probe.document().features() else {
        unreachable!()
    };
    let (
        FeatureKind::Sketch {
            plane,
            sketch: drawn,
        },
        FeatureKind::Extrude(extrude),
    ) = (&sketch.kind, &joined.kind)
    else {
        unreachable!()
    };
    editor.apply(editor.document().add_sketch(*plane)).unwrap();
    let feature = editor.document().features().last().unwrap().id;
    assert_eq!(feature, sketch.id);
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(drawn.clone()),
        })
        .unwrap();
    answered(regenerator.handle(regenerate(&editor, None)));
    let mut draft = Draft {
        revision: 1,
        feature: None,
        kind: extrude.clone().into(),
    };
    let first = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(first.draft.unwrap().touched, Some(vec![top, below]));
    assert_eq!(first.bodies.len(), 1);

    // Dragged: the tool, two touches, one union and one mesh.
    let (_, before) = regenerator.cache().counts();
    draft.revision = 2;
    draft.extrude_mut().extent = two_sides(editor.document(), "16", "5");
    let dragged = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(dragged.draft.unwrap().error, None);
    assert_eq!(dragged.bodies.len(), 1);
    let (_, worked) = regenerator.cache().counts();
    assert_eq!(worked, before + 5);

    // The lower plate taken out: the top plate's union with the tool
    // alone and both plates' meshes are new.
    let mut out = draft.clone();
    out.revision = 3;
    out.extrude_mut().operation = Operation::Join(Targets {
        excluded: vec![below],
    });
    let apart = answered(regenerator.handle(regenerate_with(&editor, Some(out))));
    assert_eq!(apart.draft.unwrap().error, None);
    assert_eq!(apart.bodies.len(), 2);
    let (_, worked) = regenerator.cache().counts();

    // Put back: the touch and the merge are found again.
    draft.revision = 4;
    let back = answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(back.draft.unwrap().error, None);
    assert_eq!(back.bodies.len(), 1);
    assert_eq!(regenerator.cache().counts().1, worked + remeshed);
}

#[test]
fn every_form_has_its_summary_and_bad_numbers_none() {
    use glam::DVec3;
    use varde_kernel::mesh::Form;
    use varde_kernel::patch::{Conic2, Conic3};
    let (z, origin) = (DVec3::Z, DVec3::new(1.0, 2.0, 3.0));
    let ellipse = Conic3::new(DVec3::X, DVec3::new(1.0, 2.0, 0.0), 0.5, DVec3::Y).unwrap();
    let meridian = Conic2::line(DVec2::new(1.0, 0.0), DVec2::new(2.0, 1.0)).unwrap();
    let cases = [
        (Form::Unknown, Summary::Other),
        (
            Form::Plane { n: z, d: 4.0 },
            Summary::Plane {
                n: [0.0, 0.0, 1.0],
                d: 4.0,
            },
        ),
        (
            Form::Cylinder {
                point: origin,
                axis: z,
                radius: 8.0,
            },
            Summary::Cylinder {
                point: [1.0, 2.0, 3.0],
                axis: [0.0, 0.0, 1.0],
                radius: 8.0,
            },
        ),
        (
            Form::Cone {
                apex: origin,
                axis: z,
                cos: 0.8,
                sin: 0.6,
            },
            Summary::Cone {
                apex: [1.0, 2.0, 3.0],
                axis: [0.0, 0.0, 1.0],
                cos: 0.8,
                sin: 0.6,
            },
        ),
        (
            Form::Sphere {
                centre: origin,
                radius: 2.0,
            },
            Summary::Sphere {
                centre: [1.0, 2.0, 3.0],
                radius: 2.0,
            },
        ),
        (
            Form::Torus {
                centre: origin,
                axis: z,
                major: 5.0,
                minor: 1.0,
            },
            Summary::Torus {
                centre: [1.0, 2.0, 3.0],
                axis: [0.0, 0.0, 1.0],
                major: 5.0,
                minor: 1.0,
            },
        ),
        (
            Form::ConicCylinder {
                conic: ellipse,
                along: z,
            },
            Summary::ConicCylinder {
                along: [0.0, 0.0, 1.0],
            },
        ),
        (
            Form::Revolved {
                origin,
                axis: z,
                meridian,
            },
            Summary::Revolved {
                origin: [1.0, 2.0, 3.0],
                axis: [0.0, 0.0, 1.0],
            },
        ),
        // Numbers a summary can't hold.
        (Form::Plane { n: z, d: f64::NAN }, Summary::Other),
        (
            Form::Cylinder {
                point: DVec3::splat(1e9),
                axis: z,
                radius: 8.0,
            },
            Summary::Other,
        ),
        (
            Form::ConicCylinder {
                conic: ellipse,
                along: 2.0 * z,
            },
            Summary::Other,
        ),
        (
            Form::Revolved {
                origin: DVec3::splat(f64::INFINITY),
                axis: z,
                meridian,
            },
            Summary::Other,
        ),
    ];
    for (form, summary) in cases {
        assert_eq!(Summary::of(&form), summary, "{form:?}");
        assert!(summary.valid());
    }
}

/// How far `p` is from the surface `summary` names (0 for
/// [`Summary::Other`]).
fn off_surface(summary: &Summary, p: glam::DVec3) -> f64 {
    use glam::DVec3;
    match *summary {
        Summary::Plane { n, d } => (DVec3::from(n).dot(p) - d).abs(),
        Summary::Cylinder {
            point,
            axis,
            radius,
        } => {
            let v = p - DVec3::from(point);
            (v - DVec3::from(axis) * v.dot(DVec3::from(axis))).length() - radius
        }
        Summary::Sphere { centre, radius } => (p - DVec3::from(centre)).length() - radius,
        Summary::Cone { .. }
        | Summary::Torus { .. }
        | Summary::ConicCylinder { .. }
        | Summary::Revolved { .. }
        | Summary::Other => 0.0,
    }
    .abs()
}

/// Checks that `picking` goes with `mesh`: a body per part, a face per
/// face and a flag per edge; each face's triangles on its surface, each
/// edge between two faces on both of theirs and of one body.
fn assert_picks_match(mesh: &RenderMesh, picking: &Picking) {
    let at = |v: u32| Vec3::from(mesh.positions()[v as usize]).as_dvec3();
    let faces = picking.faces();
    // Checked as the page checks them.
    Picking::from_parts(
        picking.bodies().to_vec(),
        faces.to_vec(),
        picking.closed().to_vec(),
        picking.tangents().to_vec(),
        picking.snaps().to_vec(),
        picking.corners().to_vec(),
        mesh,
    )
    .unwrap();
    for (f, face) in mesh.faces().enumerate() {
        for tri in face.chunks(3) {
            for &v in tri {
                let off = off_surface(&faces[f].summary, at(v));
                assert!(off < 1e-4, "{:?} is {off} off face {f}", at(v));
            }
            // A plane's normal points out, as the triangle's winding does.
            if let Summary::Plane { n, .. } = faces[f].summary {
                let [a, b, c] = [tri[0], tri[1], tri[2]].map(at);
                let normal = (b - a).cross(c - a);
                assert!(
                    normal.dot(n.into()) > 0.5 * normal.length(),
                    "face {f} faces {n:?}, its triangle {normal:?}"
                );
            }
        }
    }
    for (e, (polyline, &[a, b])) in mesh.polylines().zip(mesh.edge_faces()).enumerate() {
        if a == b {
            continue;
        }
        assert_eq!(picking.face_body(mesh, a), picking.face_body(mesh, b));
        for f in [a, b] {
            for &v in polyline {
                let off = off_surface(&faces[f as usize].summary, at(v));
                assert!(off < 1e-4, "{:?} is {off} off face {f} of edge {e}", at(v));
            }
        }
    }
}

/// How many of `mesh`'s edges are between two faces, not creases.
fn chains(mesh: &RenderMesh) -> usize {
    (mesh.edge_faces().iter()).filter(|[a, b]| a != b).count()
}

#[test]
fn the_answer_s_picking_tables_name_the_example_s_faces_and_edges() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let Response::Regenerated { mesh, picking, .. } = handle(regenerate(&editor, None)) else {
        panic!("regeneration failed");
    };
    assert_picks_match(&mesh, &picking);
    // Top, bottom, four sides and the hole's wall, whose quarter walls
    // are one face; its rims are closed edges, the plate's twelve open.
    let faces = picking.faces();
    assert_eq!(faces.len(), 7);
    assert_eq!(picking.bodies(), [body]);
    assert!((0..7).all(|f| picking.face_body(&mesh, f) == Some(body)));
    let tops = (faces.iter()).filter(|face| {
        face.summary
            == Summary::Plane {
                n: [0.0, 0.0, 1.0],
                d: 10.0,
            }
    });
    assert_eq!(tops.count(), 1);
    let hole = (faces.iter())
        .filter(|face| matches!(face.summary, Summary::Cylinder { radius, .. } if (radius - 8.0).abs() < 1e-9));
    assert_eq!(hole.count(), 1, "{faces:?}");
    assert_eq!(mesh.edge_count(), 14);
    assert_eq!(chains(&mesh), 14);
    assert_eq!(picking.closed().iter().filter(|&&c| c).count(), 2);
    // A sketch on the top would find it by its key.
    let top = faces.iter().position(|face| {
        face.summary
            == Summary::Plane {
                n: [0.0, 0.0, 1.0],
                d: 10.0,
            }
    });
    let top_key = faces[top.unwrap()].key;
    assert!(
        (0..mesh.edge_count() as u32)
            .any(|e| picking.edge_keys(&mesh, e).unwrap().contains(&top_key))
    );
}

/// Two plates stacked flush and a boss joined through both: the plates'
/// sides and holes merge into one face each, which the tables list once.
#[test]
fn faces_merged_flush_are_one_entry_in_the_tables() {
    use crate::history::tests::{add_extrude, disc, plate_below, two_sides};
    use varde_document::Targets;
    let mut editor = Editor::new(Document::example());
    plate_below(&mut editor);
    let extent = two_sides(editor.document(), "15", "5");
    add_extrude(
        &mut editor,
        disc((20.0, 0.0), 5.0),
        extent,
        Operation::Join(Targets::default()),
    );
    let Response::Regenerated {
        mesh,
        picking,
        failed,
        bodies,
        ..
    } = handle(regenerate(&editor, None))
    else {
        panic!("regeneration failed");
    };
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(bodies.len(), 1);
    assert_picks_match(&mesh, &picking);
    let faces = picking.faces();
    let hole = (faces.iter())
        .filter(|face| matches!(face.summary, Summary::Cylinder { radius, .. } if (radius - 8.0).abs() < 1e-9));
    assert_eq!(hole.count(), 1, "the hole through both plates is one face");
    let side = (faces.iter()).filter(|face| {
        face.summary
            == Summary::Plane {
                n: [1.0, 0.0, 0.0],
                d: 30.0,
            }
    });
    let side: Vec<&PickFace> = side.collect();
    assert_eq!(side.len(), 1, "the plates' side at +x is one face");
    // It took the lower plate's key in as an alias.
    assert_eq!(side[0].aliases.len(), 1);
    // The boss's wall above and below the plates: two faces of one key.
    let boss = (faces.iter())
        .filter(|face| matches!(face.summary, Summary::Cylinder { radius, .. } if (radius - 5.0).abs() < 1e-9));
    let boss: Vec<&PickFace> = boss.collect();
    assert_eq!(boss.len(), 2);
    assert_eq!(boss[0].key, boss[1].key);
}

#[test]
fn picking_tables_are_deterministic_and_the_same_from_the_cache() {
    let mut editor = Editor::new(Document::example());
    crate::history::tests::plate_below(&mut editor);
    let tables = |response: Response| match response {
        Response::Regenerated { mesh, picking, .. } => (mesh, picking),
        Response::Failed { error, .. } => panic!("{error}"),
        Response::Exported { .. } => panic!("not a regeneration"),
    };
    let first = tables(handle(regenerate(&editor, None)));
    let second = tables(handle(regenerate(&editor, None)));
    assert_eq!(first, second);
    let mut regenerator = Regenerator::default();
    regenerator.handle(regenerate(&editor, None));
    // The scene from the cache, and one joined again from the bodies'
    // drawings kept.
    let cached = tables(regenerator.handle(regenerate(&editor, None)));
    assert_eq!(cached, first);
    let mut hidden = editor.clone();
    let below = hidden.document().bodies()[1].id;
    hidden.apply(Command::SetVisible(below, false)).unwrap();
    regenerator.handle(regenerate(&hidden, None));
    let rejoined = tables(regenerator.handle(regenerate(&editor, None)));
    assert_eq!(rejoined, first);
    // Two bodies: the second's faces and edges follow the first's.
    let (mesh, picking) = &first;
    let bodies: Vec<BodyId> = (0..picking.faces().len() as u32)
        .map(|f| picking.face_body(mesh, f).unwrap())
        .collect();
    assert_eq!(bodies.len(), 14);
    assert!(
        bodies[..7]
            .iter()
            .all(|&b| b == editor.document().bodies()[0].id)
    );
    assert!(bodies[7..].iter().all(|&b| b == below));
    assert_eq!(mesh.edge_count(), 28);
    assert!(
        mesh.edge_faces()[14..]
            .iter()
            .all(|faces| faces.iter().all(|&f| f >= 7))
    );
}

/// The cache counts what a body's picking tables and a scene's hold.
#[test]
fn the_cache_counts_the_picking_tables() {
    let editor = Editor::new(Document::example());
    let mut regenerator = Regenerator::default();
    let Response::Regenerated { mesh, picking, .. } = regenerator.handle(regenerate(&editor, None))
    else {
        panic!("regeneration failed");
    };
    let mesh_bytes = std::mem::size_of_val(mesh.positions())
        + std::mem::size_of_val(mesh.normals())
        + std::mem::size_of_val(mesh.indices())
        + std::mem::size_of_val(mesh.edge_vertices());
    let table_bytes =
        std::mem::size_of_val(picking.faces()) + std::mem::size_of_val(picking.closed());
    // The body's drawing and the scene, each with its tables.
    assert!(regenerator.cache().bytes() >= 2 * (mesh_bytes + table_bytes));
    let (total, _) = regenerator.cache().audit();
    assert_eq!(regenerator.cache().bytes(), total);
}

/// The tables of a regeneration that worked, checked against its mesh.
fn picked(response: Response) -> (Arc<RenderMesh>, Arc<Picking>, Vec<BodyId>) {
    let Response::Regenerated {
        mesh,
        picking,
        bodies,
        failed,
        ..
    } = response
    else {
        panic!("regeneration failed: {response:?}");
    };
    assert!(failed.is_empty(), "{failed:?}");
    assert_picks_match(&mesh, &picking);
    // Every part of a body the answer lists.
    let listed: Vec<BodyId> = bodies.iter().map(|&(body, _)| body).collect();
    assert!(picking.bodies().iter().all(|b| listed.contains(b)));
    (mesh, picking, listed)
}

/// Hiding and showing bodies changes the tables with the mesh: a hidden
/// body's faces are gone, the other's indices start at 0, and showing it
/// again gives the first tables back.
#[test]
fn picking_tables_follow_the_bodies_shown() {
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let mut regenerator = Regenerator::default();
    let (_, both, _) = picked(regenerator.handle(regenerate(&editor, None)));
    assert_eq!(both.bodies(), [top, below]);

    editor.apply(Command::SetVisible(top, false)).unwrap();
    let (mesh, picking, listed) = picked(regenerator.handle(regenerate(&editor, None)));
    assert_eq!(listed, [top, below], "hidden bodies keep their boxes");
    assert_eq!(picking.bodies(), [below]);
    assert_eq!(picking.faces().len(), 7);
    assert_eq!(picking.faces(), &both.faces()[7..]);
    // Below the top plate, which is gone.
    assert!(mesh.positions().iter().all(|p| p[2] <= 0.0));

    editor.apply(Command::SetVisible(below, false)).unwrap();
    let (mesh, picking, _) = picked(regenerator.handle(regenerate(&editor, None)));
    assert_eq!(mesh.triangle_count(), 0);
    assert_eq!(*picking, Picking::default());

    editor.apply(Command::SetVisible(top, true)).unwrap();
    let (_, picking, _) = picked(regenerator.handle(regenerate(&editor, None)));
    assert_eq!(picking.faces(), &both.faces()[..7]);
    editor.apply(Command::SetVisible(below, true)).unwrap();
    let (_, picking, _) = picked(regenerator.handle(regenerate(&editor, None)));
    assert_eq!(picking, both);
}

/// The same shown bodies in another order: a scene filed by mesh keys
/// alone would hand back the other order's tables.
#[test]
fn picking_tables_name_each_body_even_when_their_solids_match() {
    // Two bodies of the same extrude settings on the same sketch: equal
    // solids, so equal mesh keys if the key held only the solid.
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let extrude = example_extrude(editor.document());
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    let twin = editor.document().bodies().last().unwrap().id;
    let mut regenerator = Regenerator::default();
    let (_, both, _) = picked(regenerator.handle(regenerate(&editor, None)));
    let shown = both.bodies();
    assert!(shown.contains(&top));
    for (hidden, kept) in [(top, twin), (twin, top)] {
        let mut one = editor.clone();
        one.apply(Command::SetVisible(hidden, false)).unwrap();
        let (_, picking, _) = picked(regenerator.handle(regenerate(&one, None)));
        assert_eq!(picking.bodies(), [kept], "{hidden:?} hidden");
    }
}

/// A join merging two plates: the merged body's faces are all the
/// holder's, none the consumed body's, in the committed answer and in a
/// draft's; and the tables follow an edit that changes a face's plane.
#[test]
fn picking_tables_follow_merges_drafts_and_edits() {
    use crate::history::tests::{add_extrude, disc, set_extrude, two_sides};
    use varde_document::Targets;
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let mut regenerator = Regenerator::default();
    let (_, apart, _) = picked(regenerator.handle(regenerate(&editor, None)));
    assert_eq!(apart.bodies(), [top, below]);

    // The join as a draft first.
    let mut probe = editor.clone();
    let extent = two_sides(editor.document(), "15", "5");
    let join = add_extrude(
        &mut probe,
        disc((20.0, 0.0), 5.0),
        extent,
        Operation::Join(Targets::default()),
    );
    let FeatureKind::Extrude(extrude) = &probe.document().feature(join).unwrap().kind else {
        unreachable!()
    };
    let mut sketched = probe.clone();
    sketched.undo();
    let draft = Draft {
        revision: 1,
        feature: None,
        kind: extrude.clone().into(),
    };
    let response = regenerator.handle(regenerate_with(&sketched, Some(draft)));
    let Response::Regenerated { draft, merged, .. } = &response else {
        panic!("regeneration failed");
    };
    assert_eq!(draft.as_ref().unwrap().error, None);
    assert_eq!(merged.len(), 1);
    let (consumed, holder) = merged[0];
    let (mesh, drafted, listed) = picked(response);
    assert!(!listed.contains(&consumed));
    assert_eq!(drafted.bodies(), [holder]);
    assert_eq!(drafted.faces().len(), mesh.face_count());

    // Committed: the same tables.
    let (_, joined, _) = picked(regenerator.handle(regenerate(&probe, None)));
    assert_eq!(joined.faces(), drafted.faces());
    assert_eq!(joined.bodies(), [holder]);
    assert!([top, below].contains(&holder) && holder != consumed);

    // The top plate made thicker: its top face's plane follows, in the
    // answer after the scene before was cached.
    let plate = editor.document().features()[1].id;
    let twelve = crate::history::tests::length(probe.document(), "12");
    set_extrude(&mut probe, plate, |extrude| {
        extrude.extent = Extent::OneSide(twelve);
    });
    let (_, thicker, _) = picked(regenerator.handle(regenerate(&probe, None)));
    let plane = |d: f64| Summary::Plane {
        n: [0.0, 0.0, 1.0],
        d,
    };
    assert!(joined.faces().iter().any(|f| f.summary == plane(10.0)));
    assert!(!thicker.faces().iter().any(|f| f.summary == plane(10.0)));
    assert!(thicker.faces().iter().any(|f| f.summary == plane(12.0)));
}

/// The ellipse of half-axes `a` along x and `b` along y about `centre`,
/// counterclockwise in four quarter arcs, as curve `curve`.
fn ellipse_loop(centre: DVec2, a: f64, b: f64, curve: u64) -> varde_kernel::Loop {
    use varde_kernel::patch::Conic2;
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let at = |x: f64, y: f64| centre + DVec2::new(a * x, b * y);
    let quarters = [
        (at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)),
        (at(0.0, 1.0), at(-1.0, 1.0), at(-1.0, 0.0)),
        (at(-1.0, 0.0), at(-1.0, -1.0), at(0.0, -1.0)),
        (at(0.0, -1.0), at(1.0, -1.0), at(1.0, 0.0)),
    ];
    varde_kernel::Loop {
        segments: quarters
            .map(|(p0, c, p1)| varde_kernel::Segment {
                conic: Conic2::new(p0, c, w, p1).unwrap(),
                curve,
            })
            .to_vec(),
    }
}

/// An extrude on a tilted, moved plane: its caps' summaries are the
/// planes they lie on, facing out, a circle's wall a cylinder along the
/// plane's normal through the circle's centre, an ellipse's a conic
/// cylinder along it; every triangle lies on its face's surface.
#[test]
fn summaries_of_an_extrude_on_a_tilted_plane() {
    use glam::DVec3;
    use varde_kernel::{Budget, Frame, Profile, extrude};
    let frame = Frame {
        origin: DVec3::new(5.0, -3.0, 2.0),
        x: DVec3::new(0.6, 0.0, 0.8),
        y: DVec3::Y,
    };
    let normal = frame.normal();
    assert_eq!(normal, DVec3::new(-0.8, 0.0, 0.6));
    let body = Document::example().bodies()[0].id;
    let tol = Tolerance::default();
    for (ellipse, b) in [(false, 2.0), (true, 1.0)] {
        let centre = DVec2::new(1.0, -2.0);
        let profile = Profile {
            loops: vec![ellipse_loop(centre, 2.0, b, 7)],
        };
        let solid = extrude(&profile, &frame, 1.0, 4.0, 3, &tol, &Budget::DEFAULT).unwrap();
        let drawn = Drawn::new(&solid, &solid.topology(), &Display::new(&tol)).unwrap();
        let mut picking = Picking::default();
        picking.append(body, &drawn).unwrap();
        assert_picks_match(&drawn.mesh, &picking);
        let faces = picking.faces();
        assert_eq!(faces.len(), 3, "{faces:?}");
        let near = |a: [f64; 3], b: DVec3| (DVec3::from(a) - b).length() < 1e-12;
        let plane = |n: DVec3, d: f64| {
            faces.iter().any(|f| match f.summary {
                Summary::Plane { n: m, d: e } => near(m, n) && (e - d).abs() < 1e-12,
                _ => false,
            })
        };
        let at = normal.dot(frame.origin);
        assert!(plane(normal, at + 4.0), "{faces:?}");
        assert!(plane(-normal, -(at + 1.0)), "{faces:?}");
        let axis = frame.point(centre, 0.0);
        let wall = faces
            .iter()
            .find(|f| !matches!(f.summary, Summary::Plane { .. }));
        match wall.unwrap().summary {
            Summary::Cylinder {
                point,
                axis: along,
                radius,
            } if !ellipse => {
                assert!(near(along, normal) || near(along, -normal), "{along:?}");
                let off = DVec3::from(point) - axis;
                assert!(off.cross(normal).length() < 1e-12, "{point:?}");
                assert!((radius - 2.0).abs() < 1e-12);
            }
            Summary::ConicCylinder { along } if ellipse => {
                assert!(near(along, normal) || near(along, -normal), "{along:?}");
            }
            summary => panic!("the wall is {summary:?}"),
        }
        // The wall's two rims, closed edges.
        assert_eq!(drawn.mesh.edge_count(), 2);
        assert_eq!(picking.closed(), [true, true]);
    }
}

/// A pocket cut into the plate's bottom: its ceiling faces down into the
/// pocket, its walls out of the plate into it, as the plane summaries
/// say (a cut's faces are the tool's turned round).
#[test]
fn a_cut_s_faces_face_out_of_the_solid() {
    let mut editor = Editor::new(Document::example());
    crate::history::tests::add_pocket(&mut editor);
    let (_, picking, _) = picked(handle(regenerate(&editor, None)));
    let faces = picking.faces();
    // The plate's seven and the pocket's ceiling and four walls.
    assert_eq!(faces.len(), 12, "{faces:?}");
    let ceiling = Summary::Plane {
        n: [0.0, 0.0, -1.0],
        d: -4.0,
    };
    assert_eq!(faces.iter().filter(|f| f.summary == ceiling).count(), 1);
}

#[test]
fn shown_bodies_are_exported_as_manifolds() {
    let document = with_bodies(true);
    let [shown, hidden] = [0, 1].map(|i| document.bodies()[i].id);
    let cylinder = Solid::cylinder(glam::DVec3::ZERO, 10.0, 1.0, 0, &Tolerance::DEFAULT).unwrap();
    let solids = made([(shown, cylinder.clone()), (hidden, cuboid(3.0, 2.0))]);
    let exported = export(&document, &solids).unwrap();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].body, shown);
    assert_eq!(exported[0].name, document.bodies()[0].name);
    let display = Display::new(&document.tolerance());
    assert_eq!(exported[0].mesh, cylinder.manifold_mesh(&display).unwrap());
    // The example plate, from its history: one body, as much as it holds.
    let evaluation = evaluate(&document, &mut Cache::default());
    let exported = export(&document, &evaluation).unwrap();
    assert_eq!(exported.len(), 1);
    let solid = &evaluation.bodies[0].solid;
    let bounds = solid.bounds3().unwrap();
    let chord = display.chord((bounds.max - bounds.min).length());
    let volume = exported[0].mesh.volume();
    assert!((volume - solid.volume()).abs() <= chord * solid.area());
    // To the document's tolerance: a coarser one, fewer triangles.
    let mut editor = Editor::new(document.clone());
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    editor.apply(Command::SetTolerance(coarse)).unwrap();
    let solids = made([(shown, cylinder)]);
    let fine = export(&document, &solids).unwrap();
    let rough = export(editor.document(), &solids).unwrap();
    assert!(rough[0].mesh.triangles().len() < fine[0].mesh.triangles().len());
    // None shown, none exported.
    assert_eq!(
        export(&document, &made([(hidden, cuboid(0.0, 1.0))])),
        Ok(vec![])
    );
}

/// A tetrahedron with its corners 3e6 out: a solid, as the kernel's
/// check takes it, but past the positions a mesh may have.
fn far_tetrahedron() -> Solid {
    use varde_kernel::mesh::{Face, FaceName, FacePart, Form, MeshBuilder, Surface};
    let mut builder = MeshBuilder::new();
    let face = builder.face(Face {
        name: FaceName::new(1, FacePart::Split(0)),
        surface: Surface::Free,
        form: Form::Unknown,
        slack: 1.0,
    });
    let at = glam::DVec3::splat(3e6);
    let v = [
        glam::DVec3::ZERO,
        glam::DVec3::X,
        glam::DVec3::Y,
        glam::DVec3::Z,
    ]
    .map(|p| builder.vert(at + p * 100.0));
    for [a, b, c] in [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]] {
        builder.tri([v[a], v[b], v[c]], face);
    }
    Solid::new(builder.build().unwrap(), &Tolerance::DEFAULT).unwrap()
}

#[test]
fn a_body_that_cannot_be_exported_is_named() {
    let document = with_bodies(false);
    let body = &document.bodies()[0];
    let error = export(&document, &made([(body.id, far_tetrahedron())])).unwrap_err();
    assert_eq!(error.body, body.id);
    assert_eq!(error.name, body.name);
    assert_eq!(
        error.to_string(),
        format!(
            "{} can't be exported: the mesh is too far from the origin",
            body.name
        )
    );
}

#[test]
fn an_export_request_is_answered_with_the_committed_bodies() {
    let editor = Editor::new(Document::example());
    let request = Request::Export {
        export: 4,
        document: editor.snapshot(),
    };
    let mut regenerator = Regenerator::default();
    // After a regeneration, from its cache: the same bodies as fresh.
    regenerator.handle(regenerate(&editor, None));
    let Response::Exported { export: 4, result } = regenerator.handle(request.clone()) else {
        panic!("not an export's answer");
    };
    let bodies = result.unwrap();
    let evaluation = evaluate(editor.document(), &mut Cache::default());
    assert_eq!(bodies, export(editor.document(), &evaluation).unwrap());
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].mesh.volume() > 0.0);
    assert!(matches!(
        handle(request),
        Response::Exported { export: 4, result: Ok(fresh) } if fresh == bodies
    ));
    // A request with no generation, so the feed never takes it for a
    // model.
    let response = regenerator.handle(Request::Export {
        export: 5,
        document: editor.snapshot(),
    });
    assert_eq!(response.generation(), None);
}
