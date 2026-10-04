//! The fillet session: `F` on the example plate, edges picked and
//! toggled as a chamfer's (the edge session they share), the mock's
//! panel (Edges, Radius, Tangent chain), the kernel's stand-in failing
//! as too complex in the panel and Add anyway keeping it; with
//! regeneration filleting by its stand-in ([`varde_regen::testing`]),
//! the round previewed, the radius and Tangent chain drafted, a radius
//! of nothing refused, OK as one undo step; editing from the Timeline,
//! Cancel and undo; an edge an undo takes away said to be gone; the
//! rail's Fillet; the edges selected taken in, `F` again backing out; a
//! slot's rim lit whole.

use glam::DVec3;
use varde_document::{FeatureKind, Fillet};
use varde_regen::Summary;
use varde_view::{
    Edit, Look, MotionField, MotionKind, MotionLook, MotionPick, PanelHover, Picked, Picks,
    Selection, SelectionMode,
};

use super::chamfer::{
    BACK, FRONT, RIGHT, click_edge, edge_pick, picking, plate, shows, slot, straight,
};
use super::{Plates, character, enter, later_disc, plates};
use crate::tests::key_in;

/// The fillet the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<Fillet> {
    match plates.last_draft()?.1 {
        FeatureKind::Fillet(fillet) => Some(fillet),
        _ => None,
    }
}

/// The edges of the fillet being set up.
fn edges(plates: &Plates) -> Vec<varde_document::EdgeRef> {
    (plates
        .doc
        .motion
        .as_ref()
        .expect("a session")
        .blend
        .edges
        .refs)
        .clone()
}

/// Whether the model shown has a round of `radius` along X on `body`.
fn rounded(plates: &Plates, body: varde_document::BodyId, radius: f64) -> bool {
    let index = plates.doc.feed.pick_index();
    (index.body_faces(body)).any(|face| {
        matches!(index.picking().faces()[face as usize].summary,
            Summary::Cylinder { axis, radius: r, .. }
                if (r - radius).abs() < 1e-9 && axis[0].abs() > 1.0 - 1e-9)
    })
}

/// `F` with the example's only body: a fillet picking edges, the mock's
/// panel; a face refused; edges clicked picked, lit, listed sorted with
/// their lengths, a second click taking one out, a row hovered lighting
/// its edge; the preview fails as too complex (the kernel's fillet isn't
/// built), shown in the panel; OK waits, Add anyway keeps it as one undo
/// step, failing in the Timeline.
#[test]
fn f_picks_and_toggles_edges_and_add_anyway_keeps_the_too_complex_fillet() {
    let (mut plates, plate) = plate();
    let before = plates.doc.editor.document().clone();
    key_in(&mut plates.doc, character("f"));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Fillet);
    assert!(session.bodies.is_empty(), "its body is its edges'");
    assert_eq!(picking(&plates), MotionPick::Edges);
    for text in [
        "New fillet",
        "Edges",
        "Click edges",
        "Radius",
        "Tangent chain",
        "pick the edges to fillet",
    ] {
        assert!(shows(&plates, text), "{text}");
    }
    for text in ["Type", "Flip sides", "Distance"] {
        assert!(!shows(&plates, text), "{text}");
    }
    let picks = plates.doc.model_picking().expect("picking");
    assert_eq!(picks.picks, Picks::Edges);
    assert!(!plates.doc.motion_ready());
    assert!(plates.last_draft().is_none(), "nothing to fillet yet");

    let top = plates.face(
        plate,
        |s| matches!(s, Summary::Plane { n, .. } if n[2] > 0.5),
    );
    plates.click_at(plate, Picked::Face(top), DVec3::new(0.0, 15.0, 10.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only an edge can be filleted")
    );

    let front = edge_pick(&plates, plate, FRONT);
    click_edge(&mut plates, plate, FRONT);
    assert_eq!(edges(&plates).len(), 1);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
    assert_eq!(plates.doc.blend_lit(), [front.target]);
    assert!(shows(&plates, "Edge 1") && shows(&plates, "60 mm"));
    assert!(plates.doc.motion_ready());
    let fillet = drafted(&plates).expect("a fillet's draft");
    assert_eq!(fillet.radius.value, 2.0, "the mock's 2 mm");
    assert!(fillet.chains);
    assert!(shows(&plates, "1 edge · R2 mm · Tangent chain"));
    plates.doc.look(Look::Hover(None));
    let picked = plates.doc.motion_highlight().expect("lit").clone();
    plates.doc.look(Look::HoverPanel(Some(PanelHover::Edge(0))));
    assert_ne!(plates.doc.motion_highlight(), Some(&picked));
    plates.doc.look(Look::LeavePanel(PanelHover::Edge(0)));

    click_edge(&mut plates, plate, RIGHT);
    click_edge(&mut plates, plate, BACK);
    let picked = edges(&plates);
    assert_eq!(picked.len(), 3);
    assert!(
        picked
            .windows(2)
            .all(|pair| pair[0].order(&pair[1]).is_lt())
    );
    click_edge(&mut plates, plate, BACK);
    assert_eq!(edges(&plates).len(), 2);
    assert_eq!(drafted(&plates).unwrap().edges, edges(&plates));

    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("filleting Body 1 is too complex"), "{error}");
    assert!(shows(&plates, "Fillet fails"));
    assert!(shows(&plates, "Add anyway"));
    let state = plates.doc.motion_state().unwrap();
    assert!(!state.ready && state.accept);
    assert!(straight(&plates, plate, FRONT.0, FRONT.1).is_some());
    let features = plates.doc.editor.document().features().len();
    key_in(&mut plates.doc, enter());
    assert_eq!(plates.doc.editor.document().features().len(), features);

    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert!(matches!(&kind, FeatureKind::Fillet(stored) if stored.edges.len() == 2));
    assert_eq!(
        plates.doc.editor.document().feature(id).unwrap().name,
        "Fillet 1"
    );
    plates.answer();
    assert!(
        (plates.doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "the fillet fails"
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// With regeneration filleting by its stand-in: the front edge rounded
/// in the preview (a cylinder of the radius, the edge gone from the
/// model shown); another radius drafted and previewed; a radius of
/// nothing refused under its field; Tangent chain off drafted; OK adds
/// it as one undo step.
#[test]
fn the_radius_and_chain_are_previewed_and_ok_adds_one_undo_step() {
    varde_regen::testing::fillet_by_arcs();
    let (mut plates, plate) = plate();
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartFillet);
    click_edge(&mut plates, plate, FRONT);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(rounded(&plates, plate, 2.0), "the round");
    assert!(straight(&plates, plate, FRONT.0, FRONT.1).is_none());
    assert!(plates.doc.blend_lit().is_empty(), "rounded off");
    assert!(shows(&plates, "Edge 1") && !shows(&plates, "60 mm"));

    plates.input(MotionField::Radius, "3.5");
    assert_eq!(drafted(&plates).unwrap().radius.value, 3.5);
    plates.answer();
    assert!(rounded(&plates, plate, 3.5) && !rounded(&plates, plate, 2.0));
    assert!(shows(&plates, "1 edge · R3.5 mm · Tangent chain"));

    plates.input(MotionField::Radius, "0");
    assert!(!plates.doc.motion_ready());
    let state = plates.doc.motion_state().unwrap();
    assert!(state.fields[MotionField::Radius.index()].error.is_some());
    plates.input(MotionField::Radius, "1");
    assert!(plates.doc.motion_ready());
    plates.motion(MotionLook::Chain);
    let fillet = drafted(&plates).expect("a draft");
    assert!(!fillet.chains);
    assert_eq!(fillet.radius.value, 1.0);
    // No Flip for a fillet.
    plates.motion(MotionLook::Flip);
    assert_eq!(drafted(&plates), Some(fillet.clone()));
    plates.answer();
    assert!(rounded(&plates, plate, 1.0));

    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (_, kind) = plates.last_feature();
    assert_eq!(kind, FeatureKind::Fillet(fillet));
    plates.answer();
    assert!(rounded(&plates, plate, 1.0));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// The example plate with "Fillet 1" of its front top edge, 2 mm: the
/// plates holding it and the fillet.
fn filleted() -> (Plates, varde_document::FeatureId) {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartFillet);
    click_edge(&mut plates, plate, FRONT);
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    let (id, _) = plates.last_feature();
    let (doc, requests) = crate::tests::holding(plates.doc.editor.document().clone());
    let plates = Plates {
        doc,
        requests,
        bodies: [plate; 3],
    };
    (plates, id)
}

/// Editing a fillet from the Timeline opens it with its edges, radius
/// and Tangent chain, its preview the fillet; Cancel leaves no trace;
/// another radius and Add anyway sets it as one undo step; its edges
/// all taken out, it shows the body as of the feature.
#[test]
fn editing_a_fillet_from_the_timeline_cancel_and_undo() {
    let (mut plates, id) = filleted();
    let plate = plates.bodies[0];
    let before = plates.doc.editor.document().clone();
    let FeatureKind::Fillet(stored) = before.feature(id).unwrap().kind.clone() else {
        panic!("a fillet");
    };
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Fillet);
    assert_eq!(session.feature, Some(id));
    assert_eq!(session.blend.edges.refs, stored.edges);
    assert_eq!(session.bodies, [plate]);
    assert!(shows(&plates, "Fillet 1"));
    assert_eq!(session.fillet(), Some(stored.clone()));
    assert_eq!(
        plates.last_draft(),
        Some((Some(id), FeatureKind::Fillet(stored.clone())))
    );
    plates.input(MotionField::Radius, "4");
    plates.motion(MotionLook::Cancel);
    assert!(plates.doc.motion.is_none());
    assert_eq!(*plates.doc.editor.document(), before);

    plates.doc.look(Look::EditFeature(id));
    plates.input(MotionField::Radius, "4");
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let FeatureKind::Fillet(set) = &plates.doc.editor.document().feature(id).unwrap().kind else {
        panic!("a fillet");
    };
    assert_eq!(set.radius.value, 4.0);
    assert_eq!(
        plates.doc.editor.document().features().len(),
        before.features().len()
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);

    plates.doc.look(Look::EditFeature(id));
    plates.motion(MotionLook::DropEdge(stored.edges[0]));
    let (feature, kind) = plates.last_draft().expect("a draft");
    assert_eq!(feature, Some(id));
    assert!(matches!(kind, FeatureKind::Move(moved) if moved.bodies == [plate]));
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "pick the edges to fillet"));
}

/// An edge on a body an undo takes away is kept, said to be gone,
/// nothing previewed or committed, until a redo brings it back.
#[test]
fn an_edge_an_undo_takes_away_is_said_to_be_gone() {
    let mut plates = plates();
    let last = later_disc(&mut plates);
    plates.doc.look(Look::StartFillet);
    let index = plates.doc.feed.pick_index();
    let snaps = index.picking().snaps();
    let rim = (0..snaps.len() as u32)
        .find(|&edge| {
            index.body(Picked::Edge(edge)) == Some(last)
                && snaps[edge as usize]
                    .is_some_and(|at| DVec3::from(at).distance(DVec3::new(0.0, 30.0, 5.0)) < 1e-9)
        })
        .expect("the disc's top rim");
    plates.click_at(last, Picked::Edge(rim), DVec3::new(5.0, 30.0, 5.0));
    assert_eq!(edges(&plates).len(), 1);
    assert!(shows(&plates, "Ø10 mm"));
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::Undo);
    assert_eq!(edges(&plates).len(), 1, "kept");
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "A picked edge is gone"));
    assert!(drafted(&plates).is_none());
    plates.doc.update(Edit::Redo);
    assert!(plates.doc.motion_ready());
    assert!(drafted(&plates).is_some());
}

/// The rail's Modify set offers Fillet first, as the icon mock's; it
/// starts with the edges selected that it takes; `F` again backs out;
/// another body's edges are refused in the fillet's words.
#[test]
fn fillet_takes_the_edges_selected_and_f_backs_out() {
    let (mut plates, plate) = plate();
    plates.doc.pick.selection = Selection::new(SelectionMode::Edges { tangent: false });
    for ends in [FRONT, RIGHT] {
        let pick = edge_pick(&plates, plate, ends);
        plates.doc.look(Look::ClickModel {
            pick: Some(pick),
            add: true,
            double: false,
        });
    }
    plates.doc.look(Look::StartFillet);
    assert_eq!(edges(&plates).len(), 2);
    assert!(drafted(&plates).is_some());
    key_in(&mut plates.doc, character("f"));
    assert!(plates.doc.motion.is_none());

    let mut plates = super::plates();
    let [_, right, _] = plates.bodies;
    plates.doc.look(Look::StartFillet);
    click_edge(&mut plates, plate, FRONT);
    let index = plates.doc.feed.pick_index();
    let snaps = index.picking().snaps();
    let rim = (0..snaps.len() as u32)
        .find(|&edge| {
            index.body(Picked::Edge(edge)) == Some(right)
                && snaps[edge as usize]
                    .is_some_and(|at| DVec3::from(at).distance(DVec3::new(20.0, 0.0, 15.0)) < 1e-9)
        })
        .expect("the disc's top rim");
    plates.click_at(right, Picked::Edge(rim), DVec3::new(25.0, 0.0, 15.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("A fillet's edges are all on one body: pick edges of Body 1")
    );
    assert_eq!(edges(&plates).len(), 1);
}

/// With Tangent chain on, the slot's top front line lights with the
/// rest of its rim once picked; off, the line alone.
#[test]
fn a_tangent_chain_lights_whole() {
    let (mut plates, body) = slot();
    plates.doc.look(Look::StartFillet);
    let line = edge_pick(&plates, body, ([0.0, -3.0, 5.0], [10.0, -3.0, 5.0]));
    let Picked::Edge(edge) = line.target else {
        unreachable!()
    };
    let rim = plates.doc.feed.pick_index().tangent_chain(edge).to_vec();
    assert_eq!(rim.len(), 4);
    let selected = |plates: &Plates| {
        let highlight = plates.doc.motion_highlight().expect("lit");
        let mut edges = highlight.highlights.selected_edges.clone();
        edges.sort_unstable();
        edges
    };
    plates.doc.look(Look::ClickModel {
        pick: Some(line),
        add: false,
        double: false,
    });
    assert_eq!(selected(&plates), rim);
    plates.motion(MotionLook::Chain);
    assert_eq!(selected(&plates), [edge]);
}

/// With Tangent chain on, the list of the model's overlaps names a row
/// of an edge of a picked chain (the line picked, or another edge of its
/// rim) as the chain, ticked, which a click on it takes out whole;
/// with Tangent chain off, the line alone is ticked, as an edge.
#[test]
fn the_overlap_list_names_an_edge_of_a_picked_chain_as_the_chain() {
    use varde_view::OverlapNote;
    let (mut plates, body) = slot();
    plates.doc.look(Look::StartFillet);
    let line = edge_pick(&plates, body, ([0.0, -3.0, 5.0], [10.0, -3.0, 5.0]));
    let Picked::Edge(edge) = line.target else {
        unreachable!()
    };
    let rim = plates.doc.feed.pick_index().tangent_chain(edge).to_vec();
    let other = *rim.iter().find(|&&other| other != edge).expect("the rim");
    let other = varde_view::Pick {
        target: Picked::Edge(other),
        ..line
    };
    plates.doc.look(Look::ClickModel {
        pick: Some(line),
        add: false,
        double: false,
    });
    assert_eq!(edges(&plates).len(), 1);
    let list = varde_view::Overlaps {
        held: glam::DVec2::ZERO,
        at: glam::DVec2::ZERO,
        items: varde_view::OverlapItems::Model(vec![line, other]),
    };
    let notes = |plates: &Plates| -> Vec<OverlapNote> {
        let ticks = plates.doc.overlap_ticks().expect("the session's");
        ticks.iter().map(|tick| tick.note).collect()
    };
    plates.doc.look(Look::OpenOverlaps(list.clone()));
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, true]));
    assert_eq!(notes(&plates), [OverlapNote::Chain, OverlapNote::Chain]);
    plates.doc.look(Look::CloseOverlaps);
    plates.motion(MotionLook::Chain);
    plates.doc.look(Look::OpenOverlaps(list.clone()));
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, false]));
    assert_eq!(notes(&plates), [OverlapNote::None, OverlapNote::None]);
    plates.doc.look(Look::CloseOverlaps);
    plates.motion(MotionLook::Chain);
    plates.doc.look(Look::OpenOverlaps(list));
    plates.doc.look(Look::ToggleOverlap(1));
    assert!(edges(&plates).is_empty(), "the chain taken out");
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![false, false]));
}

/// What overlaps where the left button was held, listed in a fillet
/// session: its rows ticked as the fillet has their edges (not as the
/// model's selection), a tick picking one with the list kept open, a row
/// chosen taking one out as a click would.
#[test]
fn the_overlap_list_ticks_and_picks_the_fillet_s_edges() {
    let (mut plates, plate) = plate();
    let (front, right) = (
        edge_pick(&plates, plate, FRONT),
        edge_pick(&plates, plate, RIGHT),
    );
    // Selected before the session, which takes it.
    plates.doc.pick.selection = Selection::new(SelectionMode::Edges { tangent: false });
    plates.doc.look(Look::ClickModel {
        pick: Some(front),
        add: true,
        double: false,
    });
    plates.doc.look(Look::StartFillet);
    assert_eq!(edges(&plates).len(), 1);
    let list = varde_view::Overlaps {
        held: glam::DVec2::ZERO,
        at: glam::DVec2::ZERO,
        items: varde_view::OverlapItems::Model(vec![front, right]),
    };
    plates.doc.look(Look::OpenOverlaps(list));
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, false]));
    plates.doc.look(Look::ToggleOverlap(1));
    assert!(plates.doc.overlaps.is_some());
    assert_eq!(edges(&plates).len(), 2);
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, true]));
    plates.doc.look(Look::ChooseOverlap {
        index: 0,
        add: false,
    });
    assert!(plates.doc.overlaps.is_none());
    let left = edges(&plates);
    assert_eq!(left.len(), 1);
    assert!(
        left[0]
            .near
            .distance(DVec3::from(RIGHT.0).midpoint(DVec3::from(RIGHT.1)))
            < 1.0
    );
}

/// The rim of `body` about (20, 0) at height `z` on the model shown, as
/// a pick at its point at x 25.
fn disc_rim(plates: &Plates, body: varde_document::BodyId, z: f64) -> varde_view::Pick {
    let index = plates.doc.feed.pick_index();
    let snaps = index.picking().snaps();
    let rim = (0..snaps.len() as u32)
        .find(|&edge| {
            index.body(Picked::Edge(edge)) == Some(body)
                && snaps[edge as usize]
                    .is_some_and(|at| DVec3::from(at).distance(DVec3::new(20.0, 0.0, z)) < 1e-9)
        })
        .expect("the disc's rim");
    varde_view::Pick {
        model: index.model(),
        target: Picked::Edge(rim),
        body,
        at: DVec3::new(25.0, 0.0, z),
        snap: None,
    }
}

/// A fillet of the right disc's top rim, then a join merging the disc
/// into the plate after it: edited, its rim is found and lit on the
/// plate drawing it, the disc's other rim is picked on it as the
/// disc's, the plate's own edges refused, and OK keeps it on the disc.
#[test]
fn editing_a_fillet_whose_body_a_later_join_merged() {
    let mut plates = super::plates();
    let [_, right, _] = plates.bodies;
    plates.doc.look(Look::StartFillet);
    let top = disc_rim(&plates, right, 15.0);
    plates.doc.look(Look::ClickModel {
        pick: Some(top),
        add: false,
        double: false,
    });
    assert_eq!(edges(&plates).len(), 1);
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let (id, _) = plates.last_feature();
    let extent = crate::tests::two_sides(plates.doc.editor.document(), "12", "1");
    crate::tests::add_disc(
        &mut plates.doc.editor,
        (14.0, 0.0),
        extent,
        varde_document::Operation::Join(varde_document::Targets::default()),
    );
    plates.doc.sync();
    plates.answer();
    assert!(
        (plates.doc.feed.merged_bodies().iter()).any(|&(consumed, _)| consumed == right),
        "the disc merged"
    );
    plates.doc.look(Look::EditFeature(id));
    plates.answer();
    let holder = plates.doc.feed.pick_index().body(top.target);
    let lit = plates.doc.blend_lit();
    assert_eq!(lit.len(), 1, "its rim lit");
    assert_ne!(holder, Some(right), "drawn by the plate");
    let bottom = disc_rim(&plates, holder.unwrap(), -5.0);
    plates.doc.look(Look::ClickModel {
        pick: Some(bottom),
        add: false,
        double: false,
    });
    assert_eq!(edges(&plates).len(), 2, "{:?}", plates.doc.notice);
    assert!(edges(&plates).iter().all(|edge| edge.body == right));
    let front = super::chamfer::edge_pick(&plates, holder.unwrap(), FRONT);
    plates.doc.look(Look::ClickModel {
        pick: Some(front),
        add: false,
        double: false,
    });
    assert_eq!(edges(&plates).len(), 2);
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    plates.doc.update(Edit::CommitMotion);
    assert!(plates.doc.motion.is_none(), "{:?}", plates.doc.edit_error);
    let Some(FeatureKind::Fillet(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        panic!("the fillet");
    };
    assert_eq!(stored.edges.len(), 2);
    assert_eq!(stored.body(), Some(right));
}

/// The list of what overlaps held open in a fillet session while each
/// tick's preview comes: its rows are found again on each new model by
/// their names, so the next tick picks, and a tick takes out, as on the
/// model the list was opened on. With the stand-in rounding, an edge
/// rounded off in the preview isn't there to list: its row stays,
/// marked removed, to take it out.
#[test]
fn the_overlap_list_follows_each_preview() {
    let (mut plates, plate) = plate();
    let (front, right, back) = (
        edge_pick(&plates, plate, FRONT),
        edge_pick(&plates, plate, RIGHT),
        edge_pick(&plates, plate, BACK),
    );
    plates.doc.look(Look::StartFillet);
    let list = varde_view::Overlaps {
        held: glam::DVec2::ZERO,
        at: glam::DVec2::ZERO,
        items: varde_view::OverlapItems::Model(vec![front, right, back]),
    };
    plates.doc.look(Look::OpenOverlaps(list));
    plates.doc.look(Look::ToggleOverlap(0));
    assert_eq!(edges(&plates).len(), 1);
    plates.answer();
    assert!(plates.doc.feed.draft_error().is_some(), "the kernel's stub");
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, false, false]));
    plates.doc.look(Look::ToggleOverlap(1));
    assert_eq!(edges(&plates).len(), 2);
    plates.answer();
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, true, false]));
    plates.doc.look(Look::ToggleOverlap(0));
    assert_eq!(edges(&plates).len(), 1);
    plates.answer();
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![false, true, false]));
    plates.doc.look(Look::ToggleOverlap(1));
    assert!(edges(&plates).is_empty());
    plates.answer();

    // Rounded off in the preview, the front edge's row stays, marked
    // removed and ticked, so it can be taken out from the list; hovered,
    // it lights nothing, as it's on no model shown.
    varde_regen::testing::fillet_by_arcs();
    plates.doc.look(Look::ToggleOverlap(0));
    plates.answer();
    assert!(plates.doc.feed.draft_error().is_none());
    let rows = |plates: &Plates| {
        let listed = plates.doc.overlaps.as_ref().expect("still open");
        let varde_view::OverlapItems::Model(picks) = &listed.list.items else {
            panic!("the model's");
        };
        assert!(
            picks
                .iter()
                .all(|pick| pick.model == plates.doc.feed.model())
        );
        picks.clone()
    };
    use varde_view::OverlapNote;
    let notes = |plates: &Plates| -> Vec<OverlapNote> {
        let ticks = plates.doc.overlap_ticks().expect("the session's");
        ticks.iter().map(|tick| tick.note).collect()
    };
    assert_eq!(rows(&plates).len(), 3, "the front edge kept");
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, false, false]));
    assert_eq!(
        notes(&plates),
        [OverlapNote::Removed, OverlapNote::None, OverlapNote::None]
    );
    plates.doc.look(Look::HoverOverlap(Some(0)));
    assert!(plates.doc.pick.hover().is_none());
    plates.doc.look(Look::HoverOverlap(None));
    // The back edge ticked too, rounded off alike.
    plates.doc.look(Look::ToggleOverlap(2));
    assert_eq!(edges(&plates).len(), 2, "the front and back edges");
    plates.answer();
    assert!(plates.doc.feed.draft_error().is_none());
    assert_eq!(rows(&plates).len(), 3);
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, false, true]));
    // The front unticked from the list: taken out, and its edge, back in
    // the next preview, found again as it was.
    plates.doc.look(Look::ToggleOverlap(0));
    assert_eq!(edges(&plates).len(), 1, "the back edge alone");
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![false, false, true]));
    plates.answer();
    assert_eq!(rows(&plates).len(), 3);
    assert_eq!(
        notes(&plates),
        [OverlapNote::None, OverlapNote::None, OverlapNote::Removed]
    );
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![false, false, true]));
    plates.doc.look(Look::ToggleOverlap(1));
    assert_eq!(edges(&plates).len(), 2);
}

/// Two edges of one tangent chain picked apart (the line with Tangent
/// chain off, then an arc of its rim), then Tangent chain on: the rim
/// lights whole as both pick it, and a click on another of its edges
/// takes the chain out, both edges, not one of them leaving the rim lit
/// as picked still.
#[test]
fn a_click_on_a_chain_takes_out_every_edge_picked_on_it() {
    let (mut plates, body) = slot();
    plates.doc.look(Look::StartFillet);
    plates.motion(MotionLook::Chain);
    let line = super::chamfer::edge_pick(&plates, body, ([0.0, -3.0, 5.0], [10.0, -3.0, 5.0]));
    let Picked::Edge(edge) = line.target else {
        unreachable!()
    };
    let rim = plates.doc.feed.pick_index().tangent_chain(edge).to_vec();
    assert_eq!(rim.len(), 4);
    let index = plates.doc.feed.pick_index();
    let arcs: Vec<u32> = (rim.iter().copied())
        .filter(|&other| {
            other != edge
                && index
                    .chain_keys(other)
                    .and_then(|keys| index.edge_ends(other, &keys))
                    .is_none()
        })
        .collect();
    assert_eq!(arcs.len(), 2);
    let at_arc = |arc: u32, plates: &Plates| {
        let at = plates.doc.feed.pick_index().chain_point(arc).unwrap();
        varde_view::Pick {
            target: Picked::Edge(arc),
            at,
            ..line
        }
    };
    for pick in [line, at_arc(arcs[0], &plates)] {
        plates.doc.look(Look::ClickModel {
            pick: Some(pick),
            add: false,
            double: false,
        });
    }
    assert_eq!(edges(&plates).len(), 2);
    plates.motion(MotionLook::Chain);
    plates.doc.look(Look::ClickModel {
        pick: Some(at_arc(arcs[1], &plates)),
        add: false,
        double: false,
    });
    assert!(edges(&plates).is_empty(), "{:?}", edges(&plates));
    assert!(plates.doc.blend_lit().is_empty());
}

mod fuzz;
