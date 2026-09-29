use super::*;
use crate::CheckError;

#[test]
fn undo_redo_roundtrip() {
    let mut editor = Editor::new(Document::example());
    let id = editor.document().bodies[0].id;

    editor.apply(Command::RemoveBody(id)).unwrap();
    assert!(editor.document().bodies.is_empty());

    editor.undo();
    assert_eq!(editor.document().bodies.len(), 1);

    editor.redo();
    assert!(editor.document().bodies.is_empty());
    assert!(!editor.can_redo());
}

#[test]
fn undo_and_redo_give_a_state_back_its_revision() {
    let mut editor = Editor::new(Document::example());
    let id = editor.document().bodies[0].id;
    let loaded = editor.revision();
    let generation = editor.generation();

    editor.apply(Command::SetVisible(id, false)).unwrap();
    let hidden = editor.revision();
    assert_ne!(hidden, loaded);
    editor.undo();
    assert_eq!(editor.revision(), loaded);
    editor.redo();
    assert_eq!(editor.revision(), hidden);

    // A new edit is a new state, even one undone to make room for it.
    editor.undo();
    editor.apply(Command::RemoveBody(id)).unwrap();
    assert_ne!(editor.revision(), loaded);
    assert_ne!(editor.revision(), hidden);

    // The generation grows through all of it.
    assert_eq!(
        editor.generation(),
        Generation::from(u64::from(generation) + 5)
    );
}

#[test]
fn snapshot_is_shared_and_unaffected_by_edits() {
    let mut editor = Editor::new(Document::example());
    let snapshot = editor.snapshot();
    assert!(Arc::ptr_eq(&snapshot, &editor.snapshot()));

    let id = editor.document().bodies[0].id;
    editor.apply(Command::RemoveBody(id)).unwrap();
    assert_eq!(snapshot.bodies.len(), 1);
    assert!(editor.document().bodies.is_empty());
}

#[test]
fn edits_that_change_nothing_are_dropped() {
    let mut editor = Editor::new(Document::example());
    let snapshot = editor.snapshot();
    let id = editor.document().bodies[0].id;
    let missing = BodyId(id.0 + 1);

    editor.apply(Command::RemoveBody(missing)).unwrap();
    editor.apply(Command::SetVisible(missing, false)).unwrap();
    editor.apply(Command::SetVisible(id, true)).unwrap();
    editor
        .apply(Command::Replace(Box::new(Document::example())))
        .unwrap();

    assert!(Arc::ptr_eq(&snapshot, &editor.snapshot()));
    assert_eq!(editor.revision(), Revision(0));
    assert!(!editor.can_undo());
}

fn add_cube() -> Command {
    Command::AddBody {
        name: "Cube".to_owned(),
        shape: varde_kernel::Shape::cuboid(glam::Vec3::ONE),
        position: glam::Vec3::ZERO,
    }
}

#[test]
fn adding_a_body_past_the_last_id_fails() {
    // `next_id` comes from the file, so it can be anything.
    let document = Document {
        next_id: u64::MAX,
        ..Document::default()
    };
    let document = Document::from_postcard(&document.to_postcard()).unwrap();
    let mut editor = Editor::new(document);
    assert_eq!(editor.apply(add_cube()), Err(EditError::OutOfIds));
    assert!(editor.document().bodies.is_empty());
    assert_eq!(editor.revision(), Revision(0));
    assert!(!editor.can_undo());

    let mut editor = Editor::new(Document {
        next_id: u64::MAX - 1,
        ..Document::default()
    });
    editor.apply(add_cube()).unwrap();
    assert_eq!(editor.document().bodies[0].id, BodyId(u64::MAX - 1));
    assert_eq!(editor.apply(add_cube()), Err(EditError::OutOfIds));
    assert_eq!(editor.document().bodies.len(), 1);
    assert_eq!(editor.revision(), Revision(1));
}

#[test]
fn check_refuses_ids_a_new_body_could_reuse() {
    let mut document = Document::example();
    assert_eq!(document.check(), Ok(()));
    document.next_id = 0;
    let first = document.bodies[0].id;
    assert_eq!(document.check(), Err(CheckError::NextId(first)));
    assert!(Document::from_postcard(&document.to_postcard()).is_err());

    let mut document = Document::example();
    document
        .add_body(
            "Twin",
            varde_kernel::Shape::cuboid(glam::Vec3::ONE),
            glam::Vec3::ZERO,
        )
        .unwrap();
    let second = document.bodies[1].id;
    assert_eq!(document.check(), Ok(()));
    assert_eq!(document.body(second).map(|b| b.name.as_str()), Some("Twin"));

    // Bodies out of id order would break lookups by id.
    let mut swapped = document.clone();
    swapped.bodies.swap(0, 1);
    let first = document.bodies[0].id;
    assert_eq!(swapped.check(), Err(CheckError::Order(first, second)));
    assert!(Document::from_postcard(&swapped.to_postcard()).is_err());

    document.bodies[1].id = document.bodies[0].id;
    assert!(document.check().is_err());
}

#[test]
fn check_refuses_long_names() {
    let mut document = Document::example();
    document.bodies[0].name = "é".repeat(crate::MAX_NAME_LEN / 2);
    assert_eq!(document.check(), Ok(()));
    // Long enough to overflow text shaping, yet a small file.
    document.bodies[0].name = "é".repeat(70_000);
    assert!(matches!(
        document.check(),
        Err(CheckError::NameLength(_, 140_000))
    ));
    assert!(matches!(
        Document::from_postcard(&document.to_postcard()),
        Err(crate::DecodeError { .. })
    ));
}

#[test]
fn check_refuses_geometry_out_of_bounds() {
    let refused = |edit: &dyn Fn(&mut Document)| {
        let mut document = Document::example();
        edit(&mut document);
        assert!(document.check().is_err());
        assert!(matches!(
            Document::from_postcard(&document.to_postcard()),
            Err(crate::DecodeError { .. })
        ));
    };
    let mut document = Document::example();
    document.bodies[0].position = Vec3::splat(-crate::MAX_COORD);
    document.bodies[0].shape = Shape::cuboid(Vec3::splat(crate::MAX_COORD));
    let mut sketch = crate::Sketch::default();
    let a = varde_sketch::PointId(0);
    sketch
        .points
        .push(glam::DVec2::splat(f64::from(crate::MAX_COORD)));
    sketch
        .constraints
        .push(varde_sketch::Constraint::Distance(a, a, 1.0));
    document.sketches.push(sketch);
    assert_eq!(document.check(), Ok(()));

    // NaN would make the document unequal to itself, so no-op edits count.
    for bad in [f32::NAN, f32::INFINITY, 1e20, 3e38] {
        refused(&|d| d.bodies[0].position = Vec3::new(0.0, bad, 0.0));
        refused(&|d| d.bodies[0].shape = Shape::cuboid(Vec3::new(1.0, 1.0, bad)));
    }
    for bad in [0.0, -1.0, f32::NAN] {
        refused(&|d| d.bodies[0].shape = Shape::cuboid(Vec3::new(2.0, bad, 2.0)));
    }
    let mut flat = Document::example();
    flat.bodies[0].shape = Shape::cuboid(Vec3::new(2.0, 0.0, 2.0));
    assert!(matches!(
        flat.check(),
        Err(CheckError::Shape(id, crate::ShapeError::Size(_))) if id == flat.bodies[0].id
    ));
    refused(&|d| {
        d.sketches.push(crate::Sketch::default());
        d.sketches[0].points.push(glam::DVec2::new(f64::NAN, 0.0));
    });
    refused(&|d| {
        let mut sketch = crate::Sketch::default();
        sketch.points.push(glam::DVec2::ZERO);
        let a = varde_sketch::PointId(0);
        sketch
            .constraints
            .push(varde_sketch::Constraint::Distance(a, a, f64::INFINITY));
        d.sketches.push(sketch);
    });
}

#[test]
fn check_refuses_unknown_sketch_points() {
    use varde_sketch::{Constraint, Entity, PointId};
    let refused = |sketch: crate::Sketch| {
        let mut document = Document::example();
        document.sketches.push(sketch);
        assert!(document.check().is_err());
        assert!(matches!(
            Document::from_postcard(&document.to_postcard()),
            Err(crate::DecodeError { .. })
        ));
    };
    let mut sketch = crate::Sketch::default();
    sketch.points.extend([glam::DVec2::ZERO, glam::DVec2::ONE]);
    let (a, b) = (PointId(0), PointId(1));
    sketch.entities.push(Entity::Arc {
        center: a,
        start: b,
        end: a,
    });
    sketch.constraints.push(Constraint::Vertical(a, b));
    let mut document = Document::example();
    document.sketches.push(sketch.clone());
    assert_eq!(document.check(), Ok(()));

    let mut line = crate::Sketch::default();
    line.entities.push(Entity::Line {
        start: PointId(0),
        end: PointId(u32::MAX),
    });
    refused(line);
    let mut arc = sketch.clone();
    arc.entities.push(Entity::Arc {
        center: a,
        start: b,
        end: PointId(2),
    });
    refused(arc);
    let mut distance = sketch;
    distance
        .constraints
        .push(Constraint::Distance(PointId(2), a, 1.0));
    let mut document = Document::example();
    document.sketches.push(distance.clone());
    assert_eq!(
        document.check(),
        Err(CheckError::Sketch(
            0,
            crate::SketchError::UnknownPoint {
                id: PointId(2),
                points: 2
            }
        ))
    );
    refused(distance);
}

#[test]
fn replace_is_one_undoable_edit() {
    let mut editor = Editor::new(Document::example());
    editor.apply(Command::Replace(Box::default())).unwrap();
    assert_eq!(*editor.document(), Document::default());
    assert_eq!(editor.revision(), Revision(1));
    editor.undo();
    assert_eq!(*editor.document(), Document::example());

    // Replacing it with what it is changes nothing.
    editor
        .apply(Command::Replace(Box::new(Document::example())))
        .unwrap();
    assert_eq!(editor.revision(), Revision(0));
    assert!(editor.can_redo());
}

#[test]
fn edits_that_fail_check_are_refused() {
    let mut editor = Editor::new(Document::example());
    for position in [Vec3::new(0.0, 2.0 * crate::MAX_COORD, 0.0), Vec3::NAN] {
        let add = Command::AddBody {
            name: "Far".to_owned(),
            shape: Shape::cuboid(Vec3::ONE),
            position,
        };
        assert!(matches!(
            editor.apply(add),
            Err(EditError::Invalid(CheckError::Position(..)))
        ));
    }
    let add = Command::AddBody {
        name: "é".repeat(crate::MAX_NAME_LEN),
        shape: Shape::cuboid(Vec3::ONE),
        position: Vec3::ZERO,
    };
    assert!(matches!(
        editor.apply(add),
        Err(EditError::Invalid(CheckError::NameLength(..)))
    ));

    let mut replacement = Document::example();
    replacement.next_id = 0;
    assert!(matches!(
        editor.apply(Command::Replace(Box::new(replacement))),
        Err(EditError::Invalid(_))
    ));

    assert_eq!(*editor.document(), Document::example());
    assert_eq!(editor.revision(), Revision(0));
    assert!(!editor.can_undo());
}

#[test]
fn undo_forgets_the_oldest_edits_past_the_cap() {
    let mut editor = Editor::new(Document::example());
    let id = editor.document().bodies[0].id;
    for i in 0..MAX_UNDO + 1 {
        editor.apply(Command::SetVisible(id, i % 2 == 1)).unwrap();
    }

    let mut undone = 0;
    while editor.can_undo() {
        editor.undo();
        undone += 1;
    }
    assert_eq!(undone, MAX_UNDO);
    // The first edit is forgotten, so undo stops after it.
    assert!(!editor.document().bodies[0].visible);

    while editor.can_redo() {
        editor.redo();
    }
    assert_eq!(editor.undo.len(), MAX_UNDO);
}

#[test]
fn add_cube_numbers_and_places_past_the_bodies() {
    let mut editor = Editor::new(Document::default());
    assert_eq!(
        editor.document().add_cube(),
        Command::AddBody {
            name: "Cube 1".to_owned(),
            shape: Shape::cuboid(Vec3::splat(CUBE_SIZE)),
            position: Vec3::ZERO,
        }
    );
    editor
        .apply(Command::AddBody {
            name: "Cube 7".to_owned(),
            shape: Shape::cuboid(Vec3::ONE),
            position: Vec3::new(4.0, 0.0, 0.0),
        })
        .unwrap();
    editor
        .apply(Command::AddBody {
            name: "Cube".to_owned(),
            shape: Shape::cuboid(Vec3::ONE),
            position: Vec3::ZERO,
        })
        .unwrap();
    let Command::AddBody { name, position, .. } = editor.document().add_cube() else {
        panic!("add_cube adds a body");
    };
    assert_eq!(name, "Cube 8");
    assert_eq!(position, Vec3::new(4.0 + 1.0 + CUBE_GAP, 0.0, 0.0));
}
