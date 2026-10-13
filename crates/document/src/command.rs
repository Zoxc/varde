//! The edits to a [`Document`]: [`Command`], and [`Document::after`], the
//! document each makes; `planning`, `sketch_face` and `params` hold the
//! body planning, sketch-face upkeep and parameter edits they share.

mod params;
mod planning;
mod sketch_face;

use std::fmt;

use varde_expr::LengthUnit;
use varde_kernel_compat::Tolerance;
use varde_sketch::{Budget, Sketch};

use crate::{
    Body, BodyId, CheckError, Document, FeatureId, FeatureKind, LinkSource, Named, Opacity, Param,
    Plane, Removable, Removal, Tint,
};

/// An edit to a [`Document`]. [`Editor::apply`](crate::Editor::apply) refuses one that would
/// leave the document failing [`Document::check`].
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Removes a body with the feature that makes it, and so with all
    /// [`Document::removal`] lists for it.
    RemoveBody(BodyId),
    SetVisible(BodyId, bool),
    SetOpacity(BodyId, Opacity),
    /// Gives a body a colour of its own, or with `None` the theme's back.
    SetColor(BodyId, Option<Tint>),
    /// Adds a feature holding an empty sketch on `plane`, an origin plane
    /// or a face of a body ([`Plane::Face`]).
    AddSketch {
        name: String,
        plane: Plane,
    },
    /// Replaces a sketch feature's sketch whole: how every edit inside a
    /// sketch is committed, as one undoable change. The sources of links
    /// it no longer has go with them; a link it has that the feature has
    /// no source for is refused (a new one comes with
    /// [`Command::AddLink`]).
    SetSketch {
        feature: FeatureId,
        sketch: Box<Sketch>,
    },
    /// Replaces a sketch feature's sketch whole, as
    /// [`Command::SetSketch`] does, giving the link `source.link` of the
    /// new sketch, one the feature has no source for, what it comes
    /// from: how a link the Project or Intersect tool adds is committed.
    AddLink {
        feature: FeatureId,
        sketch: Box<Sketch>,
        source: LinkSource,
    },
    /// Puts a sketch feature on another plane, an origin plane or a face,
    /// keeping its drawing as it is in its own coordinates. Not a sketch,
    /// or the plane it's on already, changes nothing.
    SetSketchPlane {
        feature: FeatureId,
        plane: Plane,
    },
    /// Adds a feature of any kind but a sketch's ([`Command::AddSketch`]
    /// adds those), hiding the sketches whose regions it takes
    /// ([`FeatureKind::profile_sketches`]: a loft's sections' sketches). One making a new body adds the body,
    /// "Body N" one past the bodies so named, and gives it its id whatever
    /// [`Operation::NewBody`] held ([`BodyId::NEW`]); a split keeping
    /// both sides makes one too, whatever its `new_body` held, and one
    /// keeping a side makes none ([`Split::new_body`]); nothing added
    /// holds an id ([`FeatureKind::held_body`]). A pattern whose
    /// copies are bodies of their own ([`Copies::Separate`](crate::Copies::Separate)) adds them,
    /// one per copy, named so in turn, whatever its list held. A
    /// revolve's axis must be a line of its sketch
    /// ([`Revolve::check_axis`]), a split's line's curves curves of its
    /// sketch ([`Split::check_curves`]), a sweep's path's curves curves
    /// of theirs ([`Sweep::check_curves`]), and a loft's start points,
    /// points and rail curves those of theirs ([`Loft::check_names`]).
    ///
    /// [`Loft::check_names`]: crate::Loft::check_names
    /// [`Revolve::check_axis`]: crate::Revolve::check_axis
    /// [`Operation::NewBody`]: crate::Operation::NewBody
    /// [`Split::new_body`]: crate::Split::new_body
    /// [`Split::check_curves`]: crate::Split::check_curves
    /// [`Sweep::check_curves`]: crate::Sweep::check_curves
    AddFeature {
        name: String,
        kind: Box<FeatureKind>,
    },
    /// Replaces a feature's kind, as edited, keeping its id, name and
    /// visibility. Neither may be a sketch's: [`Command::SetSketch`] sets
    /// those. The caller passes the regions as
    /// references made again from the sketch as it is now
    /// (`varde_sketch::Profiles::reference`), not the old ones. One that
    /// made a new body and still does keeps the body; one that stops
    /// making it removes it, dropping it from the other features'
    /// excluded lists (a later feature naming it, as a combine's target
    /// or tool, keeps naming it, and fails), holding its id ([`FeatureKind::held_body`]); one
    /// that starts making one gets back the body whose id it held, else
    /// adds one as [`Command::AddFeature`] does. The id is filled in
    /// whatever the command held. A split's new body goes the same
    /// way, as its `keep` has both sides or one. A pattern's copy bodies go the same
    /// way: each copy (by its original and its `k`) the feature made a
    /// body of keeps it, the others get new ones, and those it no longer
    /// makes (fewer copies, a body taken out, joined to the original
    /// again, another kind) are removed, likewise. A revolve's axis must be a line of its sketch, as for
    /// [`Command::AddFeature`].
    SetFeature {
        feature: FeatureId,
        kind: Box<FeatureKind>,
    },
    /// Removes a feature with every later feature that uses it, directly
    /// or through others, and the bodies they all make, dropping those
    /// bodies from the other features' excluded lists: what
    /// [`Document::removal`] lists.
    RemoveFeature(FeatureId),
    /// Removes the features, with all [`Document::removal_of`] lists for
    /// them, as one edit.
    RemoveFeatures(Vec<FeatureId>),
    /// Removes the features and the bodies they make, keeping the later
    /// features that use them, which then name what isn't there: what
    /// [`Document::breaking_removal`] lists.
    RemoveOnly(Vec<FeatureId>),
    SetFeatureVisible(FeatureId, bool),
    /// Rolls the Timeline back to before the feature, or forward to its
    /// end with `None`, see [`Document::rollback`]. A feature that isn't
    /// there changes nothing.
    SetRollback(Option<FeatureId>),
    /// Gives a feature or a body `name`, as it is: [`Document::rename`]
    /// makes the command with a name no other has. The name it has
    /// already, or one that isn't there, changes nothing.
    Rename {
        target: Named,
        name: String,
    },
    /// Changes the design's units. Every dimension's, feature value's and
    /// parameter's expression first has the old units written in after
    /// its bare numbers ([`Sketch::pin_units`],
    /// [`Value::pin_units`](varde_expr::Value::pin_units),
    /// [`Params::pin_units`](varde_expr::Params::pin_units)), so it means
    /// what it did, and no value or geometry changes. One the units would
    /// take too long is replaced by its value ([`varde_expr::exact`]); a
    /// parameter's in error, which has none, stays as it is.
    SetUnits(LengthUnit),
    /// Adds a parameter, `name = text`, after the others. Its name must
    /// be one [`varde_expr::check_name`] takes, no other parameter's; its
    /// text may be in error, as nothing uses it yet.
    AddParam {
        name: String,
        text: String,
    },
    /// Sets the expression of the parameter at `index`, without the
    /// whitespace around it, and evaluates every feature value and sketch
    /// dimension naming a parameter again, the geometry following: a
    /// sketch whose driving dimensions change is solved again here, within
    /// the solver's iteration bound ([`varde_sketch::revalue`]), in the
    /// same undo step. Refused, as [`EditError::Value`], where a value
    /// that uses it, directly or through other parameters, would be in
    /// error, and as [`EditError::Unsolved`] where a sketch wouldn't
    /// solve. One that isn't there changes nothing.
    SetParam {
        index: usize,
        text: String,
    },
    /// Renames the parameter at `index`, rewriting every use of it in
    /// feature values, sketch dimensions and other parameters
    /// ([`varde_expr::rename`]).
    /// Refused as [`EditError::Value`] where that takes an expression
    /// over [`varde_expr::MAX_LEN`]. One that isn't there changes nothing.
    RenameParam {
        index: usize,
        name: String,
    },
    /// Removes the parameter at `index`: refused, as
    /// [`EditError::ParamUsed`], while a feature value, a sketch
    /// dimension or another parameter uses it. One that isn't there changes nothing.
    RemoveParam(usize),
    /// Changes the design's tolerance, see [`Document::tolerance`].
    SetTolerance(Tolerance),
    /// Replaces the whole document, e.g. with unsaved changes recovered
    /// after a crash.
    Replace(Box<Document>),
}

impl Document {
    /// The command adding a new sketch on `plane`, named one past the
    /// highest "Sketch N" in the document, so that the numbering holds
    /// across sessions and undo.
    pub fn add_sketch(&self, plane: Plane) -> Command {
        Command::AddSketch {
            name: format!("Sketch {}", self.next_number("Sketch")),
            plane,
        }
    }

    /// The command adding a feature of `kind`, named one past the highest
    /// of its kind's names in the document ("Extrude N", "Revolve N"),
    /// like [`Document::add_sketch`].
    pub fn add_feature(&self, kind: FeatureKind) -> Command {
        Command::AddFeature {
            name: self.feature_name(&kind),
            kind: Box::new(kind),
        }
    }

    /// The name [`Document::add_feature`] gives a feature of `kind`.
    pub fn feature_name(&self, kind: &FeatureKind) -> String {
        let noun = kind.noun();
        format!("{noun} {}", self.next_number(noun))
    }

    /// A copy with body `id` as `change` leaves it, or `None` if there's
    /// no such body or `change` leaves it as it was.
    fn with_body(&self, id: BodyId, change: impl FnOnce(&mut Body)) -> Option<Document> {
        let index = self.body_index(id)?;
        let mut body = self.bodies[index].clone();
        change(&mut body);
        (body != self.bodies[index]).then(|| {
            let mut next = Document::clone(self);
            next.bodies[index] = body;
            next
        })
    }
}

impl Document {
    /// The document `command` makes of this one, or `None` if it changes
    /// nothing; sketches solved within `budget`. Each edit works on a
    /// copy, so one that would change nothing returns before copying.
    pub(crate) fn after(
        &self,
        command: Command,
        budget: &Budget,
    ) -> Result<Option<Document>, EditError> {
        let next = match command {
            Command::RemoveBody(id) => {
                return Ok(self.with_removal(self.removal(Removable::Body(id))));
            }
            Command::SetVisible(id, visible) => {
                return Ok(self.with_body(id, |body| body.visible = visible));
            }
            Command::SetOpacity(id, opacity) => {
                return Ok(self.with_body(id, |body| body.opacity = opacity));
            }
            Command::SetColor(id, color) => {
                return Ok(self.with_body(id, |body| body.color = color));
            }
            Command::AddSketch { name, plane } => self.with_new_sketch(name, plane)?,
            Command::SetSketch { feature, sketch } => {
                let Some(index) = self.feature_index(feature) else {
                    return Ok(None);
                };
                match &self.features[index].kind {
                    FeatureKind::Sketch { sketch: old, .. } if *old != *sketch => {}
                    _ => return Ok(None),
                }
                let mut next = self.clone();
                next.set_sketch(index, *sketch, None);
                next.keeps_sketch_face(self, index)?;
                next
            }
            Command::AddLink {
                feature,
                sketch,
                source,
            } => {
                let Some(index) = self.feature_index(feature) else {
                    return Ok(None);
                };
                if !matches!(self.features[index].kind, FeatureKind::Sketch { .. }) {
                    return Ok(None);
                }
                let mut next = self.clone();
                next.set_sketch(index, *sketch, Some(source));
                next.keeps_sketch_face(self, index)?;
                next
            }
            Command::SetSketchPlane { feature, plane } => {
                let Some(index) = self.feature_index(feature) else {
                    return Ok(None);
                };
                match &self.features[index].kind {
                    FeatureKind::Sketch { plane: old, .. } if *old != plane => {}
                    _ => return Ok(None),
                }
                let mut next = self.clone();
                let was = next.sketch_face_of(index);
                if let FeatureKind::Sketch { plane: old, .. } = &mut next.features[index].kind {
                    *old = plane;
                }
                next.follow_sketch_face(index, was)?;
                next
            }
            Command::AddFeature { name, kind } => self.with_new_feature(name, kind)?,
            Command::SetFeature { feature, kind } => return self.with_kind(feature, kind),
            Command::RemoveFeature(id) => {
                return Ok(self.with_removal(self.removal(Removable::Feature(id))));
            }
            Command::RemoveFeatures(ids) => {
                let targets: Vec<_> = ids.into_iter().map(Removable::Feature).collect();
                return Ok(self.with_removal(self.removal_of(&targets)));
            }
            Command::RemoveOnly(ids) => {
                let targets: Vec<_> = ids.into_iter().map(Removable::Feature).collect();
                return Ok(self.with_removal(self.breaking_removal(&targets)));
            }
            Command::SetFeatureVisible(id, visible) => {
                let Some(index) = self
                    .feature_index(id)
                    .filter(|&index| self.features[index].visible != visible)
                else {
                    return Ok(None);
                };
                let mut next = self.clone();
                next.features[index].visible = visible;
                next
            }
            Command::SetRollback(rollback) => {
                if self.rollback == rollback
                    || rollback.is_some_and(|id| self.feature(id).is_none())
                {
                    return Ok(None);
                }
                let mut next = self.clone();
                next.rollback = rollback;
                next
            }
            Command::Rename { target, name } => {
                if self.name_of(target).is_none_or(|old| old == name) {
                    return Ok(None);
                }
                let mut next = self.clone();
                if let Some(slot) = next.name_mut(target) {
                    *slot = name;
                }
                next
            }
            Command::SetUnits(units) => return Ok(self.with_units(units)),
            Command::AddParam { name, text } => {
                let mut params = self.params.clone();
                params.push(Param {
                    name,
                    text: text.trim().to_owned(),
                });
                return self.with_params(params, None, budget);
            }
            Command::SetParam { index, text } => {
                let mut params = self.params.clone();
                let Some(param) = params.get_mut(index) else {
                    return Ok(None);
                };
                param.text = text.trim().to_owned();
                return self.with_params(params, None, budget);
            }
            Command::RenameParam { index, name } => {
                let Some(old) = self.params.get(index).map(|param| param.name.as_str()) else {
                    return Ok(None);
                };
                if old == name {
                    return Ok(None);
                }
                let mut params = self.params.clone();
                for (at, param) in params.iter_mut().enumerate() {
                    param.text = varde_expr::rename(&param.text, old, &name)
                        .map_err(|why| EditError::Value(ValueOf::Param(at), why))?;
                }
                params[index].name = name.clone();
                return self.with_params(params, Some((old, &name)), budget);
            }
            Command::RemoveParam(index) => {
                let Some(param) = self.params.get(index) else {
                    return Ok(None);
                };
                if self.param_used(&param.name) {
                    return Err(EditError::ParamUsed(param.name.clone()));
                }
                let mut params = self.params.clone();
                params.remove(index);
                return self.with_params(params, None, budget);
            }
            Command::SetTolerance(tolerance) => {
                if tolerance == self.tolerance() {
                    return Ok(None);
                }
                let mut next = self.clone();
                next.tolerance = tolerance.fit();
                next
            }
            Command::Replace(replacement) => {
                if *replacement == *self {
                    return Ok(None);
                }
                *replacement
            }
        };
        Ok(Some(next))
    }

    /// A copy with `removal` removed, or `None` if it removes nothing.
    fn with_removal(&self, removal: Removal) -> Option<Document> {
        if removal.is_empty() {
            return None;
        }
        let mut next = self.clone();
        next.remove(&removal);
        Some(next)
    }
}

/// Why an edit was refused. The document is left as it was.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EditError {
    /// Every id for a body or a feature has been used.
    #[error("the document has no ids left")]
    OutOfIds,
    /// The edit would leave the document failing [`Document::check`], for
    /// the reason given.
    #[error("{0}")]
    Invalid(#[source] CheckError),
    /// An edit of the sketch of this feature can't be made, see
    /// [`SketchEdit::apply`](varde_sketch::SketchEdit::apply).
    #[error("feature {}: {}", .0.0, .1)]
    Sketch(FeatureId, #[source] varde_sketch::EditError),
    /// [`Command::AddFeature`] or [`Command::SetFeature`] was given a
    /// sketch, or asked to set one: sketches are added by
    /// [`Command::AddSketch`] and set by [`Command::SetSketch`].
    #[error("sketches are added and set by their own commands")]
    SketchKind,
    /// [`Command::SetSketch`] or [`Command::AddLink`] would take away the
    /// sketch face (`sketch_face`) of this sketch feature, which stays
    /// while the sketch is on its face.
    #[error("the sketch face can't be removed")]
    SketchFace(FeatureId),
    /// A parameter command would leave this value in error: a feature's
    /// that uses the parameter (directly or through others) or, renaming
    /// one, a parameter's text the new name takes over
    /// [`MAX_LEN`](varde_expr::MAX_LEN).
    #[error(fmt = value_message)]
    Value(ValueOf, #[source] varde_expr::Error),
    /// [`Command::RemoveParam`] was asked to remove this parameter, which
    /// a feature value or another parameter uses.
    #[error("the parameter '{0}' is in use")]
    ParamUsed(String),
    /// A parameter command changes dimensions of this sketch feature's
    /// sketch, which then doesn't solve, see
    /// [`varde_sketch::revalue`].
    #[error("feature {}: the sketch wouldn't solve: {}", .0.0, .1)]
    Unsolved(FeatureId, #[source] varde_sketch::Rejected),
}

/// [`EditError::Value`]: whose value, and why.
fn value_message(of: &ValueOf, why: &varde_expr::Error, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match of {
        ValueOf::Feature(id) => write!(f, "feature {}: a value would be in error: {why}", id.0),
        ValueOf::Param(index) => write!(f, "parameter {}: {why}", index + 1),
    }
}

/// Whose value an [`EditError::Value`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueOf {
    /// One of this feature's values.
    Feature(FeatureId),
    /// The text of the parameter at this index.
    Param(usize),
}
