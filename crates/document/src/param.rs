//! The design's parameters: named values, such as `height = 40 mm`, that
//! features' values use by name (`height / 2`), see [`Param`].

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, MAX_LEN, MAX_PARAMS, NameError, Params, Value, check_name};

use crate::{
    ChamferSize, Design, Document, FeatureId, FeatureKind, Move, PathRef, PatternKind, ScaleFactor,
    Turn,
};

/// A parameter as the document holds it: its name and its expression as
/// typed. What it comes to is worked out from the list whole
/// ([`Document::params_resolved`]): it may use other parameters, in any
/// order, and a value using it is kept in step with it by
/// [`Command::SetParam`](crate::Command::SetParam).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    /// A word, see [`varde_expr::check_name`]: unique in the document.
    pub name: String,
    /// The expression, at most [`varde_expr::MAX_LEN`] bytes. It may be in
    /// error (a name nothing defines, say) while nothing uses it.
    pub text: String,
}

/// What's wrong with the design's parameters, see
/// [`CheckError::Param`](crate::CheckError::Param) and
/// [`CheckError::Params`](crate::CheckError::Params).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamError {
    /// Its name can't be one, see [`NameError`].
    Name(NameError),
    /// A parameter before it has its name.
    Duplicate,
    /// Its expression is this many bytes, over [`varde_expr::MAX_LEN`].
    TextLength(usize),
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamError::Name(why) => write!(f, "its name is wrong: {why}"),
            ParamError::Duplicate => f.write_str("a parameter before it has its name"),
            ParamError::TextLength(len) => write!(
                f,
                "its expression is {len} bytes, over the limit of {MAX_LEN}"
            ),
        }
    }
}

impl std::error::Error for ParamError {}

/// Checks `params` on their own: at most [`MAX_PARAMS`], each named as
/// [`check_name`] takes, no name twice, each text at most [`MAX_LEN`]
/// bytes. Errors in their expressions are no error of the document's,
/// only values that use them are.
pub(crate) fn check_params(params: &[Param]) -> Result<(), crate::CheckError> {
    use crate::CheckError;
    if params.len() > MAX_PARAMS {
        return Err(CheckError::Params(params.len()));
    }
    for (index, param) in params.iter().enumerate() {
        check_name(&param.name).map_err(|why| CheckError::Param(index, ParamError::Name(why)))?;
        if params[..index].iter().any(|p| p.name == param.name) {
            return Err(CheckError::Param(index, ParamError::Duplicate));
        }
        if param.text.len() > MAX_LEN {
            return Err(CheckError::Param(
                index,
                ParamError::TextLength(param.text.len()),
            ));
        }
    }
    Ok(())
}

/// `params` resolved in a design in `units`.
pub(crate) fn resolve(params: &[Param], units: varde_expr::LengthUnit) -> Arc<Params> {
    Arc::new(Params::evaluate(
        params.iter().map(|p| (p.name.as_str(), p.text.as_str())),
        units,
    ))
}

impl FeatureKind {
    /// Its typed values, other than a sketch's dimensions: what a
    /// parameter can be used in.
    pub fn values(&self) -> Vec<&Value> {
        match self {
            FeatureKind::Extrude(extrude) => {
                extrude.extent.values().chain(&extrude.taper).collect()
            }
            FeatureKind::Revolve(revolve) => revolve.extent.values().collect(),
            FeatureKind::Move(moved) => moved
                .offset
                .iter()
                .chain(moved.turn.as_ref().map(|(_, angle)| angle))
                .collect(),
            FeatureKind::Pattern(pattern) => match &pattern.kind {
                PatternKind::Linear { count, spacing, .. } => vec![count, spacing],
                PatternKind::Circular { count, angle, .. } => vec![count, angle],
            },
            FeatureKind::Align(align) => align.offset.iter().chain(&align.turn).collect(),
            FeatureKind::Scale(scale) => match &scale.factor {
                ScaleFactor::Uniform(value) => vec![value],
                ScaleFactor::PerAxis(values) => values.iter().collect(),
                ScaleFactor::EdgeLength { length, .. } => vec![length],
            },
            FeatureKind::Chamfer(chamfer) => match &chamfer.distances {
                ChamferSize::Equal(d) => vec![d],
                ChamferSize::Two(a, b) | ChamferSize::Angle(a, b) => vec![a, b],
            },
            FeatureKind::Shell(shell) => vec![&shell.thickness],
            FeatureKind::Fillet(fillet) => vec![&fillet.radius],
            FeatureKind::OffsetFace(offset) => vec![&offset.distance],
            FeatureKind::FaceDraft(draft) => vec![&draft.angle],
            FeatureKind::Sweep(sweep) => {
                let helix = match &sweep.path {
                    PathRef::Helix(helix) => vec![&helix.pitch, &helix.turns],
                    PathRef::Chain(_) => Vec::new(),
                };
                sweep.twist.iter().chain(helix).collect()
            }
            FeatureKind::Sketch { .. }
            | FeatureKind::Combine(_)
            | FeatureKind::Mirror(_)
            | FeatureKind::Split(_)
            | FeatureKind::Loft(_) => Vec::new(),
        }
    }
}

impl FeatureKind {
    /// Its typed values, as [`FeatureKind::values`], each with what it's
    /// checked against in `design`: for writing the units in, or
    /// evaluating them again as parameters change.
    pub(crate) fn values_mut<'p>(&mut self, design: &Design<'p>) -> Vec<(&mut Value, Ask<'p>)> {
        match self {
            FeatureKind::Extrude(extrude) => extrude.values_mut(design),
            FeatureKind::Revolve(revolve) => {
                let angle = Turn::ask(design);
                revolve.extent.values_mut().map(|v| (v, angle)).collect()
            }
            FeatureKind::Move(moved) => {
                let offset = Move::offset_ask(design);
                let (offsets, angle) = moved.values_mut();
                let angle = angle.map(|v| (v, Move::angle_ask(design)));
                offsets
                    .iter_mut()
                    .map(|v| (v, offset))
                    .chain(angle)
                    .collect()
            }
            FeatureKind::Pattern(pattern) => pattern.values_mut(design).into(),
            FeatureKind::Align(align) => {
                let offset = (align.offset.as_mut()).map(|v| (v, Move::offset_ask(design)));
                let turn = (align.turn.as_mut()).map(|v| (v, Move::angle_ask(design)));
                offset.into_iter().chain(turn).collect()
            }
            FeatureKind::Scale(scale) => scale.values_mut(design),
            FeatureKind::Chamfer(chamfer) => chamfer.values_mut(design),
            FeatureKind::Shell(shell) => shell.values_mut(design),
            FeatureKind::Fillet(fillet) => fillet.values_mut(design),
            FeatureKind::OffsetFace(offset) => offset.values_mut(design),
            FeatureKind::FaceDraft(draft) => draft.values_mut(design),
            FeatureKind::Sweep(sweep) => sweep.values_mut(design),
            FeatureKind::Sketch { .. }
            | FeatureKind::Combine(_)
            | FeatureKind::Mirror(_)
            | FeatureKind::Split(_)
            | FeatureKind::Loft(_) => Vec::new(),
        }
    }
}

/// What uses a parameter, see [`Document::all_param_uses`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParamUses {
    /// The features using it directly, in order, as
    /// [`Document::param_uses`] has them.
    pub features: Vec<FeatureId>,
    /// The other parameters using it, by index, as
    /// [`Document::param_users`] has them.
    pub params: Vec<usize>,
}

impl Document {
    /// What uses each parameter, by index as [`Document::params`] lists
    /// them: [`Document::param_uses`] and [`Document::param_users`] for
    /// all at once, each text's names found once.
    pub fn all_param_uses(&self) -> Vec<ParamUses> {
        let mut by_name: std::collections::BTreeMap<&str, Vec<usize>> = Default::default();
        for (index, param) in self.params.iter().enumerate() {
            by_name.entry(param.name.as_str()).or_default().push(index);
        }
        let mut uses = vec![ParamUses::default(); self.params.len()];
        // The parameters `text` names, each once.
        let named = |text: &str| {
            let mut named: Vec<usize> = (varde_expr::names(text).into_iter())
                .filter_map(|span| by_name.get(&text[span.range()]))
                .flatten()
                .copied()
                .collect();
            named.sort_unstable();
            named.dedup();
            named
        };
        for feature in &self.features {
            let dimensions = match &feature.kind {
                FeatureKind::Sketch { sketch, .. } => sketch.dimensions.as_slice(),
                _ => &[],
            };
            let values = feature.kind.values();
            let texts = (values.iter().map(|value| value.text.as_str())).chain(
                dimensions
                    .iter()
                    .map(|entry| entry.dimension.value.text.as_str()),
            );
            let mut named: Vec<usize> = texts.flat_map(&named).collect();
            named.sort_unstable();
            named.dedup();
            for index in named {
                uses[index].features.push(feature.id);
            }
        }
        for (user, param) in self.params.iter().enumerate() {
            for index in named(&param.text) {
                if self.params[index].name != param.name {
                    uses[index].params.push(user);
                }
            }
        }
        uses
    }

    /// The design's parameters, in the order the user made them.
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// The design's parameters resolved in its units: each one's value
    /// or what keeps it from one, by index as [`Document::params`] lists
    /// them. What [`Document::design`] gives values to read names with.
    pub fn params_resolved(&self) -> &Params {
        &self.resolved
    }

    /// The same, shared: for what reads values with the document's
    /// parameters while it changes, such as a feature's panel (see
    /// [`Design`]'s `params`).
    pub fn params_shared(&self) -> Arc<Params> {
        self.resolved.clone()
    }

    /// The features with a value (or, a sketch, a dimension) using the
    /// parameter `name`, in order,
    /// directly (a value using a parameter that uses `name` isn't
    /// counted, see [`Document::param_users`]).
    pub fn param_uses(&self, name: &str) -> Vec<FeatureId> {
        (self.features.iter())
            .filter(|feature| {
                let dimensions = match &feature.kind {
                    FeatureKind::Sketch { sketch, .. } => sketch.dimensions.as_slice(),
                    _ => &[],
                };
                (feature.kind.values().iter()).any(|value| varde_expr::uses(&value.text, name))
                    || (dimensions.iter())
                        .any(|entry| varde_expr::uses(&entry.dimension.value.text, name))
            })
            .map(|feature| feature.id)
            .collect()
    }

    /// The other parameters whose expressions use the parameter `name`,
    /// by index.
    pub fn param_users(&self, name: &str) -> Vec<usize> {
        (self.params.iter().enumerate())
            .filter(|(_, param)| param.name != name && varde_expr::uses(&param.text, name))
            .map(|(index, _)| index)
            .collect()
    }

    /// Whether anything uses the parameter `name`: a feature's value or
    /// another parameter.
    pub fn param_used(&self, name: &str) -> bool {
        !self.param_uses(name).is_empty() || !self.param_users(name).is_empty()
    }

    /// The name `base` with the lowest number after it no parameter has,
    /// `base1` up, for a new one: `base` must be a word
    /// [`check_name`] takes with digits after it.
    pub fn new_param_name(&self, base: &str) -> String {
        (1..)
            .map(|n: u64| format!("{base}{n}"))
            .find(|name| self.params.iter().all(|param| param.name != *name))
            .expect("fewer parameters than numbers")
    }
}

#[cfg(test)]
mod tests;
