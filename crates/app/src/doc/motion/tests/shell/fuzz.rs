//! Random sequences of what the user can do around the shell session,
//! regeneration shelling boxes ([`varde_regen::testing`]): starting and
//! editing shells, faces clicked (on the model shown or one gone by,
//! edges and other bodies' faces among them, the faces a preview made
//! too, quick clicks before a model comes) to pick them or take them
//! out, bodies' rows clicked, rows' crosses, the Remove field toggled,
//! the direction, thicknesses typed (out of range, overflowing, not
//! numbers among them), units changed, rows hovered (past the last
//! among them), undo and redo, joins and combines merging bodies before
//! the shell (and undone), bodies added and features removed, models
//! answered at any point, commits, Add anyway and cancels, edits taking
//! every face out then a merge.
//!
//! After each step: a session that's ready is whole, passes its own
//! check and the document's for its faces, its faces on one body that
//! the feature can name and that is the session's, and is previewed as
//! set up; one ready whose preview didn't fail commits, and the
//! document holds what it drafted; the faces lit are on the body
//! drawing the faces' body; a shell edited opens to what it stores and
//! OK on it straight away writes nothing; a session never outlives the
//! shell it edits; the panel's texts never panic; and the document
//! passes its check. Each shell the model shows working was of a box
//! and gives the volume its box, open sides, thickness and direction
//! give.

use varde_document::{BodyOp, Combine, Targets};
use varde_expr::LengthUnit;

use super::super::chamfer::fuzz::Rng;
use super::*;
use crate::tests::{add_disc, two_sides};

const THICKNESSES: &[&str] = &[
    "1",
    "2",
    "0.5",
    "3 mm",
    "0.1 in",
    "4.9999999",
    "5",
    "9.9999",
    "10",
    "15",
    "0",
    "-1",
    "1e308",
    "1e400",
    "1/0",
    "99999999999",
    "1e-9",
    "abc",
    "",
    "45°",
];

/// The shells of `plates`' document.
fn shells(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Shell(_)))
        .map(|feature| feature.id)
        .collect()
}

/// A random click on the model shown: mostly a face (of the session's
/// body, while it has one), now and then an edge; mostly on the model
/// shown, now and then on one gone by.
pub(in crate::doc::motion::tests) fn random_pick(plates: &Plates, rng: &mut Rng) -> Option<Pick> {
    let index = plates.doc.feed.pick_index();
    let body = plates
        .doc
        .motion
        .as_ref()
        .and_then(|session| session.bodies.first().copied());
    let choosy = rng.below(3) != 0;
    let model = if rng.below(10) == 0 {
        index.model().wrapping_sub(1)
    } else {
        index.model()
    };
    let target = if rng.below(8) == 0 {
        let count = index.mesh().edge_count();
        if count == 0 {
            return None;
        }
        Picked::Edge(rng.below(count) as u32)
    } else {
        let faces: Vec<u32> = (0..index.picking().faces().len() as u32)
            .filter(|&face| !choosy || body.is_none() || index.body(Picked::Face(face)) == body)
            .collect();
        if faces.is_empty() {
            return None;
        }
        Picked::Face(*rng.pick(&faces))
    };
    let snaps = index.snaps(target);
    let at = if snaps.is_empty() {
        DVec3::ZERO
    } else {
        rng.pick(&snaps).1
    };
    Some(Pick {
        model,
        target,
        body: index.body(target)?,
        at,
        snap: None,
    })
}

/// Holds the session, if it's a shell's, to what the module's docs say.
fn check_session(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    if session.kind != MotionKind::Shell {
        return;
    }
    let document = plates.doc.editor.document();
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::Shell(_))), "{what}");
    }
    let faces = &session.faces.refs;
    assert!(
        faces.windows(2).all(|pair| pair[0].order(&pair[1]).is_lt()),
        "{what}: {faces:?}"
    );
    assert!(
        faces.iter().all(|face| face.body == faces[0].body),
        "{what}: {faces:?}"
    );
    assert!(session.bodies.len() <= 1, "{what}: {:?}", session.bodies);
    if let Some(face) = faces.first() {
        assert_eq!(session.bodies, [face.body], "{what}");
    }
    check_lit(plates, what);
    if !plates.doc.motion_ready() {
        return;
    }
    let shell = session
        .shell()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    shell
        .check_own(&document.design())
        .unwrap_or_else(|why| panic!("{what}: {why}: {shell:?}"));
    let features = document.features();
    let index = (session.feature)
        .and_then(|id| features.iter().position(|feature| feature.id == id))
        .unwrap_or(features.len());
    document
        .check_face_set(index, shell.body, &shell.open)
        .unwrap_or_else(|why| panic!("{what}: {why}: {shell:?}"));
    assert!(
        crate::doc::combine::pickable(document, shell.body, session.feature),
        "{what}: {shell:?}"
    );
    assert_eq!(
        plates.doc.motion_draft(),
        Some((session.feature, FeatureKind::Shell(shell))),
        "{what}"
    );
}

/// Holds the faces lit on a model of the document as it is to the body
/// drawing the faces' body.
fn check_lit(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    let lit = plates.doc.faces_lit();
    let current = plates.doc.feed.generation() == Some(plates.doc.editor.generation());
    if lit.is_empty() || !current {
        return;
    }
    let body =
        (session.faces.body()).unwrap_or_else(|| panic!("{what}: lit {lit:?} with no faces"));
    let merged = plates.doc.feed.merged_bodies();
    let shown = (merged.iter())
        .find(|(consumed, _)| *consumed == body)
        .map_or(body, |&(_, holder)| holder);
    let index = plates.doc.feed.pick_index();
    for target in lit {
        assert_eq!(index.body(target), Some(shown), "{what}: lit {target:?}");
    }
}

/// Edits the shell `id`, holding the session to what it stores, and OK
/// on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::Shell(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let Some(session) = plates.doc.motion.as_ref() else {
        return;
    };
    assert_eq!(session.feature, Some(id), "{what}");
    assert_eq!(session.shell(), Some(stored), "{what}");
    let revision = plates.doc.editor.revision();
    let document = plates.doc.editor.document().clone();
    plates.doc.update(Edit::CommitMotion);
    if plates.doc.motion.is_none() {
        assert_eq!(plates.doc.editor.revision(), revision, "{what}: wrote");
        assert_eq!(*plates.doc.editor.document(), document, "{what}");
    }
}

/// A join or a combine merging bodies: a disc joined across the first
/// box, or one body combined into another (the session's body into
/// another, or another into it, among them).
fn merge(plates: &mut Plates, rng: &mut Rng) {
    if rng.below(3) == 0 {
        let extent = two_sides(plates.doc.editor.document(), "12", "1");
        let operation = Operation::Join(Targets::default());
        add_disc(&mut plates.doc.editor, (40.0, 15.0), extent, operation);
    } else {
        let document = plates.doc.editor.document();
        let all: Vec<BodyId> = document.bodies().iter().map(|body| body.id).collect();
        if all.len() < 2 {
            return;
        }
        let mut target = *rng.pick(&all);
        let mut tool = *rng.pick(&all);
        let session = plates.doc.motion.as_ref();
        if let Some(&body) = session.and_then(|session| session.bodies.first())
            && rng.below(2) == 0
        {
            if rng.below(2) == 0 {
                tool = body;
            } else {
                target = body;
            }
        }
        if target == tool {
            return;
        }
        let combine = Combine {
            target,
            tools: vec![tool],
            op: BodyOp::Union,
            keep_tools: rng.below(3) == 0,
        };
        let add = document.add_feature(combine.into());
        plates.doc.apply(add);
    }
    plates.doc.sync();
}

/// The volume `shell` of `solid` should give, if `solid` is a box along
/// the axes and the shell can work: from its box, the sides its faces
/// open (by the plane all of their triangles' corners lie on), the
/// thickness and the direction.
fn wanted_volume(solid: &varde_kernel::Solid, shell: &Shell) -> Option<f64> {
    let topology = solid.topology();
    let bounds = solid.bounds3()?;
    let size = bounds.max - bounds.min;
    let whole = size.x * size.y * size.z;
    if topology.regions().len() != 6 || (solid.volume() - whole).abs() > 1e-9 * whole {
        return None;
    }
    let mut opened = [[false; 2]; 3];
    for face in &shell.open {
        let region = topology.face(solid, &face.key, face.near).ok()?;
        let tri = topology.regions()[region as usize].tris[0] as usize;
        let p = solid.mesh().patch(tri).p;
        let side = (0..3)
            .flat_map(|axis| [(axis, 0), (axis, 1)])
            .find(|&(axis, end)| {
                let at = if end == 0 { bounds.min } else { bounds.max }[axis];
                p.iter().all(|corner| (corner[axis] - at).abs() < 1e-9)
            })?;
        opened[side.0][side.1] = true;
    }
    let t = shell.thickness.value;
    let closed = |axis: usize| f64::from(2 - opened[axis].iter().filter(|&&o| o).count() as u8);
    let volume = if shell.outward {
        (0..3).map(|a| size[a] + t * closed(a)).product::<f64>() - whole
    } else {
        let inner: Vec<f64> = (0..3).map(|a| size[a] - t * closed(a)).collect();
        if inner.iter().any(|&side| side <= 0.0) {
            return None;
        }
        whole - inner.iter().product::<f64>()
    };
    (volume > 0.0).then_some(volume)
}

/// Holds each shell the model shows working to the volume
/// [`wanted_volume`] gives.
fn check_volumes(plates: &Plates, what: &str) {
    let document = plates.doc.editor.document();
    let failed = plates.doc.feed.failed_features();
    for (at, feature) in document.features().iter().enumerate() {
        let FeatureKind::Shell(shell) = &feature.kind else {
            continue;
        };
        if failed.iter().any(|failed| failed.feature == feature.id) {
            continue;
        }
        let solid = |count: usize| {
            let mut cut = Editor::new(document.clone());
            while cut.document().features().len() > count {
                let last = cut.document().features().last().unwrap().id;
                cut.apply(Command::RemoveFeature(last)).unwrap();
            }
            let evaluation =
                varde_regen::evaluate(cut.document(), &mut varde_regen::Cache::default());
            (evaluation.bodies.iter())
                .find(|made| made.body == shell.body)
                .map(|made| made.solid.clone())
        };
        let (Some(before), Some(after)) = (solid(at), solid(at + 1)) else {
            panic!("{what}: {shell:?} works on no body");
        };
        let wanted = wanted_volume(&before, shell)
            .unwrap_or_else(|| panic!("{what}: {shell:?} worked, giving {}", after.volume()));
        let got = after.volume();
        assert!(
            (got - wanted).abs() <= 1e-6 * before.volume().max(wanted),
            "{what}: {got} vs {wanted}"
        );
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (seed + 1).wrapping_mul(0x5851_f42d));
    varde_regen::testing::shell_by_boxes();
    let mut plates = boxes(true);
    let base = plates.doc.editor.document().features().len();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let open = plates.doc.motion.is_some();
        let roll = match rng.below(4) {
            0 if !open => 0,
            _ => rng.below(46),
        };
        match roll {
            0 | 1 if open && rng.below(6) != 0 => plates.answer(),
            0 | 1 => plates.doc.look(Look::StartShell),
            2 => {
                let ids = shells(&plates);
                if !ids.is_empty() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                }
            }
            3..=11 => {
                if let Some(pick) = random_pick(&plates, &mut rng) {
                    click(&mut plates, pick);
                }
            }
            12 => {
                let faces = (plates.doc.motion.as_ref())
                    .map(|session| session.faces.refs.clone())
                    .unwrap_or_default();
                if !faces.is_empty() {
                    plates.motion(MotionLook::DropFace(*rng.pick(&faces)));
                }
            }
            13 => {
                let direction = *rng.pick(&[ShellDirection::Inward, ShellDirection::Outward]);
                plates.motion(MotionLook::ShellDirection(direction));
            }
            14 => {
                let bodies: Vec<BodyId> = (plates.doc.editor.document().bodies().iter())
                    .map(|body| body.id)
                    .collect();
                if !bodies.is_empty() {
                    plates.doc.look(Look::ClickBody {
                        body: *rng.pick(&bodies),
                        add: false,
                    });
                }
            }
            15 => plates.motion(MotionLook::Picking(MotionPick::Faces)),
            16..=18 => plates.input(MotionField::Thickness, rng.pick(THICKNESSES)),
            19 => {
                let units = *rng.pick(&LengthUnit::ALL);
                plates.doc.update(Edit::SetUnits(units));
            }
            20 => {
                let at = rng.below(4);
                if rng.below(2) == 0 {
                    plates
                        .doc
                        .look(Look::HoverPanel(Some(PanelHover::Face(at))));
                } else {
                    plates.doc.look(Look::LeavePanel(PanelHover::Face(at)));
                }
            }
            21 => {
                if let Some(pick) = random_pick(&plates, &mut rng) {
                    plates.doc.look(Look::Hover(Some(pick)));
                }
            }
            22 | 23 if rng.below(2) == 0 => {
                plates.doc.update(Edit::Undo);
                if plates.doc.editor.document().features().len() < base {
                    plates.doc.update(Edit::Redo);
                }
            }
            24 if rng.below(2) == 0 => plates.doc.update(Edit::Redo),
            25..=27 => {
                let before = plates.doc.motion.as_ref().and_then(|session| {
                    let ready =
                        plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
                    (ready && session.kind == MotionKind::Shell)
                        .then(|| (session.feature, session.shell()))
                });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(shell))) = before {
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?}",
                        plates.doc.edit_error
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
                    assert_eq!(*stored, FeatureKind::Shell(shell), "{what}");
                }
            }
            // Add anyway, past a failed preview.
            28 => plates.doc.update(Edit::AcceptError),
            29 if rng.below(3) == 0 => plates.motion(MotionLook::Cancel),
            30 | 31 => merge(&mut plates, &mut rng),
            32 if rng.below(2) == 0 => {
                if rng.below(2) == 0 {
                    let mut editor = plates.doc.editor.clone();
                    let x = 100.0 + 30.0 * rng.below(3) as f64;
                    add_box(&mut editor, DVec2::new(x, 0.0), DVec2::new(x + 20.0, 10.0));
                    let document = editor.document().clone();
                    let added =
                        document.features().len() - plates.doc.editor.document().features().len();
                    // The two features added: the sketch and its extrude.
                    for feature in &document.features()[document.features().len() - added..] {
                        let add = plates
                            .doc
                            .editor
                            .document()
                            .add_feature(feature.kind.clone());
                        plates.doc.apply(add);
                    }
                    plates.doc.sync();
                } else {
                    // A feature after the boxes removed, with what it made.
                    let features = plates.doc.editor.document().features();
                    if features.len() > base {
                        let id = features[base + rng.below(features.len() - base)].id;
                        plates.doc.apply(Command::RemoveFeature(id));
                        plates.doc.sync();
                    }
                }
            }
            33 => {
                let ids = shells(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            34 => {
                // An edit taking every face out, then its body merged.
                let ids = shells(&plates);
                if !ids.is_empty() && plates.doc.motion.is_none() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                    let faces = (plates.doc.motion.as_ref())
                        .map(|session| session.faces.refs.clone())
                        .unwrap_or_default();
                    for face in faces {
                        plates.motion(MotionLook::DropFace(face));
                    }
                    if rng.below(2) == 0 {
                        plates.answer();
                    }
                    merge(&mut plates, &mut rng);
                }
            }
            35 => {
                // A merge, undone at once.
                merge(&mut plates, &mut rng);
                if rng.below(2) == 0 {
                    plates.answer();
                }
                plates.doc.update(Edit::Undo);
            }
            _ => plates.answer(),
        }
        if rng.below(3) == 0 {
            plates.answer();
        }
        check_session(&plates, &what);
        let _ = screen_texts(&plates.doc);
        plates
            .doc
            .editor
            .document()
            .check()
            .unwrap_or_else(|error| panic!("{what}: {error}"));
    }
    plates.doc.motion = None;
    plates.doc.sync();
    plates.answer();
    check_volumes(&plates, &format!("seed {seed} at the end"));
}

/// See the module's docs. `VARDE_SHELL_SEEDS` runs more seeds
/// (`VARDE_SHELL_FROM` the first).
#[test]
fn random_shell_sessions_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_SHELL_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_SHELL_SEEDS", 3)) {
        run(seed, 200);
    }
}
