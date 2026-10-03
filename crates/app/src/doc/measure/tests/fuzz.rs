//! Random sequences of what can happen while measuring: the tool started
//! and left, picks of every kind (faces, edges, vertices, snap dots,
//! bodies by a double-click or their rows), hovers, edits with picks
//! alive (units, visibility, opacity, a join merging a picked body, undo
//! and redo), and answers coming in order, late, out of order or stale.
//! After each step: the document is only ever changed by the edits, the
//! answer shown is the newest, the highlight is built only from it, and
//! what the panel shows (and copies) is what the kernel measures of the
//! same solid; leaving with `Esc` leaves no trace.

use std::cell::RefCell;

use glam::DVec3;
use varde_document::{Command, Editor, Opacity, Operation, Targets};
use varde_kernel::measure::{self, EdgeShape, Measured, Pick as KernelPick, Target};
use varde_kernel::{Budget, Topology};
use varde_regen::{At, EdgeForm, Entity, Inspected, Measure, Request, Summary};
use varde_view::{
    Edit, Look, MeasureLook, MeasureSlot, ModelHighlight, Pick, Picked, measure_values,
};

use super::super::*;
use super::two_plates;
use crate::tests::{answer, two_sides};

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// A random pick of what `doc` shows, as the viewport would make it: a
/// face, an edge (rarely a crease, which the viewport never picks) or a
/// vertex, sometimes with one of its snap points, sometimes of an older
/// model.
fn random_pick(doc: &Doc, rng: &mut Rng) -> Option<Pick> {
    let index = doc.feed.pick_index();
    let mesh = index.mesh();
    let target = match rng.below(3) {
        0 if mesh.face_count() > 0 => Picked::Face(rng.below(mesh.face_count()) as u32),
        1 if mesh.edge_count() > 0 => {
            let edge = rng.below(mesh.edge_count()) as u32;
            if index.edge_faces(edge).is_none() && !rng.chance(10) {
                return None;
            }
            Picked::Edge(edge)
        }
        2 if !mesh.corners().is_empty() => Picked::Vertex(rng.below(mesh.corners().len()) as u32),
        _ => return None,
    };
    let body = index.body(target)?;
    let at = match target {
        Picked::Face(face) => {
            let indices = mesh.face_indices(face as usize)?;
            let vertex = mesh.indices()[indices.start + rng.below(indices.len())];
            glam::Vec3::from(mesh.positions()[vertex as usize]).as_dvec3()
        }
        Picked::Edge(edge) => index.chain_point(edge)?,
        Picked::Vertex(corner) => glam::Vec3::from(mesh.corners()[corner as usize]).as_dvec3(),
    };
    let snaps = index.snaps(target);
    let snap = (rng.chance(40) && !snaps.is_empty()).then(|| snaps[rng.below(snaps.len())].0);
    let model = if rng.chance(5) {
        index.model().wrapping_sub(1)
    } else {
        index.model()
    };
    Some(Pick {
        model,
        target,
        body,
        at,
        snap,
    })
}

/// Answers some of the requests waiting: all in order, only the newest
/// (the rest stay, to come late), one at random, or all newest first.
fn deliver(doc: &mut Doc, requests: &RefCell<Vec<Request>>, rng: &mut Rng) {
    let waiting = requests.take();
    if waiting.is_empty() {
        return;
    }
    let mut waiting = waiting;
    match rng.below(4) {
        0 => {
            for request in waiting {
                doc.computed(varde_regen::handle(request));
            }
        }
        1 => {
            let newest = waiting.pop().unwrap();
            doc.computed(varde_regen::handle(newest));
            requests.borrow_mut().splice(0..0, waiting);
        }
        2 => {
            let one = waiting.remove(rng.below(waiting.len()));
            doc.computed(varde_regen::handle(one));
            requests.borrow_mut().splice(0..0, waiting);
        }
        _ => {
            for request in waiting.into_iter().rev() {
                doc.computed(varde_regen::handle(request));
            }
        }
    }
}

/// What the kernel measures of `pick` on `editor`'s document as it is,
/// resolved as a reference is, in the regeneration lane's terms.
fn direct(
    editor: &Editor,
    evaluation: &varde_regen::Evaluation,
    pick: &varde_regen::InspectPick,
) -> Option<Resolved> {
    let holder = evaluation.holder(pick.body)?;
    let made = evaluation.bodies.iter().find(|made| made.body == holder)?;
    let solid = made.solid.clone();
    let topology = solid.topology();
    let near = DVec3::from(pick.near);
    let found = match pick.entity {
        Entity::Body => KernelPick::Body,
        Entity::Face(key) => KernelPick::Face(topology.face(&solid, &key, near).ok()?),
        Entity::Edge(keys) => KernelPick::Edge(topology.edge(&solid, keys, near).ok()?),
        Entity::EdgePoint(keys) => KernelPick::EdgePoint(topology.edge(&solid, keys, near).ok()?),
        Entity::Corner(keys) => KernelPick::Corner(topology.corner(&solid, keys, near).ok()?),
    };
    let _ = editor;
    Some((found, solid, topology))
}

/// A pick resolved on its body's solid by the kernel.
type Resolved = (KernelPick, std::sync::Arc<varde_kernel::Solid>, Topology);

/// `measured` as the lane answers it.
fn answered(measured: &Measured) -> Measure {
    let a = DVec3::to_array;
    match measured {
        Measured::Body(body) => Measure::Body {
            volume: body.volume,
            area: body.area,
            centre: body.centre.as_ref().map(a),
            bounds: body.bounds.map(|b| [b.min.to_array(), b.max.to_array()]),
        },
        Measured::Face(face) => Measure::Face {
            area: face.area,
            summary: Summary::of(&face.form),
            half_angle: face.half_angle(),
        },
        Measured::Edge(edge) => Measure::Edge {
            length: edge.length,
            closed: edge.closed,
            shape: match edge.shape {
                EdgeShape::Line { from, to } => EdgeForm::Line {
                    from: a(&from),
                    to: a(&to),
                },
                EdgeShape::Circle {
                    centre,
                    axis,
                    radius,
                } => EdgeForm::Circle {
                    centre: a(&centre),
                    axis: a(&axis),
                    radius,
                },
                EdgeShape::Ellipse {
                    centre,
                    axis,
                    major,
                    minor,
                } => EdgeForm::Ellipse {
                    centre: a(&centre),
                    axis: a(&axis),
                    major,
                    minor,
                },
                EdgeShape::Other => EdgeForm::Other,
            },
        },
        Measured::Point(p) => Measure::Point(p.to_array()),
    }
}

/// Checks what's shown while measuring against `inspected`, the newest
/// answer, and against the kernel: each pick's values and what's between
/// them, as the panel shows and copies them, are those of a direct
/// measure of the same solid; the highlight is of where the answer found
/// the picks, of the model shown.
fn check_measured(doc: &Doc, inspected: &Inspected) -> usize {
    let mut compared = 0;
    let session = doc.measure.as_ref().unwrap();
    let editor = &doc.editor;
    let document = editor.document();
    let units = document.units();
    let tol = document.tolerance();
    let evaluation = varde_regen::evaluate(document, &mut varde_regen::Cache::default());
    let probed = [Some(&inspected.first), inspected.second.as_ref()];
    let mut targets = [Vec::new(), Vec::new()];
    let mut resolved = [None, None];
    let index = doc.feed.pick_index();
    for slot in 0..2 {
        let (Some(pick), Some(outcome)) = (session.picks[slot], probed[slot]) else {
            assert!(probed[slot].is_none() || session.picks[slot].is_some());
            continue;
        };
        let Ok(found) = outcome else {
            continue;
        };
        let Some((kernel, solid, topology)) = direct(editor, &evaluation, &pick) else {
            panic!("the lane found {pick:?}, the kernel doesn't");
        };
        let target = Target {
            solid: &solid,
            topology: &topology,
            pick: kernel,
        };
        let direct = measure::measure(&target, &tol, &Budget::DEFAULT);
        match (&found.measure, &direct) {
            (Ok(answer), Ok(direct)) => {
                let direct = answered(direct);
                assert_eq!(*answer, direct, "{pick:?}");
                compared += 1;
                assert_eq!(
                    measure_values(answer, units),
                    measure_values(&direct, units)
                );
            }
            (Err(_), Err(_)) => {}
            (answer, direct) => panic!("{answer:?} but the kernel says {direct:?}"),
        }
        let point = matches!(found.measure, Ok(Measure::Point(_)));
        match (found.at, pick.entity) {
            (Some(At::Face(face)), _) => targets[slot].push(Picked::Face(face)),
            (Some(At::Edge(edge)), _) if !point => targets[slot].push(Picked::Edge(edge)),
            (None, Entity::Body) => {
                let holder = evaluation.holder(pick.body).unwrap();
                targets[slot].extend(index.body_faces(holder).map(Picked::Face));
            }
            _ => {}
        }
        resolved[slot] = Some((kernel, solid, topology));
    }
    if let (Some(between), [Some(a), Some(b)]) = (&inspected.between, &resolved) {
        fn target(resolved: &Resolved) -> Target<'_> {
            Target {
                solid: &resolved.1,
                topology: &resolved.2,
                pick: resolved.0,
            }
        }
        let direct = measure::distance(&target(a), &target(b), &tol, &Budget::DEFAULT);
        match (&between.distance, direct) {
            (Ok(gap), Ok(direct)) => {
                assert_eq!(gap.distance, direct.distance);
                compared += 1;
                assert_eq!(gap.points, direct.points.map(|p| p.to_array()));
                let shown = varde_view::between_values(between, units);
                let distance = shown
                    .iter()
                    .find(|value| value.label == "Distance")
                    .unwrap();
                assert_eq!(
                    distance.copied,
                    varde_expr::full(direct.distance, Some(units.into()))
                );
            }
            (Err(_), Err(_)) => {}
            (gap, direct) => panic!("{gap:?} but the kernel says {direct:?}"),
        }
    }
    // The highlight: what the answer found, of the model shown.
    let hover = doc.pick.hover().map(|pick| pick.target);
    let hovered: Vec<Picked> = (hover.iter())
        .filter(|target| !targets[0].contains(target) && !targets[1].contains(target))
        .copied()
        .collect();
    let expected = index.highlight_with(&hovered, &targets[0], &targets[1]);
    let shown = doc.highlight().cloned();
    if expected.is_empty() {
        assert_eq!(shown, None);
    } else {
        assert_eq!(shown.as_deref(), Some(&expected));
    }
    compared
}

/// The highlight's faces and edges are all of `doc`'s model shown.
fn assert_of_the_model(doc: &Doc, highlight: &ModelHighlight) {
    let mesh = doc.feed.pick_index().mesh();
    let faces = (highlight.hovered_faces.iter())
        .chain(&highlight.selected_faces)
        .chain(&highlight.second_faces);
    assert!(faces.into_iter().all(|&f| (f as usize) < mesh.face_count()));
    let h = &highlight.highlights;
    let edges = (h.outlined.iter())
        .chain(&h.selected_edges)
        .chain(&h.second_edges);
    assert!(edges.into_iter().all(|&e| (e as usize) < mesh.edge_count()));
}

/// Draws `doc` in a random window size and mode: never a panic; at a
/// size that shows the whole panel, a single pick's values all show as
/// the panel has them.
fn shows_its_values(doc: &Doc, rng: &mut Rng) {
    let mode = if rng.chance(50) {
        varde_view::Mode::Dark
    } else {
        varde_view::Mode::Light
    };
    let large = rng.chance(50);
    let size = if large {
        iced::Size::new(1280.0, 900.0)
    } else {
        iced::Size::new(
            200.0 + rng.below(1400) as f32,
            150.0 + rng.below(900) as f32,
        )
    };
    let mut renderer = varde_view::probe::renderer();
    let view = doc.view(
        false,
        mode,
        varde_view::ViewOptions::default(),
        crate::Offers::default(),
    );
    let mut ui = crate::tests::shown(view, size, &mut renderer);
    let found: Vec<String> = (crate::tests::texts(&mut ui, &renderer).into_iter())
        .map(|text| text.text)
        .collect();
    let Some(session) = &doc.measure else {
        return;
    };
    if !large || session.picks[1].is_some() {
        return;
    }
    let Some(inspected) = doc.feed.inspected() else {
        return;
    };
    if let Ok(probed) = &inspected.first
        && let Ok(measure) = &probed.measure
    {
        for value in measure_values(measure, doc.editor.document().units()) {
            assert!(found.contains(&value.shown), "{value:?} in {found:?}");
        }
    }
}

/// The newest measure revision asked so far, from what was sent.
fn newest_revision(sent: &[Request]) -> Option<u64> {
    sent.iter().filter_map(Request::inspect).max()
}

#[test]
fn random_measuring_keeps_its_invariants() {
    let seeds = std::env::var("VARDE_MEASURE_FUZZ")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2u64);
    let first = std::env::var("VARDE_MEASURE_SEED")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1u64);
    let compared: usize = (first..first + seeds).map(|seed| run(seed, 160)).sum();
    // Measures and distances compared with the kernel's: not vacuous.
    assert!(compared >= 20 * seeds as usize, "{compared}");
}

fn run(seed: u64, steps: usize) -> usize {
    let mut compared = 0;
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ seed.wrapping_mul(0x1000_0001));
    let (mut doc, requests, below) = two_plates();
    // Every request sent, in the order sent.
    let mut sent: Vec<Request> = Vec::new();
    let mut joined = false;
    for step in 0..steps {
        let generation = doc.editor.generation();
        let selection = doc.pick.selection.clone();
        let measuring = doc.measure.is_some();
        let queued = requests.borrow().len();
        let mut escaped = false;
        let mut edits = false;
        let action = rng.below(16);
        match action {
            0 => {
                doc.look(Look::StartMeasure);
            }
            1 => {
                let measuring = doc.measure.is_some();
                let selection = doc.pick.selection.clone();
                doc.look(Look::Escape);
                if measuring {
                    // Esc leaves no trace: the tool gone, the selection
                    // as it was, the model asked for without a measure.
                    assert!(doc.measure.is_none(), "seed {seed} step {step}");
                    assert_eq!(doc.pick.selection, selection, "seed {seed} step {step}");
                    escaped = true;
                }
            }
            2..=5 => {
                let pick = random_pick(&doc, &mut rng);
                let (add, double) = (rng.chance(25), rng.chance(10));
                doc.look(Look::ClickModel { pick, add, double });
            }
            6 => doc.look(Look::ClickModel {
                pick: None,
                add: false,
                double: false,
            }),
            7 => {
                let bodies = doc.editor.document().bodies();
                let Some(body) = bodies.get(rng.below(bodies.len())).map(|body| body.id) else {
                    continue;
                };
                doc.look(Look::ClickBody {
                    body,
                    add: rng.chance(30),
                });
            }
            8 => {
                let pick = random_pick(&doc, &mut rng);
                doc.look(Look::Hover(pick));
            }
            9 => {
                edits = true;
                doc.update(crate::tests::an_edit(&doc));
            }
            10 => {
                edits = true;
                let bodies = doc.editor.document().bodies();
                let Some(body) = bodies.get(rng.below(bodies.len())) else {
                    continue;
                };
                doc.apply(Command::SetVisible(body.id, !body.visible));
                doc.sync();
            }
            11 => {
                edits = true;
                let bodies = doc.editor.document().bodies();
                let Some(body) = bodies.get(rng.below(bodies.len())).map(|body| body.id) else {
                    continue;
                };
                let opacity = Opacity::new(10 + rng.below(91) as u8).unwrap();
                doc.apply(Command::SetOpacity(body, opacity));
                doc.sync();
            }
            12 => {
                edits = true;
                if !joined || rng.chance(30) {
                    // A boss through both plates, joining them: the
                    // lower one merged into the upper.
                    let extent = two_sides(doc.editor.document(), "15", "5");
                    let join = Operation::Join(Targets::default());
                    crate::tests::add_disc(&mut doc.editor, (20.0, 0.0), extent, join);
                    doc.sync();
                    joined = true;
                }
            }
            13 => {
                edits = true;
                doc.update(if rng.chance(60) {
                    Edit::Undo
                } else {
                    Edit::Redo
                });
            }
            14 => {
                let slot = if rng.chance(50) {
                    MeasureSlot::A
                } else {
                    MeasureSlot::B
                };
                doc.look(Look::Measure(MeasureLook::Fold(slot)));
            }
            _ => shows_its_values(&doc, &mut rng),
        }
        if std::env::var_os("VARDE_FUZZ_LOG").is_some() {
            let items = doc.pick.selection.items().count();
            let measuring = doc.measure.is_some();
            let waiting: Vec<_> = (requests.borrow().iter())
                .map(|r| (r.generation(), r.inspect()))
                .collect();
            let picks = doc
                .measure
                .as_ref()
                .map(|m| m.picks.map(|p| p.map(|p| p.entity)));
            eprintln!(
                "{seed}/{step}: action {action}, measuring {measuring}, selected {items}, \
                 editor {:?}, shown {:?}, waiting {waiting:?}, picks {picks:?}",
                doc.editor.generation(),
                doc.feed.generation()
            );
        }
        // Picks while measuring leave the selection as it was.
        if measuring && doc.measure.is_some() && !edits {
            assert_eq!(doc.pick.selection, selection, "seed {seed} step {step}");
        }
        if !edits {
            assert_eq!(
                doc.editor.generation(),
                generation,
                "seed {seed} step {step}: measuring wrote to the document"
            );
        }
        // What this step asked: the newest request has no measure once
        // the tool is left (asked anew, or already so).
        let new: Vec<Request> = requests.borrow()[queued..].to_vec();
        sent.extend(new);
        if escaped {
            let last = sent.last();
            assert!(
                last.is_none_or(|last| last.inspect().is_none()),
                "seed {seed} step {step}"
            );
        }
        if rng.chance(60) {
            deliver(&mut doc, &requests, &mut rng);
        }
        // Only the newest answer is shown: its revision is the newest
        // asked.
        if let Some(inspected) = doc.feed.inspected() {
            assert!(doc.measure.is_some(), "seed {seed} step {step}");
            assert_eq!(Some(inspected.revision), newest_revision(&sent));
        }
        if let Some(highlight) = doc.highlight() {
            assert_of_the_model(&doc, highlight);
        }
        if doc.measure.is_some()
            && let Some(inspected) = doc.feed.inspected()
            && doc.feed.generation() == Some(doc.editor.generation())
        {
            let inspected = inspected.clone();
            compared += check_measured(&doc, &inspected);
        }
        let _ = below;
    }
    // Everything answered, the panel and its copies hold.
    answer(&mut doc, &requests);
    if doc.measure.is_some()
        && let Some(inspected) = doc.feed.inspected()
    {
        let inspected = inspected.clone();
        compared += check_measured(&doc, &inspected);
    }
    compared
}
