//! The MCP client's design-level edits (see `agents/mcp.md`, "Edits"):
//! renaming, a body's visibility, colour and opacity, removals, the
//! parameters, the units, the fit tolerance and the rollback marker,
//! each the commands the UI's own control makes.

use varde_document::{Command, Document, FeatureId, Named, Opacity, Removable, Tint, Tolerance};
use varde_expr::{Ask, LengthUnit, Value as ExprValue};
use varde_mcp::tools::{Color, Target, Units};
use varde_view::ParamField;

use super::edit::{LISTED, Made, Refused};
use super::names::{body_id, feature_id, not_named};
use crate::doc::params;

/// What an edit of one command makes.
type Commands = Result<(Vec<Command>, Made), Refused>;

fn one(command: Command, made: Made) -> Commands {
    Ok((vec![command], made))
}

/// `rename`: as the rename field commits it; none for the name it has:
/// nothing to change.
pub(super) fn rename(document: &Document, target: &Target, to: &str) -> Commands {
    let target = match target {
        Target::Feature(name) => Named::Feature(feature_id(document, name)?),
        Target::Body(name) => Named::Body(body_id(document, name)?),
    };
    let (commands, taken) = match document.rename(target, to) {
        Some(rename) => (vec![rename.command], rename.taken),
        None => (vec![], None),
    };
    Ok((commands, Made::Renamed { target, taken }))
}

// The body's settings as the Objects panel makes them; one as it is
// changes nothing, so the editor adds no step and the call says so.

/// `set_visible`.
pub(super) fn set_visible(document: &Document, body: &str, visible: bool) -> Commands {
    let id = body_id(document, body)?;
    one(Command::SetVisible(id, visible), Made::Body(id))
}

/// `set_color`: in range (the tools' check), so as the sliders round it.
pub(super) fn set_color(document: &Document, body: &str, color: Option<Color>) -> Commands {
    let id = body_id(document, body)?;
    let tint =
        color.map(|color| Tint::clamped(color.hue as f32, (color.saturation * 100.0) as f32));
    one(Command::SetColor(id, tint), Made::Body(id))
}

/// `set_opacity`.
pub(super) fn set_opacity(document: &Document, body: &str, opacity: f64) -> Commands {
    let id = body_id(document, body)?;
    let opacity = Opacity::clamped((opacity * 100.0) as f32);
    one(Command::SetOpacity(id, opacity), Made::Body(id))
}

/// `remove_feature`: see [`removal`].
pub(super) fn remove_feature(document: &Document, feature: &str) -> Commands {
    let id = feature_id(document, feature)?;
    removal(document, Removable::Feature(id))
}

/// `remove_body`: see [`removal`].
pub(super) fn remove_body(document: &Document, body: &str) -> Commands {
    let id = body_id(document, body)?;
    removal(document, Removable::Body(id))
}

/// `add_parameter`.
pub(super) fn add_parameter(document: &Document, name: &str, value: &str) -> Commands {
    let index = document.params().len();
    let command = Command::AddParam {
        name: name.trim().to_owned(),
        text: value.trim().to_owned(),
    };
    one(command, Made::Parameter(index))
}

/// `set_parameter` (`field` its expression) and `rename_parameter` (its
/// name): as the popup commits a field, none for what it is already.
pub(super) fn set_parameter(
    document: &Document,
    name: &str,
    field: ParamField,
    text: &str,
) -> Commands {
    let index = param_named(document, name)?;
    let commands = params::param_command(document, index, field, text);
    Ok((commands.into_iter().collect(), Made::Parameter(index)))
}

/// `remove_parameter`: refused while used, as the popup refuses it,
/// before the editor would.
pub(super) fn remove_parameter(document: &Document, name: &str) -> Commands {
    let index = param_named(document, name)?;
    let uses = params::uses(document, &document.params()[index].name);
    if !uses.is_empty() {
        let why = format!("it's used by {}: change those first", names(&uses));
        return Err(Refused::NotApplied(why));
    }
    let made = Made::ParameterRemoved(document.params()[index].name.clone());
    one(Command::RemoveParam(index), made)
}

/// `set_units`.
pub(super) fn set_units(units: Units) -> Commands {
    let units = match units {
        Units::Mm => LengthUnit::Mm,
        Units::In => LengthUnit::In,
    };
    one(Command::SetUnits(units), Made::Design)
}

/// `set_tolerance`: see [`tolerance`].
pub(super) fn set_tolerance(document: &Document, text: &str) -> Commands {
    let tolerance = tolerance(text, document.units())
        .map_err(|why| Refused::failed(format!("set_tolerance: {why}")))?;
    one(Command::SetTolerance(tolerance), Made::Design)
}

/// `set_rollback`: before the feature named `before`, or at the end.
pub(super) fn set_rollback(document: &Document, before: Option<&str>) -> Commands {
    let until = match before {
        Some(name) => Some(feature_id(document, name)?),
        None => None,
    };
    one(Command::SetRollback(until), Made::Design)
}

/// Removing `target` without the user's cascade
/// ([`Document::breaking_removal`], as Delete only applies it): refused,
/// naming them, if later features use it, which the cascade would take
/// ([`Document::removal`]).
fn removal(document: &Document, target: Removable) -> Result<(Vec<Command>, Made), Refused> {
    let only = document.breaking_removal(&[target]);
    let all = document.removal(target);
    // By name in the Timeline's order, as the delete prompt lists them,
    // `ids` sorted (a `Removal`'s are).
    let in_order = |ids: &[FeatureId]| -> Vec<String> {
        (document.features().iter())
            .filter(|feature| ids.binary_search(&feature.id).is_ok())
            .map(|feature| feature.name.clone())
            .collect()
    };
    let dependents: Vec<FeatureId> = (all.features.iter())
        .filter(|id| only.features.binary_search(id).is_err())
        .copied()
        .collect();
    if !dependents.is_empty() {
        let why = format!(
            "later features use it: {}. Remove those first, the last first, or change them",
            names(&in_order(&dependents))
        );
        return Err(Refused::NotApplied(why));
    }
    let bodies = (document.bodies().iter())
        .filter(|body| only.bodies.binary_search(&body.id).is_ok())
        .map(|body| body.name.clone())
        .collect();
    let made = Made::Removed {
        features: in_order(&only.features),
        bodies,
    };
    Ok((vec![Command::RemoveOnly(only.features)], made))
}

/// The fit tolerance `text` asks for, in the design's `units`: a length
/// as a field reads it, or micrometres (`"1 µm"`, as `design_summary`
/// shows it), within [`Tolerance`]'s range.
fn tolerance(text: &str, units: LengthUnit) -> Result<Tolerance, String> {
    let text = text.trim();
    let micrometres = (text.strip_suffix("µm")).or_else(|| text.strip_suffix("um"));
    let fit = match micrometres {
        Some(number) => {
            let ask = Ask::number(units, Tolerance::MAX_FIT * 1000.0)
                .positive()
                .at_least(Tolerance::MIN_FIT * 1000.0);
            ExprValue::new(number, &ask)
                .map_err(|why| why.to_string())?
                .value
                / 1000.0
        }
        None => {
            let ask = Ask::length(units, Tolerance::MAX_FIT)
                .positive()
                .at_least(Tolerance::MIN_FIT);
            ExprValue::new(text, &ask)
                .map_err(|why| why.to_string())?
                .value
        }
    };
    Tolerance::new(fit).ok_or_else(|| "the tolerance is from 0.01 µm to 0.1 mm".to_owned())
}

/// The index of the parameter named `name`.
fn param_named(document: &Document, name: &str) -> Result<usize, Refused> {
    (document.params().iter())
        .position(|param| param.name == name.trim())
        .ok_or_else(|| Refused::failed(not_named("parameter", name)))
}

/// `names` for a sentence, at most [`LISTED`] of them, then how many
/// more.
fn names(names: &[String]) -> String {
    let mut said = (names.iter().take(LISTED))
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(more) = names.len().checked_sub(LISTED).filter(|more| *more > 0) {
        said.push_str(&format!(" and {more} more"));
    }
    said
}
