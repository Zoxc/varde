//! Parameter and unit edits: every value naming a parameter evaluated
//! again, or every value pinned to the units it had.

use varde_expr::{LengthUnit, exact};
use varde_sketch::{Budget, Unrevalued};

use super::{EditError, ValueOf};
use crate::{Document, FeatureKind, Param, param};

impl Document {
    /// The document with `params` for its parameters, resolved, and every
    /// feature value and sketch dimension naming a parameter evaluated
    /// again, as the parameter commands do, each sketch whose dimensions
    /// change solved again ([`varde_sketch::revalue`]) within `budget`; with `renamed`, the old name and the new, each use of
    /// the old in feature values (and in `params`, which the caller
    /// renames) written as the new first. `None` if nothing changes.
    pub(super) fn with_params(
        &self,
        params: Vec<Param>,
        renamed: Option<(&str, &str)>,
        budget: &Budget,
    ) -> Result<Option<Document>, EditError> {
        if params == self.params {
            return Ok(None);
        }
        param::check_params(&params).map_err(EditError::Invalid)?;
        let mut next = self.clone();
        next.resolved = param::resolve(&params, self.units);
        next.params = params;
        let design = next.design();
        let mut changed = Vec::new();
        let mut sketches = Vec::new();
        for (index, feature) in next.features.iter().enumerate() {
            if let FeatureKind::Sketch { sketch, .. } = &feature.kind {
                let error = |why| EditError::Value(ValueOf::Feature(feature.id), why);
                let mut renamed_sketch = None;
                if let Some((old, new)) = renamed {
                    let mut copy = sketch.clone();
                    for entry in &mut copy.dimensions {
                        let value = &mut entry.dimension.value;
                        value.text = varde_expr::rename(&value.text, old, new).map_err(error)?;
                    }
                    renamed_sketch = Some(copy);
                }
                let base = renamed_sketch.as_ref().unwrap_or(sketch);
                // Solved within the caller's bound, as a proposal is, but
                // here and now: the change is one undo step.
                let solved = match varde_sketch::revalue(base, &design, budget) {
                    Ok(solved) => solved.or(renamed_sketch),
                    Err(Unrevalued::Value(_, why)) => return Err(error(why)),
                    Err(Unrevalued::Rejected(why)) => {
                        return Err(EditError::Unsolved(feature.id, why));
                    }
                };
                if let Some(solved) = solved.filter(|solved| solved != sketch) {
                    sketches.push((index, solved));
                }
                continue;
            }
            let mut kind = feature.kind.clone();
            for (value, ask) in kind.values_mut(&design) {
                let error = |why| EditError::Value(ValueOf::Feature(feature.id), why);
                if let Some((old, new)) = renamed {
                    value.text = varde_expr::rename(&value.text, old, new).map_err(error)?;
                }
                if !varde_expr::names(&value.text).is_empty() {
                    value.value = varde_expr::evaluate(&value.text, &ask).map_err(error)?;
                }
            }
            if kind != feature.kind {
                changed.push((feature.id, Box::new(kind)));
            }
        }
        for (index, sketch) in sketches {
            next.set_sketch(index, sketch, None);
        }
        for (feature, kind) in changed {
            // Planned against `next` as each before it left it, as
            // a `SetFeature` each would be.
            if let Some(plan) = next.plan_kind(feature, kind)? {
                next.set_kind(plan)?;
            }
        }
        Ok(Some(next))
    }

    /// The document in `units`, every value and parameter pinned to the
    /// units it had, as [`Command::SetUnits`](crate::Command::SetUnits) does; `None` if they're
    /// the units it has.
    pub(super) fn with_units(&self, units: LengthUnit) -> Option<Document> {
        if units == self.units {
            return None;
        }
        let mut next = self.clone();
        let before = self.design();
        for feature in &mut next.features {
            if let FeatureKind::Sketch { sketch, .. } = &mut feature.kind {
                sketch.pin_units(&before);
            }
            // Counts, factors and turns have no unit, but are
            // pinned all the same: a bare number added to a length
            // inside one took the units.
            for (value, ask) in feature.kind.values_mut(&before) {
                value.pin_units(&ask);
            }
        }
        for (param, result) in next.params.iter_mut().zip(self.resolved.iter()) {
            if let (_, Ok(resolved)) = result {
                param.text = (self.resolved)
                    .pin_units(&param.text, self.units)
                    .unwrap_or_else(|_| exact(resolved.value, resolved.quantity));
            }
        }
        next.units = units;
        next.resolved = param::resolve(&next.params, units);
        Some(next)
    }
}
