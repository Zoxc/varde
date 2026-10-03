use std::cell::RefCell;
use std::rc::Rc;

use iced::keyboard::{self, key};
use varde_document::{BodyId, BodyOp, Combine, Document, Editor, Operation};
use varde_regen::Request;
use varde_view::{
    CombineLook, CombinePick, Edit, Look, Message as Ui, Mode, OperationKind, Panel, PanelHover,
    Picked,
};

use super::*;
use crate::tests::{
    add_disc, answer, holding, key_in, press_in, screen_texts, shown, texts, two_sides,
};

type Requests = Rc<RefCell<Vec<Request>>>;

/// The example's plate, "Body 1" (60 × 40 × 10 with a hole of radius 8
/// through its middle, from z 0 to 10), and two discs of radius 5 made as
/// new bodies, 15 mm up and 5 mm down through it: "Body 2" about
/// (20, 0) and "Body 3" about (-20, 0).
struct Plates {
    doc: Doc,
    requests: Requests,
    bodies: [BodyId; 3],
}

fn plates() -> Plates {
    let mut editor = Editor::new(Document::example());
    let new = || Operation::NewBody(BodyId::NEW);
    for center in [(20.0, 0.0), (-20.0, 0.0)] {
        let extent = two_sides(editor.document(), "15", "5");
        add_disc(&mut editor, center, extent, new());
    }
    let bodies = editor.document().bodies();
    let bodies = [bodies[0].id, bodies[1].id, bodies[2].id];
    let (doc, requests) = holding(editor.document().clone());
    Plates {
        doc,
        requests,
        bodies,
    }
}

impl Plates {
    fn combine(&mut self, message: CombineLook) {
        self.doc.look(Look::Combine(message));
    }

    fn answer(&mut self) {
        answer(&mut self.doc, &self.requests);
    }

    /// A click in the viewport on a face of `body` of the model shown.
    fn click(&mut self, body: BodyId) {
        let index = self.doc.feed.pick_index();
        let face = index.body_faces(body).next().expect("a face of the body");
        let pick = varde_view::Pick {
            model: index.model(),
            target: Picked::Face(face),
            body,
            at: glam::DVec3::ZERO,
            snap: None,
        };
        self.doc.look(Look::ClickModel {
            pick: Some(pick),
            add: false,
            double: false,
        });
    }

    /// The draft the last request waiting carries, if any, and the combine
    /// it edits.
    fn last_draft(&self) -> Option<(Option<FeatureId>, Combine)> {
        let requests = self.requests.borrow();
        let Request::Regenerate { draft, .. } = requests.last()? else {
            return None;
        };
        let draft = draft.as_ref()?;
        let FeatureKind::Combine(combine) = &draft.kind else {
            panic!("the draft is a combine's");
        };
        Some((draft.feature, combine.clone()))
    }

    /// The combines of the document.
    fn combines(&self) -> Vec<(FeatureId, Combine)> {
        (self.doc.editor.document().features().iter())
            .filter_map(|feature| match &feature.kind {
                FeatureKind::Combine(combine) => Some((feature.id, combine.clone())),
                _ => None,
            })
            .collect()
    }

    /// Sets up a union of Body 1 with Body 2 and commits it: its id.
    fn commit_union(&mut self, keep_tools: bool) -> FeatureId {
        let [plate, right, _] = self.bodies;
        self.doc.look(Look::StartCombine);
        self.click(plate);
        self.doc.look(Look::ClickBody {
            body: right,
            add: false,
        });
        if keep_tools {
            self.combine(CombineLook::KeepTools);
        }
        self.doc.update(Edit::CommitCombine);
        self.answer();
        self.combines().last().expect("a combine").0
    }
}

fn character(c: &str) -> keyboard::Key {
    keyboard::Key::Character(c.into())
}

fn enter() -> keyboard::Key {
    keyboard::Key::Named(key::Named::Enter)
}

#[test]
fn b_starts_it_the_viewport_and_objects_pick_and_enter_commits_one_undo_step() {
    let mut plates = plates();
    let [plate, right, left] = plates.bodies;
    let before = plates.doc.editor.document().clone();
    key_in(&mut plates.doc, character("b"));
    let session = plates.doc.combine.as_ref().expect("a session");
    assert_eq!(session.target, None);
    assert_eq!(session.picking, CombinePick::Target);
    assert!(plates.last_draft().is_none());
    // The cursor picks the model for it, and the measure tool doesn't
    // start (Sketch, Extrude and Revolve would take its place).
    assert!(plates.doc.picks());
    assert!(press_in(&plates.doc, character("i")).is_none());

    // A click in the viewport picks the target, the clicks go on to the
    // tools, and Objects' rows pick them.
    plates.click(plate);
    let session = plates.doc.combine.as_ref().unwrap();
    assert_eq!(session.target, Some(plate));
    assert_eq!(session.picking, CombinePick::Tools);
    assert!(!plates.doc.combine_ready());
    // The target isn't a tool.
    plates.click(plate);
    assert!(plates.doc.combine.as_ref().unwrap().tools.is_empty());
    for body in [left, right] {
        plates.doc.look(Look::ClickBody { body, add: false });
    }
    let session = plates.doc.combine.as_ref().unwrap();
    assert_eq!(session.tools, [right, left], "kept sorted");
    assert!(plates.doc.combine_ready());
    // Clicked again, a tool is taken out.
    plates.doc.look(Look::ClickBody {
        body: left,
        add: false,
    });
    assert_eq!(plates.doc.combine.as_ref().unwrap().tools, [right]);

    // The preview is the combine as a draft, using its tool up.
    let (feature, draft) = plates.last_draft().expect("a draft");
    assert_eq!(feature, None);
    assert_eq!(
        draft,
        Combine {
            target: plate,
            tools: vec![right],
            op: BodyOp::Union,
            keep_tools: false,
        }
    );
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert_eq!(plates.doc.feed.merged_bodies(), [(right, plate)]);
    // Objects shows the tool faint, in the target, while it's previewed.
    plates.doc.look(Look::SelectPanel(Panel::Objects));
    let shown = screen_texts(&plates.doc);
    assert!(shown.contains(&"in Body 1".to_owned()), "{shown:?}");
    // The status bar tells of it.
    assert!(
        shown.contains(&"· Body 1 with 1 tool · Union".to_owned()),
        "{shown:?}"
    );

    let revision = plates.doc.editor.revision();
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.combine.is_none());
    let combines = plates.combines();
    assert_eq!(combines.len(), 1);
    assert_eq!(combines[0].1, draft);
    let name = &plates
        .doc
        .editor
        .document()
        .feature(combines[0].0)
        .unwrap()
        .name;
    assert_eq!(name, "Combine 1");
    assert_eq!(plates.doc.selected_feature, Some(combines[0].0));
    plates.answer();
    assert_eq!(plates.doc.feed.merged_bodies(), [(right, plate)]);
    // One undo step.
    plates.doc.update(Edit::Undo);
    assert_eq!(plates.doc.editor.revision(), revision);
    assert_eq!(*plates.doc.editor.document(), before);
}

#[test]
fn esc_and_cancel_leave_no_trace() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    let before = plates.doc.editor.document().clone();
    let revision = plates.doc.editor.revision();
    for leave in [
        Look::Escape,
        Look::Combine(CombineLook::Cancel),
        Look::StartCombine,
    ] {
        plates.doc.look(Look::StartCombine);
        plates.click(plate);
        plates.click(right);
        assert!(plates.last_draft().is_some());
        plates.answer();
        plates.doc.look(leave);
        assert!(plates.doc.combine.is_none());
        // The model is asked for again without the draft.
        assert!(plates.last_draft().is_none());
        plates.answer();
        assert!(plates.doc.feed.merged_bodies().is_empty());
        assert_eq!(*plates.doc.editor.document(), before);
        assert_eq!(plates.doc.editor.revision(), revision);
    }
}

#[test]
fn the_selection_gives_it_its_bodies() {
    let mut plates = plates();
    let [plate, right, left] = plates.bodies;
    for body in [left, plate, right] {
        plates.doc.look(Look::ClickBody { body, add: true });
    }
    plates.doc.look(Look::StartCombine);
    let session = plates.doc.combine.as_ref().unwrap();
    assert_eq!(session.target, Some(left));
    assert_eq!(session.tools, [plate, right]);
    assert_eq!(session.picking, CombinePick::Tools);
    assert!(plates.doc.combine_ready());
}

#[test]
fn chips_take_bodies_out_and_the_fields_choose_what_clicks_pick() {
    let mut plates = plates();
    let [plate, right, left] = plates.bodies;
    plates.doc.look(Look::StartCombine);
    plates.click(plate);
    plates.click(right);
    plates.click(left);
    // Through the panel itself: Body 3's chip's button.
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(plates.doc.view_in(Mode::Light), size, &mut renderer);
    let shown_texts = texts(&mut ui, &renderer);
    let chip = (shown_texts.iter())
        .find(|text| text.text == "Body 3" && text.bounds.x > size.width - 300.0)
        .expect("Body 3's chip");
    // Its button is at the chip's right end, by the panel's edge.
    let button = iced::Point::new(size.width - 12.0 - 10.0 - 2.0 - 9.0, chip.bounds.center_y());
    let sent = crate::tests::clicked(&mut ui, &mut renderer, button);
    drop(ui);
    assert!(
        sent.iter().any(
            |message| matches!(message, Ui::Look(Look::Combine(CombineLook::Drop(b))) if *b == left)
        ),
        "{sent:?}"
    );
    for message in sent {
        if let Ui::Look(look) = message {
            plates.doc.look(look);
        }
    }
    assert_eq!(plates.doc.combine.as_ref().unwrap().tools, [right]);
    // Dropping the target hands the clicks to it.
    plates.combine(CombineLook::Drop(plate));
    let session = plates.doc.combine.as_ref().unwrap();
    assert_eq!(
        (session.target, session.picking),
        (None, CombinePick::Target)
    );
    assert!(plates.last_draft().is_none());
    // A tool picked as the target stops being a tool.
    plates.click(right);
    let session = plates.doc.combine.as_ref().unwrap();
    assert_eq!(session.target, Some(right));
    assert!(session.tools.is_empty());
    // The Target field picks the target again.
    plates.combine(CombineLook::Picking(CombinePick::Target));
    plates.click(left);
    let session = plates.doc.combine.as_ref().unwrap();
    assert_eq!(session.target, Some(left));
    assert_eq!(session.picking, CombinePick::Tools);
}

#[test]
fn editing_reopens_it_with_its_values() {
    let mut plates = plates();
    let [plate, right, left] = plates.bodies;
    let combine = plates.commit_union(true);
    assert!(
        plates.doc.feed.merged_bodies().is_empty(),
        "the tool is kept"
    );
    let revision = plates.doc.editor.revision();

    plates.doc.look(Look::EditFeature(combine));
    let session = plates.doc.combine.as_ref().expect("a session");
    assert_eq!(session.feature, Some(combine));
    assert_eq!(session.target, Some(plate));
    assert_eq!(session.tools, [right]);
    assert!(session.keep_tools);
    assert_eq!(session.op, BodyOp::Union);
    let shown = screen_texts(&plates.doc);
    // In the toolbar's pill, the Timeline and the panel's head.
    let named = shown.iter().filter(|text| *text == "Combine 1").count();
    assert!(named >= 3, "{shown:?}");
    // OK with nothing changed writes nothing.
    plates.doc.update(Edit::CommitCombine);
    assert!(plates.doc.combine.is_none());
    assert_eq!(plates.doc.editor.revision(), revision);

    plates.doc.look(Look::EditFeature(combine));
    plates.combine(CombineLook::Operation(BodyOp::Subtract));
    plates.click(left);
    let (feature, draft) = plates.last_draft().unwrap();
    assert_eq!(feature, Some(combine));
    assert_eq!(draft.op, BodyOp::Subtract);
    plates.doc.update(Edit::CommitCombine);
    let combines = plates.combines();
    assert_eq!(combines.len(), 1);
    assert_eq!(
        combines[0].1,
        Combine {
            target: plate,
            tools: vec![right, left],
            op: BodyOp::Subtract,
            keep_tools: true,
        }
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(plates.doc.editor.revision(), revision);
}

#[test]
fn a_body_merged_before_picks_the_one_holding_it() {
    let mut plates = plates();
    let [plate, right, left] = plates.bodies;
    let first = plates.commit_union(false);
    let document = plates.doc.editor.document();
    assert_eq!(
        plates.doc.feed.merged_before(document, None).holder(right),
        Some(plate)
    );
    assert_eq!(
        plates
            .doc
            .feed
            .merged_before(document, Some(first))
            .holder(right),
        None
    );
    // Objects lists it faint in Body 1, and its row picks Body 1.
    plates.doc.look(Look::SelectPanel(Panel::Objects));
    assert!(screen_texts(&plates.doc).contains(&"in Body 1".to_owned()));
    plates.doc.look(Look::StartCombine);
    plates.doc.look(Look::ClickBody {
        body: left,
        add: false,
    });
    plates.doc.look(Look::ClickBody {
        body: right,
        add: false,
    });
    let session = plates.doc.combine.as_ref().unwrap();
    assert_eq!(session.target, Some(left));
    assert_eq!(session.tools, [plate]);
    // Edited, the first combine sees Body 2 as itself.
    plates.doc.look(Look::Escape);
    plates.doc.look(Look::EditFeature(first));
    plates.combine(CombineLook::Drop(right));
    plates.doc.look(Look::ClickBody {
        body: right,
        add: false,
    });
    assert_eq!(plates.doc.combine.as_ref().unwrap().tools, [right]);
}

#[test]
fn the_highlight_shows_the_target_and_the_tools() {
    let mut plates = plates();
    let [plate, right, left] = plates.bodies;
    plates.doc.look(Look::StartCombine);
    plates.click(plate);
    plates.click(left);
    plates.combine(CombineLook::KeepTools);
    plates.answer();
    let index = plates.doc.feed.pick_index();
    let faces = |body| {
        let mut faces: Vec<u32> = index.body_faces(body).collect();
        faces.sort_unstable();
        faces
    };
    let (plate_faces, left_faces, right_faces) = (faces(plate), faces(left), faces(right));
    let hover = varde_view::Pick {
        model: index.model(),
        target: Picked::Face(right_faces[0]),
        body: right,
        at: glam::DVec3::ZERO,
        snap: None,
    };
    plates.doc.look(Look::Hover(Some(hover)));
    let highlight = plates.doc.highlight().expect("a highlight");
    let mut selected = highlight.selected_faces.clone();
    let mut second = highlight.second_faces.clone();
    let mut hovered = highlight.hovered_faces.clone();
    selected.sort_unstable();
    second.sort_unstable();
    hovered.sort_unstable();
    assert_eq!(selected, plate_faces);
    assert_eq!(second, left_faces);
    assert_eq!(hovered, right_faces);
    // The selection's own comes back after.
    plates.doc.look(Look::Escape);
    plates.doc.look(Look::Hover(None));
    assert!(plates.doc.highlight().is_none());
}

#[test]
fn a_body_hovered_in_the_panel_is_lit_even_if_picked() {
    let mut plates = plates();
    let [plate, _, left] = plates.bodies;
    plates.doc.look(Look::StartCombine);
    plates.click(plate);
    plates.click(left);
    plates.answer();
    let index = plates.doc.feed.pick_index();
    let mut plate_faces: Vec<u32> = index.body_faces(plate).collect();
    plate_faces.sort_unstable();
    plates
        .doc
        .look(Look::HoverPanel(Some(PanelHover::Body(plate))));
    assert_eq!(
        plates.doc.combine_state().unwrap().hover,
        Some(PanelHover::Body(plate))
    );
    let highlight = plates.doc.highlight().expect("a highlight");
    let mut hovered = highlight.hovered_faces.clone();
    hovered.sort_unstable();
    assert_eq!(hovered, plate_faces);
    assert!(highlight.selected_faces.is_empty());
    // Left, the target is drawn as the target again.
    plates.doc.look(Look::HoverPanel(None));
    let highlight = plates.doc.highlight().expect("a highlight");
    assert!(highlight.hovered_faces.is_empty());
    assert!(!highlight.selected_faces.is_empty());
    // A body no longer named isn't hovered, whatever the panel last said.
    plates
        .doc
        .look(Look::HoverPanel(Some(PanelHover::Body(left))));
    plates.combine(CombineLook::Drop(left));
    assert_eq!(plates.doc.combine_state().unwrap().hover, None);
}

#[test]
fn a_read_only_document_has_no_session_and_one_body_has_nothing_to_combine() {
    let mut plates = plates();
    plates.doc.read_only = Some("read-only".to_owned());
    plates.doc.sync();
    plates.doc.look(Look::StartCombine);
    assert!(plates.doc.combine.is_none());
    assert!(press_in(&plates.doc, character("b")).is_none());

    let (mut doc, _requests) = crate::tests::example();
    assert!(!doc.combinable());
    assert!(press_in(&doc, character("b")).is_none());
    // Started anyway, the panel says why nothing can be done.
    doc.look(Look::StartCombine);
    let shown = screen_texts(&doc);
    assert!(
        shown.contains(&"There’s only one body: make another to combine with".to_owned()),
        "{shown:?}"
    );
}

#[test]
fn other_tools_end_it_and_it_ends_them() {
    let mut plates = plates();
    let [plate, _, _] = plates.bodies;
    plates.doc.look(Look::StartMeasure);
    plates.doc.look(Look::StartCombine);
    assert!(plates.doc.measure.is_none());
    assert!(plates.doc.combine.is_some());
    // Not with the measure tool; an extrude or a revolve takes its place,
    // and a new sketch.
    plates.doc.look(Look::StartMeasure);
    assert!(plates.doc.measure.is_none() && plates.doc.combine.is_some());
    plates.doc.look(Look::StartExtrude);
    assert!(plates.doc.combine.is_none() && plates.doc.extrude.is_some());
    plates.doc.look(Look::StartCombine);
    assert!(plates.doc.combine.is_none(), "not over an extrude");
    plates.doc.look(Look::Escape);
    plates.doc.look(Look::StartCombine);
    plates.doc.look(Look::StartRevolve);
    assert!(plates.doc.combine.is_none() && plates.doc.revolve.is_some());
    plates.doc.look(Look::Escape);
    plates.doc.look(Look::StartCombine);
    plates.doc.look(Look::PickPlane);
    assert!(plates.doc.combine.is_none() && plates.doc.picking_plane.is_some());
    plates.doc.look(Look::Escape);
    plates.doc.look(Look::StartCombine);
    // Editing an extrude drops it, and it drops an extrude.
    let extrude = plates.doc.editor.document().features()[1].id;
    plates.doc.look(Look::EditFeature(extrude));
    assert!(plates.doc.combine.is_none() && plates.doc.extrude.is_some());
    plates.doc.look(Look::StartCombine);
    assert!(plates.doc.combine.is_none(), "not over an extrude");
    plates.doc.look(Look::Escape);
    plates.doc.look(Look::StartCombine);
    plates.click(plate);
    // Entering a sketch ends it.
    let sketch = plates.doc.editor.document().features()[0].id;
    plates.doc.look(Look::EditFeature(sketch));
    assert!(plates.doc.combine.is_none());
}

#[test]
fn a_body_going_drops_it_from_the_session() {
    let mut plates = plates();
    let [plate, _, left] = plates.bodies;
    plates.doc.look(Look::StartCombine);
    plates.click(plate);
    plates.click(left);
    // The extrude making Body 3 removed, as an undo past it would.
    let maker = plates.doc.editor.document().body(left).unwrap().created_by;
    plates
        .doc
        .apply(varde_document::Command::RemoveFeature(maker));
    plates.doc.sync();
    assert!(plates.doc.editor.document().body(left).is_none());
    let session = plates.doc.combine.as_ref().expect("still set up");
    assert_eq!(session.target, Some(plate));
    assert!(session.tools.is_empty());
    assert!(!plates.doc.combine_ready());
}

#[test]
fn the_extrude_making_a_combined_body_says_why_it_stays_a_new_body() {
    let mut plates = plates();
    plates.commit_union(false);
    let maker = plates.doc.editor.document().features()[3].id;
    plates.doc.look(Look::EditFeature(maker));
    assert!(plates.doc.extrude_state().unwrap().held.is_none());
    plates
        .doc
        .look(Look::Extrude(varde_view::ExtrudeLook::Operation(
            OperationKind::Join,
        )));
    let state = plates.doc.extrude_state().unwrap();
    let held = state.held.clone().expect("a note");
    assert_eq!(
        held,
        "Combine 1 combines Body 2, so this stays a new body: take Body 2 out of Combine 1 or \
         delete it first"
    );
    assert!(!state.ready);
    let shown = screen_texts(&plates.doc);
    assert!(
        shown
            .iter()
            .any(|text| text.starts_with("Combine 1 combines Body 2")),
        "{shown:?}"
    );
    // OK can't be pressed, and nothing is written.
    let revision = plates.doc.editor.revision();
    plates.doc.update(Edit::CommitExtrude);
    assert_eq!(plates.doc.editor.revision(), revision);
    // Back to a new body, it can.
    plates
        .doc
        .look(Look::Extrude(varde_view::ExtrudeLook::Operation(
            OperationKind::NewBody,
        )));
    assert!(plates.doc.extrude_state().unwrap().held.is_none());
}

#[test]
fn the_timeline_shows_its_icon_and_operation() {
    let mut plates = plates();
    let combine = plates.commit_union(false);
    plates.doc.look(Look::SelectPanel(Panel::Timeline));
    let shown = screen_texts(&plates.doc);
    assert!(shown.contains(&"Combine 1".to_owned()), "{shown:?}");
    assert!(shown.contains(&"Union".to_owned()), "{shown:?}");
    assert_eq!(plates.doc.selected_feature, Some(combine));
    // Selected, the status bar says what it combines.
    assert!(
        shown.contains(&"Body 1 with Body 2 · Union".to_owned()),
        "{shown:?}"
    );
}

#[test]
fn deleting_a_tool_s_maker_takes_the_combine_with_it() {
    let mut plates = plates();
    let combine = plates.commit_union(false);
    let maker = plates.doc.editor.document().features()[3].id;
    plates.doc.update(Edit::RemoveFeature(maker));
    let prompt = plates.doc.delete_prompt().expect("asked first");
    let names: Vec<&str> = prompt.features.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"Combine 1"), "{names:?}");
    plates.doc.update(Edit::ConfirmDelete);
    assert!(plates.doc.editor.document().feature(combine).is_none());
}

#[test]
fn a_click_in_the_viewport_picks_the_body_under_the_cursor() {
    use iced::mouse::{Button, Cursor, Event};
    let mut plates = plates();
    let [plate, _, _] = plates.bodies;
    plates.doc.look(Look::StartCombine);
    // Over the middle of the viewport, the plate's top shows from Home.
    let at = iced::Point::new(780.0, 450.0);
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(plates.doc.view_in(Mode::Light), size, &mut renderer);
    let mut sent = Vec::new();
    for event in [
        iced::Event::Mouse(Event::CursorMoved { position: at }),
        iced::Event::Mouse(Event::ButtonPressed(Button::Left)),
        iced::Event::Mouse(Event::ButtonReleased(Button::Left)),
    ] {
        let _ = ui.update(
            &[event],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
    }
    drop(ui);
    for message in sent {
        if let Ui::Look(look) = message {
            plates.doc.look(look);
        }
    }
    assert_eq!(plates.doc.combine.as_ref().unwrap().target, Some(plate));
    // The body under the cursor shows hovered.
    assert!(plates.doc.pick.hover().is_some());
    assert!(!plates.doc.highlight().unwrap().selected_faces.is_empty());
}

/// Whatever the selection's mode, a combine's cursor picks faces, so a
/// click anywhere on a body picks it (an edges-only mode would pick
/// nothing on a face), and it's back to the mode's once it ends.
#[test]
fn the_cursor_picks_faces_whatever_the_selection_s_mode() {
    use varde_view::{Picks, Selection, SelectionMode};
    let mut plates = plates();
    let [plate, ..] = plates.bodies;
    plates.doc.pick.selection = Selection::new(SelectionMode::Edges { tangent: false });
    let picks = |doc: &Doc| doc.model_picking().map(|picking| picking.picks);
    assert_eq!(picks(&plates.doc), Some(Picks::Edges));
    plates.doc.look(Look::StartCombine);
    assert_eq!(picks(&plates.doc), Some(Picks::Faces));
    plates.click(plate);
    assert_eq!(plates.doc.combine.as_ref().unwrap().target, Some(plate));
    plates.doc.look(Look::Escape);
    assert_eq!(picks(&plates.doc), Some(Picks::Edges));
}

/// Bodies picked before the model shows a join merging them (committed,
/// not answered yet) move on to the body holding them once it does, as a
/// click then would pick: the combine would otherwise fail naming them.
#[test]
fn picks_follow_a_merge_the_model_shows_late() {
    for tool_is_holder in [false, true] {
        let mut plates = plates();
        let [plate, right, left] = plates.bodies;
        // A join over the plate and the right disc: it merges the disc
        // into the plate.
        let join = Operation::Join(Default::default());
        fuzz::add_disc_in(&mut plates.doc, (20.0, 0.0), join);
        plates.doc.sync();
        assert!(plates.doc.feed.merged_bodies().is_empty());
        plates.doc.look(Look::StartCombine);
        let tool = if tool_is_holder { plate } else { left };
        for body in [right, tool] {
            plates.doc.look(Look::ClickBody { body, add: false });
        }
        assert_eq!(plates.doc.combine.as_ref().unwrap().target, Some(right));
        plates.answer();
        assert_eq!(plates.doc.feed.merged_bodies(), [(right, plate)]);
        let session = plates.doc.combine.as_ref().unwrap();
        assert_eq!(session.target, Some(plate));
        if tool_is_holder {
            // Now the target: taken out of the tools.
            assert!(session.tools.is_empty());
            assert!(plates.last_draft().is_none());
            continue;
        }
        assert_eq!(session.tools, [left]);
        // The preview asked for again, of the bodies as they are now.
        let (_, draft) = plates.last_draft().expect("a draft");
        assert_eq!((draft.target, draft.tools), (plate, vec![left]));
        plates.answer();
        assert_eq!(plates.doc.feed.draft_error(), None);
    }
}

mod fuzz;
