use std::collections::BTreeSet;

use varde_document::{BodyId, Document, FeatureKind, Named, Opacity, Tint};
use varde_sketch::{Curve, Id, LinkKind, Sketch};

use super::objects::{ObjectMenu, ObjectsView, consumed_note, objects};
use super::rows::Rows;
use super::sketch_tab::{GeometryRow, geometry, geometry_point_name, geometry_rows};
use super::*;
use crate::{Edit, LinkRow, RowMenu, SketchState};
use varde_expr::LengthUnit;

#[test]
fn a_size_the_constraints_set_is_noted_driven_and_one_a_dimension_gives_faint() {
    use crate::testing::{dimension, plate};
    use varde_sketch::{Analysis, Measure};
    let mut sketch = plate(20.0, 10.0, 3.0);
    let [bottom, right, top] = [0, 1, 2].map(|k| sketch.curves[k].id);
    let label = glam::DVec2::new(0.0, -5.0);
    dimension(&mut sketch, Measure::Length(bottom), "40 mm", true, label);
    // The bottom's length given, the top's set by it, the right's
    // free: only the top's is driven.
    let analysis = Analysis {
        sized: BTreeSet::from([bottom, top]),
        solved: true,
        ..Analysis::default()
    };
    let selection = BTreeSet::new();
    let mut state = SketchState::plain(&sketch, &selection, None);
    state.analysis = Some(&analysis);
    let mut laid = crate::testing::Laid::new(geometry(state, 400.0), iced::Size::new(300.0, 400.0));
    let texts: Vec<String> = laid.texts().into_iter().map(|shown| shown.text).collect();
    let after = |name: &str| {
        let at = texts.iter().position(|text| text == name).unwrap();
        texts[at + 1..]
            .iter()
            .take_while(|text| !text.starts_with("Line") && !text.starts_with("Circle"))
            .cloned()
            .collect::<Vec<_>>()
    };
    let [bottom, right, top] = [bottom, right, top].map(|id| sketch.curve(id).unwrap().name());
    assert_eq!(after(&bottom), ["40 mm"]);
    assert_eq!(after(&right), ["20 mm"]);
    assert_eq!(after(&top), ["=", "40 mm"]);
}

#[test]
fn a_merged_body_is_noted_in_its_holder() {
    use varde_document::Editor;
    let mut editor = Editor::new(Document::example());
    let FeatureKind::Extrude(extrude) = &editor.document().features()[1].kind else {
        panic!("the example's second feature is its extrude");
    };
    let command = editor.document().add_feature(extrude.clone().into());
    editor.apply(command).unwrap();
    let [top, below] = [0, 1].map(|k| editor.document().bodies()[k].id);
    let merged = [(below, top)];
    let document = editor.document();
    assert_eq!(consumed_note(document, &merged, top), None);
    assert_eq!(
        consumed_note(document, &merged, below).as_deref(),
        Some("in Body 1")
    );
    assert_eq!(consumed_note(document, &[], below), None);

    let texts = |merged: &[(BodyId, BodyId)]| -> Vec<String> {
        let objects = objects(ObjectsView {
            merged,
            ..ObjectsView::plain(document)
        });
        let mut laid = crate::testing::Laid::new(objects, iced::Size::new(300.0, 400.0));
        laid.texts().into_iter().map(|shown| shown.text).collect()
    };
    let shown = texts(&merged);
    let at = |text: &str| shown.iter().position(|shown| shown == text);
    // The note is on Body 2's row, after its name.
    let note = at("in Body 1").unwrap_or_else(|| panic!("{shown:?}"));
    assert_eq!(at("Body 2"), Some(note - 1), "{shown:?}");
    assert!(!texts(&[]).iter().any(|text| text.starts_with("in ")));

    // The row hovered is drawn over the plain one, which shows through
    // a translucent highlight: its note is laid out in the same place
    // in both, though the hovered one has its bin.
    let objects = objects(ObjectsView {
        merged: &merged,
        ..ObjectsView::plain(document)
    });
    let mut laid = crate::testing::Laid::new(objects, iced::Size::new(300.0, 400.0));
    let notes: Vec<_> = (laid.texts().into_iter())
        .filter(|shown| shown.text == "in Body 1")
        .map(|shown| shown.bounds)
        .collect();
    assert_eq!(notes.len(), 2, "plain and hovered");
    assert_eq!(notes[0], notes[1]);
}

/// Objects lists the origin objects first, in a group of their own.
#[test]
fn the_origin_objects_are_listed_first() {
    let document = Document::example();
    let objects = objects(ObjectsView::plain(&document));
    let mut laid = crate::testing::Laid::new(objects, iced::Size::new(300.0, 600.0));
    let shown: Vec<String> = laid.texts().into_iter().map(|shown| shown.text).collect();
    let at = |text: &str| shown.iter().position(|shown| shown == text);
    let names = [
        "Origin", "X axis", "Y axis", "Z axis", "XY plane", "XZ plane", "YZ plane", "Bodies",
    ];
    let places: Vec<_> = names.iter().map(|name| at(name)).collect();
    assert!(places.iter().all(Option::is_some), "{shown:?}");
    assert!(places.is_sorted(), "{shown:?}");
}

/// A body's menu has an Opacity row between Hide and Delete, the
/// slider's value beside it; a menu without an opacity, none.
#[test]
fn a_body_s_menu_has_an_opacity_row() {
    let body = Document::example().bodies()[0].id;
    let menu = |opacity| {
        let menu = ObjectMenu {
            on: RowMenu::Body(body),
            open: true,
            edit: None,
            rename: Message::Look(Look::StartRename(Named::Body(body))),
            opacity,
            color: None,
            look_hovered: false,
            delete: Message::Edit(Edit::RemoveBody(body)),
        };
        let toggle = Some(Message::Edit(Edit::ToggleVisible(body)));
        let view = menu.view(Icon::Body, true, toggle, true);
        let mut laid = crate::testing::Laid::new(view, iced::Size::new(300.0, 400.0));
        let shown = laid.texts().into_iter().map(|shown| shown.text);
        shown.collect::<Vec<_>>()
    };
    let shown = menu(Some((body, Opacity::new(40).unwrap())));
    let rows = ["Rename", "Hide", "Opacity", "40 %", "Delete"];
    assert_eq!(shown, rows);
    let shown = menu(Some((body, Opacity::MAX)));
    assert_eq!(shown, ["Rename", "Hide", "Opacity", "100 %", "Delete"]);
    assert_eq!(menu(None), ["Rename", "Hide", "Delete"]);
}

/// A body's menu has a Colour heading with Hue and Saturation rows
/// after its opacity, their values beside them, and the heading says
/// Default, the values dashes, while the body has no colour of its own.
#[test]
fn a_body_s_menu_has_colour_rows() {
    let body = Document::example().bodies()[0].id;
    let menu = |color| {
        let menu = ObjectMenu {
            on: RowMenu::Body(body),
            open: true,
            edit: None,
            rename: Message::Look(Look::StartRename(Named::Body(body))),
            opacity: Some((body, Opacity::MAX)),
            color: Some((body, color)),
            look_hovered: false,
            delete: Message::Edit(Edit::RemoveBody(body)),
        };
        let view = menu.view(Icon::Body, true, None, true);
        let mut laid = crate::testing::Laid::new(view, iced::Size::new(300.0, 500.0));
        let shown = laid.texts().into_iter().map(|shown| shown.text);
        shown.collect::<Vec<_>>()
    };
    let shown = menu(Tint::new(200, 30));
    let rows = [
        "Rename", "Opacity", "100 %", "Colour", "200°", "30 %", "Delete",
    ];
    assert_eq!(shown, rows);
    let shown = menu(None);
    let rows = [
        "Rename", "Opacity", "100 %", "Colour", "Default", "–", "–", "Delete",
    ];
    assert_eq!(shown, rows);
}

#[test]
fn only_the_rows_in_view_are_laid_out() {
    let rows = Rows::in_view(1000, 10.0 * ROW_HEIGHT + 5.0, 4.0 * ROW_HEIGHT);
    assert_eq!(rows.shown, 10..15);
    assert_eq!(rows.before, 10.0 * ROW_HEIGHT);
    assert_eq!(rows.after, 985.0 * ROW_HEIGHT);

    // All that fit, from the top.
    let rows = Rows::in_view(3, 0.0, 10.0 * ROW_HEIGHT);
    assert_eq!((rows.before, rows.shown, rows.after), (0.0, 0..3, 0.0));

    // A stale offset past the end shows the last rows.
    let rows = Rows::in_view(20, 1e9, 4.0 * ROW_HEIGHT);
    assert_eq!(rows.shown, 15..20);
    assert_eq!(rows.after, 0.0);
    let rows = Rows::in_view(3, 1e9, 10.0 * ROW_HEIGHT);
    assert_eq!(rows.shown, 0..3);

    // Nonsense offsets and heights show something from the top.
    let rows = Rows::in_view(20, f32::NAN, f32::NAN);
    assert_eq!(rows.shown, 0..1);
    assert_eq!(Rows::in_view(0, 50.0, 100.0).shown, 0..0);
}

/// The Geometry list groups the sketch's own items, then its
/// projected and intersected links, under a header row each, empty
/// groups left out, a curve's points under it while it's unfolded.
#[test]
fn the_geometry_list_groups_own_geometry_and_links() {
    use glam::DVec2;
    use varde_sketch::{LinkShape, SketchEdit};

    let design = varde_sketch::Design {
        max: 1e6,
        units: LengthUnit::Mm,
        params: varde_expr::Params::EMPTY,
    };
    let row = |link: Id, kind: LinkKind| LinkRow {
        link,
        kind,
        source: "Edge of Body 1".into(),
        broken: None,
        profiles: false,
        sketch_face: false,
    };
    let none = BTreeSet::new();
    // Nothing yet: Geometry alone, empty.
    let empty = Sketch::default();
    assert_eq!(
        geometry_rows(&empty, &[], &none, &BTreeSet::new()),
        [GeometryRow::Header(GeometryGroup::Own, 0)]
    );

    // A link only: no Geometry group.
    let mut sketch = SketchEdit::AddLink {
        kind: LinkKind::Intersect,
    }
    .apply(&empty, &design)
    .unwrap();
    let intersected = sketch.links[0].id;
    let mut shape = LinkShape::default();
    let start = shape.point(DVec2::ZERO);
    let end = shape.point(DVec2::X);
    shape.curve(Curve::Line { start, end });
    sketch = SketchEdit::Relink(vec![(intersected, shape)])
        .apply(&sketch, &design)
        .unwrap();
    let links = [row(intersected, LinkKind::Intersect)];
    assert_eq!(
        geometry_rows(&sketch, &links, &none, &BTreeSet::new()),
        [
            GeometryRow::Header(GeometryGroup::Intersected, 1),
            GeometryRow::Link(0)
        ]
    );

    // A line of its own, a point of its own and a projected link too:
    // its own first, the link's line and points not among them, the
    // line's points under it while it's unfolded, the loose point
    // after the curves.
    let a = sketch.add_point(DVec2::new(0.0, 5.0)).unwrap();
    let b = sketch.add_point(DVec2::new(5.0, 5.0)).unwrap();
    let line = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    sketch.add_point(DVec2::new(9.0, 9.0)).unwrap();
    sketch = SketchEdit::AddLink {
        kind: LinkKind::Project,
    }
    .apply(&sketch, &design)
    .unwrap();
    let projected = sketch.links[1].id;
    let links = [
        row(intersected, LinkKind::Intersect),
        row(projected, LinkKind::Project),
    ];
    let own_line = sketch.curves.len() - 1;
    let rows = |folded: &BTreeSet<GeometryGroup>, expanded: &BTreeSet<Id>| {
        geometry_rows(&sketch, &links, folded, expanded)
    };
    assert_eq!(
        rows(&none, &BTreeSet::new()),
        [
            GeometryRow::Header(GeometryGroup::Own, 2),
            GeometryRow::Curve(own_line),
            GeometryRow::Point {
                at: 4,
                child: false
            },
            GeometryRow::Header(GeometryGroup::Projected, 1),
            GeometryRow::Link(1),
            GeometryRow::Header(GeometryGroup::Intersected, 1),
            GeometryRow::Link(0),
        ]
    );
    let expanded = BTreeSet::from([line]);
    assert_eq!(
        rows(&none, &expanded)[..5],
        [
            GeometryRow::Header(GeometryGroup::Own, 4),
            GeometryRow::Curve(own_line),
            GeometryRow::Point { at: 2, child: true },
            GeometryRow::Point { at: 3, child: true },
            GeometryRow::Point {
                at: 4,
                child: false
            },
        ]
    );
    // A folded group lists its header alone.
    let folded = BTreeSet::from([GeometryGroup::Own, GeometryGroup::Intersected]);
    assert_eq!(
        rows(&folded, &expanded),
        [
            GeometryRow::Header(GeometryGroup::Own, 4),
            GeometryRow::Header(GeometryGroup::Projected, 1),
            GeometryRow::Link(1),
            GeometryRow::Header(GeometryGroup::Intersected, 1),
        ]
    );
}

#[test]
fn a_spline_unfolded_lists_its_handle_s_tip_then_its_end() {
    let mut sketch = Sketch::default();
    let (spline, fit, tip) = crate::testing::handled_spline(&mut sketch);
    let (_, handle) = sketch.handle(tip).unwrap();
    let place = |id: Id| {
        sketch
            .points
            .iter()
            .position(|point| point.id == id)
            .unwrap()
    };
    let rows = geometry_rows(&sketch, &[], &BTreeSet::new(), &BTreeSet::from([spline]));
    let child = |id| GeometryRow::Point {
        at: place(id),
        child: true,
    };
    assert_eq!(
        rows,
        [
            GeometryRow::Header(GeometryGroup::Own, 6),
            GeometryRow::Curve(0),
            child(fit[0]),
            child(fit[1]),
            child(fit[2]),
            child(tip),
            child(handle.end),
        ]
    );
    // Named as the two ends of one handle.
    let name = sketch.curve(spline).unwrap().name();
    let tip_point = &sketch.points[place(tip)];
    assert_eq!(
        sketch.point_name(tip_point),
        format!("End 1 of Handle 1 of {name}")
    );
    assert_eq!(
        sketch.point_name(&sketch.points[place(handle.end)]),
        format!("End 2 of Handle 1 of {name}")
    );
    // The list leaves the spline out of them, as they're under it.
    assert_eq!(
        geometry_point_name(&sketch, tip_point, true),
        "End 1 of Handle 1"
    );
    assert_eq!(
        geometry_point_name(&sketch, tip_point, false),
        format!("End 1 of Handle 1 of {name}")
    );
}
