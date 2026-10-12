//! Evaluating the feature history into the bodies' solids.
//!
//! Features run in the document's order. A sketch gives its profiles
//! ([`Sketch::profiles`]) and where it is: an origin plane's placement,
//! or, for a sketch on a face, the placement of the face's plane
//! ([`Placement::on_plane`]) on the body as the features before the
//! sketch leave it (see [`place_on_face`]); one that isn't placed fails,
//! and so does every extrude or revolve made from it. An extrude or a
//! revolve finds its regions again in its sketch's profiles
//! ([`Profiles::resolve`]; one that's gone is "region not found"), merges
//! them ([`Profiles::merge`]) and turns the loops into a kernel profile
//! ([`profile`]). An extrude sweeps it
//! with [`varde_kernel_compat::extrude`] on the sketch's plane, over
//! [`Extrude::span`](varde_document::Extrude::span) (a tapered one with
//! [`varde_kernel_compat::extrude_tapered`], a stand-in for now failing as too
//! complex: see `taper`); a revolve finds its axis in the sketch
//! ([`axis_line`](revolve::axis_line); a line that's gone is "axis not found") or, about a
//! model edge, on the edge's body as the features before it leave it
//! ([`edge_axis`](revolve::edge_axis): straight and in the sketch's plane), moves the
//! profile into the axis's frame ([`axis_frame`](revolve::axis_frame)) and turns it with
//! [`varde_kernel_compat::revolve`] over [`Revolve::span`](varde_document::Revolve::span). Both run within the
//! document's tolerance and the default budget, their faces named by
//! their feature id: the tool solid. A new body gets it. A join, cut or
//! intersect finds the bodies made before it,
//! less those it excludes, that the tool touches
//! ([`varde_kernel_compat::touches`]). A cut or intersect replaces each of those
//! by its difference from or intersection with the tool
//! ([`varde_kernel_compat::boolean`], the body first), one at a time. A join
//! touching one body replaces it by its union with the tool; one touching
//! several merges them: the first made gets the union of them all and the
//! tool, and the others are *consumed*, left out of
//! [`Evaluation::bodies`] and listed in [`Evaluation::merged`] (see
//! `Run::merge` for the order). A through-all extent's
//! span is worked out from the bodies made before it ([`through_all`]).
//! A combine works on the bodies as the features before it leave it: the
//! target's solid united with, less or intersected with each tool's, the
//! tools then consumed into the target as a join's merged bodies are,
//! unless it keeps them (see `combine::evaluate`); one naming a body with
//! no solid of its own (consumed before, or its maker failed) fails.
//! A move or a mirror works on the bodies the same way: one
//! [`varde_kernel_compat::Motion`] each, a move's turn about its axis then its
//! shift, a mirror's reflection in its plane (axes and planes found on
//! their bodies as the features before leave them), each body moved by
//! [`Solid::transformed`] keeping its faces' names, a mirror keeping the
//! original assembled with its image (see `motion`); a body the motion
//! would take past the coordinate limit fails it. A pattern works on its
//! bodies the same way, its axis found as a move's: each body becomes
//! itself and its copies, each placed directly by its own motion and
//! named as that copy of the pattern, assembled into the body (side by
//! side where apart, united where they meet; see `pattern`); a count
//! whose copies would be more patches than a solid may have, or a copy
//! past the coordinate limit, fails it before anything is copied. An
//! align moves its body as a move does, by the motion taking the point
//! and directions picked on it onto those picked on the target, found
//! as the features before it leave their bodies (see `align`). A scale
//! scales its bodies as a move moves them, about its point (found as an
//! align's) by its factors, or by the factor that gives its edge (found
//! and measured on its body) its typed length; a factor out of range, or
//! a body the scale would take past the coordinate limit, fails it
//! before anything is scaled (see `scale`). A split cuts its body in
//! two by a tool (see `split`). A chamfer finds its edges on its body's
//! topology (one not found: "its edge wasn't found"), grows them along
//! tangent chains if asked, and cuts them off by the kernel's chamfer,
//! the body keeping its id (see `chamfer`). A shell finds the faces it
//! opens on its body's topology (one not found: "its open face wasn't
//! found") and hollows the body by the kernel's shell, the body keeping
//! its id (see `shell`). A fillet finds and grows its edges as a
//! chamfer does and rounds them off by the kernel's fillet (see
//! `fillet`; what the two share of their edges is `blend`). An offset
//! face finds its faces on its body's
//! topology (one not found: "its face wasn't found") and moves them by
//! the kernel's offset face, the body keeping its id and its faces their
//! names (see `offset_face`). A draft finds its faces and its neutral
//! plane (one not found: "its neutral face wasn't found") and turns the
//! faces by the kernel's draft, the body keeping its id and its faces
//! their names (see `face_draft`). What chamfers, shells, fillets,
//! offset faces and drafts share of their body, changed in place, is
//! `in_place`. A sweep makes its tool as an extrude does, but
//! by the kernel's sweep along its path (sketch chains placed by their
//! sketches, model edges found on their bodies, joined end to end from
//! the profile's plane, or a helix about an axis found as a move's; see
//! `sweep`), then goes on as an extrude does. A loft makes its tool as an extrude does, but by the kernel's loft through
//! its sections in order (each a region of one loop or a point, placed
//! by its sketch), following its rails (sketch chains placed likewise;
//! see `loft`), then goes on as an extrude does.
//! A join, cut, intersect or combine that would leave nothing of a body fails
//! (bodies are the document's, so an emptied one would stay listed with
//! no geometry): no body in an [`Evaluation`] is empty.
//! A feature that fails records why, in words for the Timeline
//! (`src/why.rs`), and, where the kernel failed, what its failure's
//! evidence shows of where ([`FeatureFailure::geometry`], see
//! `src/error_geometry.rs`; the operand faces are resolved once the model
//! is drawn, each operand on the bodies it holds: a merge's or a
//! combine's running solid is the first body or target and those united
//! with it so far, a feature's tool none), and changes no body; the later
//! ones still run. Of regen's own failures, a face that isn't flat (a
//! sketch's, a mirror's or a draft's neutral face) or isn't round (a move's axis) shows the face,
//! an axis line of no length its point, and an axis edge of the wrong
//! shape its curves, as an align's references of the wrong kind do (and
//! its secondary parallel to its primary, both of them); the others (a
//! face or body gone, a face too far out, a sketch not placed or not
//! there, an axis not found, a combine's or a move's body with no solid,
//! a body moved out of range, a scale's point or edge not found or its
//! edge's length or direction refused) have nothing to show.
//!
//! Every result goes through the [`Cache`], keyed by what it depends on,
//! so only what an edit changes runs again.
//!
//! [`Sketch::profiles`]: varde_sketch::Sketch::profiles

use std::sync::Arc;

use glam::DVec3;
use varde_document::{
    BodyId, Document, FaceRef, Feature, FeatureId, FeatureKind, Placement, Plane, Section, Sketch,
};
use varde_kernel_compat::mesh::Form;
use varde_kernel_compat::{Budget, Frame, Profile, Solid, Tolerance};
use varde_sketch::{Profiles, RegionRef, TooComplex};

use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::{ErrorGeometry, FeatureFailure, KernelFailure};
use crate::picking::region_form;
use crate::profile::profile;
use crate::why;

/// The kernel operation `$name` gives: `$kernel`, or the stand-in a
/// test set in the [`crate::testing`] overrides' `$field`.
macro_rules! kernel_op {
    ($(#[$doc:meta])* $vis:vis fn $name:ident() -> $ty:ty = $kernel:path, $field:ident;) => {
        $(#[$doc])*
        $vis fn $name() -> $ty {
            #[cfg(any(test, feature = "testing"))]
            if let Some(op) = crate::testing::overrides().$field {
                return op;
            }
            $kernel
        }
    };
}

mod align;
mod blend;
mod chamfer;
mod combine;
mod evidence;
mod face_draft;
mod fillet;
mod in_place;
mod link;
mod loft;
mod motion;
mod offset_face;
mod pattern;
mod revolve;
mod run;
pub(crate) mod scale;
mod shell;
mod split;
mod sweep;
mod taper;

pub(crate) use evidence::{add_chain, add_region, chain_curves, corners, face_geometry};
pub(crate) use revolve::chain_runs_with;
pub(crate) use run::through_all;
use run::{Run, Shape, boolean_key, cached_boolean};

/// The kernel operations tests may run stand-ins in place of
/// ([`crate::testing`]): each one's type and its stand-in.
#[cfg(any(test, feature = "testing"))]
pub(crate) mod stand_ins {
    pub(crate) use super::chamfer::{Chamferer, stand_in::by_wedges as chamfer};
    pub(crate) use super::face_draft::{Drafter, stand_in::by_boxes as draft};
    pub(crate) use super::fillet::{Filleter, stand_in::by_arcs as fillet};
    pub(crate) use super::loft::{Lofter, stand_in::by_extrude as loft};
    pub(crate) use super::offset_face::{Offsetter, stand_in::by_boxes as offset};
    pub(crate) use super::shell::{Sheller, stand_in::by_boxes as shell};
    pub(crate) use super::split::{Splitter, stand_in::by_booleans as split};
    pub(crate) use super::sweep::{Sweeper, stand_in::by_extrude as sweep};
    pub(crate) use super::taper::{Taperer, stand_in::by_frustum as taper};
}

/// What the history gives: the solids of the bodies, and the features
/// that failed.
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    /// Each body that has a solid, in the order the features made them.
    /// None is empty: a feature that would empty one fails. A body a
    /// join merged into another, or a combine used as a tool without
    /// keeping it, isn't here (see [`Evaluation::merged`]).
    pub bodies: Vec<BodySolid>,
    /// Each body a join merged into another or a combine consumed as a
    /// tool into its target (*consumed*), and the body in
    /// [`Evaluation::bodies`] that now holds it, in the document's order
    /// of the consumed bodies. A body merged into one that a later join
    /// or combine consumed in turn names the later one, so every entry names a body
    /// in `bodies`, and no consumed body is in `bodies`.
    pub merged: Vec<(BodyId, BodyId)>,
    /// The features that failed, why and where, in the document's
    /// order.
    pub failed: Vec<FeatureFailure>,
    /// Each join, cut or intersect that got as far as its tool solid,
    /// with the bodies made before it and not taken out of it that the
    /// tool touches, in the order they were made; in the document's
    /// order.
    /// One failing while finding them lists those found before and the
    /// body it couldn't tell, which taking out gets past.
    pub touched: Vec<(FeatureId, Vec<BodyId>)>,
    /// Each cut that worked, with the bodies it cut that it takes
    /// nothing from (it only touches them, face to face, say), in the
    /// order they were made; in the document's order. Told by their
    /// volumes and the tool's intersection with them holding nothing.
    pub uncut: Vec<(FeatureId, Vec<BodyId>)>,
    /// Each sketch on a face that was placed, and where, in the
    /// document's order: on the plane of its face on the body as the
    /// features before it leave it, by [`Placement::on_plane`]. A
    /// sketch on an origin plane isn't listed: its placement is the
    /// plane's. A sketch on a face that isn't listed failed, and is
    /// drawn nowhere.
    pub placements: Vec<(FeatureId, Placement)>,
    /// Each move turning about an axis, each mirror and each pattern,
    /// whose axis or plane was found, and where: a point on it and its
    /// direction (a mirror's normal), not unit, as the feature turned,
    /// mirrored or placed its copies by (a linear pattern only along
    /// the direction), in the document's order. For the app to draw a draft's axis or
    /// plane where regenerating found it.
    pub references: Vec<(FeatureId, [DVec3; 2])>,
    /// Each align that found the references of either side, and what it
    /// found, in the document's order. For the app to draw a draft's
    /// points and directions where regenerating found them.
    pub aligned: Vec<(FeatureId, crate::AlignDatums)>,
    /// Each scale that found its point or its edge, and what it found,
    /// in the document's order. For the app to show a draft's edge's
    /// length and fitted faces, and draw its point.
    pub scaled: Vec<(FeatureId, crate::ScaleFound)>,
    /// Each sweep whose path was built, and where its handles stand,
    /// in the document's order. For the app to put a draft's knobs.
    pub swept: Vec<(FeatureId, crate::SweepFound)>,
    /// Each split that kept both pieces: the body split, which kept one,
    /// and its new body, which got the other, in the document's order.
    /// A sketch on a face of the body that went to the new body follows
    /// it there, and one on a face of the new body that went to the body
    /// (with the pieces swapped) follows it back.
    pub splits: Vec<(BodyId, BodyId)>,
    /// Each sketch whose links found other geometry than they hold, as
    /// it is with what they found, solved (see `link`), in the
    /// document's order. For the app to fold into the change that moved
    /// their sources.
    pub relinked: Vec<(FeatureId, Arc<Sketch>)>,
    /// Each link that found nothing, with why: its sketch, its id, in
    /// the document's order. It keeps what it holds.
    pub broken: Vec<(FeatureId, varde_sketch::Id, String)>,
}

impl Evaluation {
    /// The body in [`Evaluation::bodies`] holding `body`'s solid: `body`
    /// itself, or the body it was merged into. `None` for a body with no
    /// solid (its maker failed, or it isn't the document's). Whatever
    /// lives on a consumed body is looked for here: a sketch on one of
    /// its faces is placed on the holder's solid, where the face lives
    /// on.
    pub fn holder(&self, body: BodyId) -> Option<BodyId> {
        let body = merged_into(&self.merged, body).unwrap_or(body);
        self.bodies
            .iter()
            .any(|made| made.body == body)
            .then_some(body)
    }

    /// `find` on the solid holding `body` ([`Evaluation::holder`]),
    /// `None` if none does. Where `missing` says what it looks for isn't
    /// there, it's followed through the splits: into the bodies splits
    /// made of it, the first split first, and back into the bodies
    /// splits made it of (a split that swapped which piece keeps the id
    /// moves it there), and on from those; the first answer not missing,
    /// else the holder's.
    pub(crate) fn following_splits<T>(
        &self,
        body: BodyId,
        mut find: impl FnMut(&BodySolid) -> T,
        missing: impl Fn(&T) -> bool,
    ) -> Option<T> {
        let holder = self.holder(body)?;
        let made = self.solid(holder)?;
        let first = find(made);
        if !missing(&first) {
            return Some(first);
        }
        let mut on = vec![made.body];
        let mut at = 0;
        while let Some(&body) = on.get(at) {
            at += 1;
            for &(split, new) in &self.splits {
                let (Some(split), Some(new)) = (self.holder(split), self.holder(new)) else {
                    continue;
                };
                let next = if split == body {
                    new
                } else if new == body {
                    split
                } else {
                    continue;
                };
                if on.contains(&next) {
                    continue;
                }
                on.push(next);
                let Some(made) = self.solid(next) else {
                    continue;
                };
                let followed = find(made);
                if !missing(&followed) {
                    return Some(followed);
                }
            }
        }
        Some(first)
    }

    /// `body`'s own solid, if it has one (not the one holding it).
    pub(crate) fn solid(&self, body: BodyId) -> Option<&BodySolid> {
        self.bodies.iter().find(|made| made.body == body)
    }

    /// The solid holding `body`'s ([`Evaluation::holder`]), if any.
    pub(crate) fn holding(&self, body: BodyId) -> Option<&BodySolid> {
        self.solid(self.holder(body)?)
    }

    /// Puts `change` in place of its body's solid, if that has one.
    pub(crate) fn replace(&mut self, change: BodySolid) {
        if let Some(made) = self.bodies.iter_mut().find(|made| made.body == change.body) {
            *made = change;
        }
    }
}

/// The body `body` was merged into by `merged`, a list as
/// [`Evaluation::merged`] keeps it (each consumed body with its holder),
/// if it was.
pub fn merged_into(merged: &[(BodyId, BodyId)], body: BodyId) -> Option<BodyId> {
    (merged.iter())
        .find(|(consumed, _)| *consumed == body)
        .map(|&(_, holder)| holder)
}

/// Notes in `merged`, a list like [`Evaluation::merged`], that a join
/// merged `bodies` (in the order they were made) into the first, the
/// *holder*: each of the others is consumed into it, and so are the bodies
/// merged into those before. Nothing for fewer than two. How regen keeps
/// `merged`, and how the bodies the joins touched replay it.
pub fn note_merge(merged: &mut Vec<(BodyId, BodyId)>, bodies: &[BodyId]) {
    let Some((&holder, consumed)) = bodies.split_first() else {
        return;
    };
    for (_, held_in) in merged.iter_mut() {
        if consumed.contains(held_in) {
            *held_in = holder;
        }
    }
    merged.extend(consumed.iter().map(|&body| (body, holder)));
}

/// Whether each of `bodies` has a solid of its own in `evaluation` (the
/// bodies the features before a combine, move or mirror made), or why
/// the feature naming them fails: one a join or a combine consumed
/// fails it, naming the body holding it (the user meant that body as it
/// was, not the one it went into), and so does one whose maker failed,
/// or one that isn't there (its maker deleted, the feature kept).
pub(crate) fn own_solids(
    document: &Document,
    mut bodies: impl Iterator<Item = BodyId>,
    evaluation: &Evaluation,
) -> Result<(), Failed> {
    let name = |body: BodyId| body_name(document, body);
    let Some(body) = bodies.find(|&body| evaluation.solid(body).is_none()) else {
        return Ok(());
    };
    if document.body(body).is_none() {
        return Err(why::BODY_GONE.into());
    }
    let consumed = (evaluation.merged.iter()).find(|(consumed, _)| *consumed == body);
    Err(match consumed {
        Some(&(_, holder)) => why::consumed(name(body), name(holder)),
        None => why::no_solid(name(body)),
    }
    .into())
}

/// [`own_solids`] for one body, giving its solid.
pub(crate) fn own_solid<'e>(
    document: &Document,
    body: BodyId,
    evaluation: &'e Evaluation,
) -> Result<&'e BodySolid, Failed> {
    own_solids(document, std::iter::once(body), evaluation)?;
    Ok(evaluation.solid(body).expect("own_solids found it"))
}

/// Why a feature fails, as the history carries it: in words, and what
/// to draw of where, made from the kernel's failure where it's the
/// kernel's ([`FeatureFailure`] without the feature).
#[derive(Debug, Clone)]
pub(crate) struct Failed {
    pub(crate) message: String,
    pub(crate) geometry: Option<Arc<ErrorGeometry>>,
    /// Whether what was looked for isn't on the body, which a split may
    /// have given to its new body: told by this, never by the words.
    pub(crate) missing: bool,
}

impl Failed {
    /// The kernel's `failure`, worded as `message`, of the bodies its
    /// operands are made of (`[a, b]`, see [`KernelFailure::geometry`]).
    pub(crate) fn kernel(
        message: String,
        failure: &KernelFailure,
        operands: [&[BodyId]; 2],
    ) -> Failed {
        Failed {
            missing: false,
            message,
            geometry: failure.geometry(operands),
        }
    }

    /// What was looked for isn't on the body, in `message`'s words, see
    /// [`Failed::missing`].
    pub(crate) fn missing(message: &str) -> Failed {
        Failed {
            missing: true,
            ..Failed::from(message)
        }
    }

    /// The failure of `feature`.
    pub(crate) fn of(self, feature: FeatureId) -> FeatureFailure {
        FeatureFailure {
            feature,
            message: self.message,
            geometry: self.geometry,
        }
    }
}

impl From<String> for Failed {
    fn from(message: String) -> Failed {
        Failed {
            missing: false,
            message,
            geometry: None,
        }
    }
}

impl From<&str> for Failed {
    fn from(message: &str) -> Failed {
        Failed::from(message.to_owned())
    }
}

/// A body's solid.
#[derive(Debug, Clone)]
pub struct BodySolid {
    pub body: BodyId,
    pub solid: Arc<Solid>,
    /// What the solid was filed under, for drawing it.
    pub(crate) key: Key,
}

/// A sketch evaluated: the sketch, its profiles, where it is (`None` for
/// one that isn't placed: on a face that wasn't found, isn't flat or
/// whose body is gone), and its key, which is of the sketch alone.
struct SketchOutput<'a> {
    id: FeatureId,
    sketch: &'a Sketch,
    profiles: Arc<Result<Profiles, TooComplex>>,
    placement: Option<Placement>,
    key: Key,
}

impl SketchOutput<'_> {
    /// The kernel profile of `regions` of the sketch, merged, in its
    /// coordinates, fitted to `fit`; or why there's none, in words.
    fn profile(&self, regions: &[RegionRef], fit: f64) -> Result<Profile, String> {
        let profiles = match &*self.profiles {
            Ok(profiles) => profiles,
            Err(e) => return Err(format!("its sketch is {e}")),
        };
        let found = profiles
            .resolve(regions)
            .into_iter()
            .collect::<Option<Vec<usize>>>()
            .ok_or("region not found")?;
        let loops = profiles.merge(&found).map_err(|e| e.to_string())?;
        profile(self.sketch, profiles, &loops, fit).map_err(|e| e.to_string())
    }
}

/// The kernel's frame of a sketch's `placement`.
fn frame(placement: &Placement) -> Frame {
    Frame {
        origin: placement.origin,
        x: placement.x,
        y: placement.y,
    }
}

/// Evaluates the history of `document`, see the module's docs.
pub fn evaluate(document: &Document, cache: &mut Cache) -> Evaluation {
    evaluate_within(document, cache, Budget::DEFAULT)
}

/// [`evaluate`], with `touching` the budget of each
/// [`varde_kernel_compat::touches`], which tests make small to see it fail.
pub(crate) fn evaluate_within(
    document: &Document,
    cache: &mut Cache,
    touching: Budget,
) -> Evaluation {
    walk(document, cache, touching, &mut |_, _| {})
}

/// [`evaluate`], telling `starting` of each feature, and its index in the
/// document's order, as it starts on it: how far it has got.
pub(crate) fn evaluate_reporting(
    document: &Document,
    cache: &mut Cache,
    starting: &mut dyn FnMut(usize, &Feature),
) -> Evaluation {
    walk(document, cache, Budget::DEFAULT, starting)
}

/// [`evaluate_within`], telling `starting` of each feature as it starts
/// on it.
fn walk(
    document: &Document,
    cache: &mut Cache,
    touching: Budget,
    starting: &mut dyn FnMut(usize, &Feature),
) -> Evaluation {
    let tolerance = document.tolerance();
    let mut sketches: Vec<SketchOutput> = Vec::new();
    let mut evaluation = Evaluation::default();
    for (index, feature) in document.features().iter().enumerate() {
        starting(index, feature);
        let result = match &feature.kind {
            FeatureKind::Sketch {
                plane,
                sketch,
                sources,
            } => {
                // Its profiles are 2D: they don't depend on where it is.
                let key = Keyer::new("sketch").value(sketch).finish();
                let profiles = cache.profiles(key, || sketch.profiles());
                let placement = match plane {
                    Plane::Origin(origin) => Some(origin.placement()),
                    Plane::Face(face) => {
                        match place_on_face(face, &evaluation, &tolerance, cache) {
                            Ok(placement) => {
                                evaluation.placements.push((feature.id, placement));
                                Some(placement)
                            }
                            Err(error) => {
                                (evaluation.failed).push(error.of(feature.id));
                                None
                            }
                        }
                    }
                };
                if !sources.is_empty() {
                    let found = link::find(
                        document,
                        sketch,
                        sources,
                        placement,
                        &sketches,
                        &evaluation,
                        tolerance,
                        cache,
                    );
                    link::relink(
                        feature.id,
                        sketch,
                        key,
                        found,
                        document,
                        &mut evaluation,
                        cache,
                    );
                }
                sketches.push(SketchOutput {
                    id: feature.id,
                    sketch,
                    profiles,
                    placement,
                    key,
                });
                Ok(())
            }
            FeatureKind::Extrude(_)
            | FeatureKind::Revolve(_)
            | FeatureKind::Sweep(_)
            | FeatureKind::Loft(_) => {
                let (sketch, shape, operation) = match &feature.kind {
                    FeatureKind::Extrude(extrude) => {
                        (extrude.sketch, Shape::Extrude(extrude), &extrude.operation)
                    }
                    FeatureKind::Revolve(revolve) => {
                        (revolve.sketch, Shape::Revolve(revolve), &revolve.operation)
                    }
                    FeatureKind::Sweep(sweep) => {
                        (sweep.sketch, Shape::Sweep(sweep), &sweep.operation)
                    }
                    // Its first section's sketch stands as its own (a
                    // checked loft has sections): the others are found
                    // among `sketches`.
                    FeatureKind::Loft(loft) => (
                        loft.sections.first().map_or(feature.id, Section::sketch),
                        Shape::Loft(loft),
                        &loft.operation,
                    ),
                    FeatureKind::Sketch { .. }
                    | FeatureKind::Combine(_)
                    | FeatureKind::Move(_)
                    | FeatureKind::Mirror(_)
                    | FeatureKind::Pattern(_)
                    | FeatureKind::Align(_)
                    | FeatureKind::Scale(_)
                    | FeatureKind::Split(_)
                    | FeatureKind::Chamfer(_)
                    | FeatureKind::Shell(_)
                    | FeatureKind::Fillet(_)
                    | FeatureKind::OffsetFace(_)
                    | FeatureKind::FaceDraft(_) => unreachable!("matched apart"),
                };
                // A checked document's extrude, revolve, sweep or loft names a
                // sketch before it.
                match sketches.iter().find(|s| s.id == sketch) {
                    None => Err(Failed::from("its sketch isn't there")),
                    Some(sketch) => Run {
                        document,
                        feature,
                        shape,
                        operation,
                        sketch,
                        sketches: &sketches,
                        tolerance,
                        touching,
                    }
                    .evaluate(&mut evaluation, cache),
                }
            }
            FeatureKind::Combine(combine) => {
                combine::evaluate(document, combine, &tolerance, &mut evaluation, cache)
            }
            FeatureKind::Move(moved) => motion::evaluate_move(
                document,
                feature.id,
                moved,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::Mirror(mirror) => motion::evaluate_mirror(
                document,
                feature.id,
                mirror,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::Pattern(pattern) => pattern::evaluate_pattern(
                document,
                feature.id,
                pattern,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::Align(align) => align::evaluate_align(
                document,
                feature.id,
                align,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::Scale(scale) => scale::evaluate_scale(
                document,
                feature.id,
                scale,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::Chamfer(chamfer) => chamfer::evaluate_chamfer(
                document,
                feature.id,
                chamfer,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::Shell(shell) => shell::evaluate_shell(
                document,
                feature.id,
                shell,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::Fillet(fillet) => fillet::evaluate_fillet(
                document,
                feature.id,
                fillet,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::OffsetFace(offset) => offset_face::evaluate_offset_face(
                document,
                feature.id,
                offset,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::FaceDraft(draft) => face_draft::evaluate_face_draft(
                document,
                feature.id,
                draft,
                &tolerance,
                &mut evaluation,
                cache,
            ),
            FeatureKind::Split(split) => split::evaluate_split(
                document,
                feature.id,
                split,
                &sketches,
                &tolerance,
                &mut evaluation,
                cache,
            ),
        };
        // A sketch records its own failures, and answers `Ok`.
        if let Err(failed) = result {
            evaluation.failed.push(failed.of(feature.id));
        }
    }
    // In the document's order (bodies are kept in increasing id order).
    evaluation.merged.sort_by_key(|&(consumed, _)| consumed);
    // A new body's tool is never empty, a union of two solids that
    // aren't isn't, and a cut or intersect that would empty one fails.
    debug_assert!(
        evaluation.bodies.iter().all(|made| !made.solid.is_empty()),
        "a body is never empty"
    );
    debug_assert!(
        evaluation.merged.iter().all(|(consumed, into)| {
            let held = |body: &BodyId| evaluation.bodies.iter().any(|made| made.body == *body);
            !held(consumed) && held(into)
        }),
        "a consumed body has no solid and names one that has"
    );
    evaluation
}

/// Where a sketch on `face` is, given the bodies the features before it
/// made (`evaluation`), or why it isn't anywhere. The face's body is
/// looked for through [`Evaluation::holder`] (a body a join consumed
/// lives on in the one holding it; one with no solid is "gone"); the
/// face is found on its solid's [`Topology`](varde_kernel_compat::Topology) by
/// name or alias, the nearest to its point among several
/// ([`Topology::face`](varde_kernel_compat::Topology::face); none is "wasn't
/// found", unless a split before the sketch gave the face to its new
/// body: the face is then looked for on the bodies splits made of the
/// holder or made it of, in [`Evaluation::splits`]' order, and on those
/// made of them or made them of, and the sketch follows it to the first
/// it's found on); its form must be a plane ("isn't flat", which shows the face
/// found, see [`face_geometry`]), whose `n` and `d`
/// give the placement by [`Placement::on_plane`], the same rule and the
/// same bits as the app's from the picking tables' summary of the face
/// (both take the region's form by [`region_form`]). A placement that
/// isn't [`Placement::valid`] (its origin past the coordinate limit) is
/// refused. Cached by the solid's key, the face's name and point and the
/// fit tolerance (which the face is drawn at), so the topology is worked
/// out again only when the solid changes.
pub(crate) fn place_on_face(
    face: &FaceRef,
    evaluation: &Evaluation,
    tolerance: &Tolerance,
    cache: &mut Cache,
) -> Result<Placement, Failed> {
    evaluation
        .following_splits(
            face.body,
            |made| place_on(made, face, tolerance, cache),
            not_found,
        )
        .unwrap_or(Err(why::FACE_BODY_GONE.into()))
}

/// Whether [`place_on`] found no face of the name on its body, which a
/// split before the sketch may have given to its new body.
fn not_found(placed: &Result<Placement, Failed>) -> bool {
    placed.as_ref().is_err_and(|failed| failed.missing)
}

/// Where a sketch on `face` is on the body `made` holds, or why it isn't
/// anywhere, as [`place_on_face`] finds it.
fn place_on(
    made: &BodySolid,
    face: &FaceRef,
    tolerance: &Tolerance,
    cache: &mut Cache,
) -> Result<Placement, Failed> {
    let key = Keyer::new("placement")
        .key(made.key)
        .value(&face.key)
        .point(face.near)
        .number(tolerance.fit().to_bits())
        .finish();
    cache.placement(key, || {
        let solid = &made.solid;
        let topology = solid.topology();
        let region = (topology.face(solid, &face.key, face.near))
            .map_err(|_| Failed::missing(why::FACE_NOT_FOUND))?;
        let region = &topology.regions()[region as usize];
        let not_flat = || Failed {
            missing: false,
            message: why::FACE_NOT_FLAT.to_owned(),
            geometry: face_geometry(solid, region, tolerance),
        };
        let Form::Plane { n, d } = *region_form(solid, region) else {
            return Err(not_flat());
        };
        let placement = Placement::on_plane(n, d).ok_or_else(not_flat)?;
        if placement.valid() {
            Ok(placement)
        } else {
            Err(why::FACE_TOO_FAR.into())
        }
    })
}

/// The name of `body` in `document`, as the Timeline's messages give it
/// ([`why::SOME_BODY`] for one that isn't there).
pub(crate) fn body_name(document: &Document, body: BodyId) -> &str {
    document
        .body(body)
        .map_or(why::SOME_BODY, |body| body.name.as_str())
}

pub(crate) use link::Broken;

#[cfg(test)]
pub(crate) mod tests;
