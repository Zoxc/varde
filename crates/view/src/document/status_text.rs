//! What the document screen's status bar says: its hints, what's going
//! on, and what's selected. The bar itself is `crate::status`.

use iced::widget::text::Wrapping;
use iced::widget::{row, text};
use iced::{Alignment, Element};
use varde_document::{BodyId, FeatureKind};
use varde_sketch::{Id, Selectable, TooComplex};

use crate::chrome::{self, Hint, chord_hint, key_hint, mouse_hint, step_hint};
use crate::feature_text::{feature_icon, feature_info};
use crate::icons::{self, Icon, MouseButton};
use crate::shortcut::{Held, Shortcut};
use crate::status::Status;
use crate::words::{counted, counts};
use crate::{Message, SketchItem, theme, viewport};

mod sketch;

use sketch::sketch_hints;

use super::prompts::{edit_refused, name_action};
use super::{DocumentState, MeshStatus, OperationState, Overlay, SketchState};

/// The status bar's text: small and on one line.
fn bar_text<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    text(content).size(12).wrapping(Wrapping::None)
}

/// A title in the status bar, e.g. what's selected.
fn bar_title<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    bar_text(content).font(theme::SEMIBOLD)
}

/// A muted detail in the status bar.
fn bar_note<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    bar_text(content).style(theme::muted_text)
}

/// What the status bar shows: the feature selected, what's going on, the
/// hints, and the view options menu's button.
pub(super) fn status<'a>(state: &DocumentState<'a>) -> Status<'a> {
    Status {
        selection: selection(state),
        info: info(state),
        hints: hints(state),
        mouse_hints: state.options.mouse_hints,
        chip: crate::mcp::status_chip(state.mcp.as_ref()),
        view_menu: Some(matches!(state.overlay, Some(Overlay::ViewMenu(_)))),
    }
}

/// The hints of `hints` there are, in order.
fn hint_list<'a>(hints: impl IntoIterator<Item = Option<Hint<'a>>>) -> Vec<Hint<'a>> {
    hints.into_iter().flatten().collect()
}

/// An operation's hints: its pick step, if any, then the keys every
/// operation shares (held, the key shows the model before it).
fn operation_hints<'a>(pick: Option<&'a str>, ready: bool) -> Vec<Hint<'a>> {
    hint_list([
        pick.map(|what| step_hint(MouseButton::Left, what)),
        Some(key_hint(Held::BEFORE, "Hide preview")),
        ready.then(|| key_hint(Shortcut::ENTER, "OK")),
        Some(key_hint(Shortcut::ESCAPE, "Cancel")),
    ])
}

/// An operation's status: its bold title, then the muted detail and the
/// state's suffix.
fn operation_status<'a>(
    title: &'a str,
    detail: &str,
    state: &DocumentState<'a>,
) -> Option<Element<'a, Message>> {
    Some(
        row![
            bar_title(title),
            bar_note(format!("· {detail}{}", status_suffix(state))),
        ]
        .spacing(4)
        .into(),
    )
}

/// The status bar's hints: what the keys do for what's going on and the
/// viewport's mouse bindings; under the unsaved changes or delete prompt
/// only that `Esc` cancels it, under the name prompt what `Enter` does, if
/// anything, and that `Esc` cancels it.
fn hints<'a>(state: &DocumentState<'a>) -> Vec<Hint<'a>> {
    // The unsaved changes and delete prompts take every key but `Esc`,
    // and the viewport behind them nothing.
    if state.overlay == Some(Overlay::UnsavedPrompt) || state.deleting.is_some() {
        return vec![key_hint(Shortcut::ESCAPE, "Cancel")];
    }
    // The name prompt's field takes the keys: `Enter` confirms it, as
    // its button says, unless that's blocked, and `Esc` cancels it.
    if state.overlay == Some(Overlay::NamePrompt)
        && let Some(prompt) = &state.file.naming
    {
        let (action, blocked) = name_action(prompt);
        let enter = (!blocked).then(|| key_hint(Shortcut::ENTER, action));
        return hint_list([enter, Some(key_hint(Shortcut::ESCAPE, "Cancel"))]);
    }
    let sketching = !left_orbits(state.sketch.as_ref());
    let keys: Vec<_> = if state.picking_plane.is_some() {
        vec![key_hint(Shortcut::ESCAPE, "Cancel")]
    } else if let Some(operation) = &state.operation {
        if let OperationState::Measure(measure) = operation {
            measure_hints(measure)
        } else {
            let ready = operation.verdict().is_some_and(|verdict| verdict.ready);
            operation_hints(operation.pick_hint(), ready)
        }
    } else if let Some(sketch) = state.sketch {
        sketch_hints(&sketch, state.editable(), state.options.mouse.sketch_snap)
    } else if state.selected_feature.is_some() {
        let delete = state
            .editable()
            .then(|| key_hint(Shortcut::DELETE, "Delete"));
        hint_list([Some(key_hint(Shortcut::ENTER, "Edit")), delete])
    } else if state.picking.is_some() {
        selecting_hints(state.model_selection)
    } else {
        Vec::new()
    };
    keys.into_iter()
        .chain(viewport::hints(sketching, &state.options.mouse))
        .collect()
}

/// Whether the left button orbits the camera, as outside a sketch: not
/// in `sketch`, where it's the sketch's, but while its tool picks outside
/// it ([`Tool::picks_outside`](crate::Tool::picks_outside)).
fn left_orbits(sketch: Option<&SketchState<'_>>) -> bool {
    sketch.is_none_or(|sketch| (sketch.tool).is_some_and(|tool| tool.tool.picks_outside()))
}

/// The status bar's hints for the measure tool: a click picks A, then B,
/// then A again; `Shift` with it replaces B; a double-click picks the
/// body; `Esc` leaves.
fn measure_hints<'a>(measure: &crate::MeasureState<'a>) -> Vec<Hint<'a>> {
    let click = match measure.picks {
        [Some(_), None] => "Pick B",
        _ => "Pick A",
    };
    vec![
        step_hint(MouseButton::Left, click),
        chord_hint(Held::TOGGLE, MouseButton::Left, "Replace B"),
        chrome::double_hint(MouseButton::Left, "Body"),
        key_hint(Shortcut::ESCAPE, "Done"),
    ]
}

/// The status bar's hints for selecting in the model with `selection`:
/// clicking to select and, once something is, holding [`Held::TOGGLE`] to
/// add or take out, and double-clicking for the body where that selects
/// it.
fn selecting_hints<'a>(selection: &crate::Selection) -> Vec<Hint<'a>> {
    use crate::SelectionMode;

    let click = if selection.is_empty() {
        mouse_hint(MouseButton::Left, "Select")
    } else {
        chord_hint(Held::TOGGLE, MouseButton::Left, "Add or remove")
    };
    let body = (selection.mode() == SelectionMode::Any)
        .then(|| chrome::double_hint(MouseButton::Left, "Body"));
    hint_list([Some(click), body])
}

/// What the status bar says while picking the plane for a new sketch
/// with a curved face under the cursor, and the app when a sketch is
/// asked for on one.
pub const CURVED_FACE: &str = "Only flat faces can be sketched on";

/// What the status bar says is going on: what's asked of the user while
/// picking a plane, what the sketch being edited holds or the extrude being
/// set up. Whether the mesh is still being regenerated, or why it couldn't
/// be built, if it couldn't; why the last edit was refused, if it was, and
/// whether a save is in flight. Nothing, with nothing of that, so with
/// nothing selected the bar has only its hints. On one line, cut short
/// where the bar doesn't fit (`status::status_bar`).
fn info<'a>(state: &DocumentState<'a>) -> Option<Element<'a, Message>> {
    if let Some(pick) = state.picking_plane {
        // A face hovered that can't take the sketch isn't highlighted:
        // this says why.
        let refused = (state.picking.as_ref()).and_then(|picking| match picking.hovered {
            Some(crate::Picked::Face(face)) => pick.refusal(picking.index, face),
            _ => None,
        });
        let (line, style): (String, fn(&iced::Theme) -> text::Style) = match refused {
            Some(why) => (why.into_owned(), theme::danger_text),
            None => (pick.asking(), theme::accent_text),
        };
        return Some(bar_title(line).style(style).into());
    }
    if let Some(sketch) = &state.sketch {
        // A selection has the bar's box of its own: the sketch's standing
        // and profiles make way for it.
        let selected = !sketch.selection.is_empty();
        let piece =
            |line: String, style: fn(&iced::Theme) -> text::Style| bar_text(line).style(style);
        let standing = (!selected).then(|| standing(sketch)).flatten();
        let mut pieces: Vec<_> = (standing.into_iter())
            .map(|(standing, trouble)| {
                let style = if trouble {
                    theme::danger_text
                } else {
                    theme::muted_text
                };
                piece(standing, style)
            })
            .chain(
                (!selected)
                    .then(|| profile_count(sketch))
                    .flatten()
                    .map(|count| piece(count, theme::muted_text)),
            )
            .chain(
                status_notes(state)
                    .into_iter()
                    .map(|note| piece(note, theme::muted_text)),
            )
            .chain(
                (sketch.refusal)
                    .map(|refusal| edit_refused(refusal, sketch.sketch))
                    .map(|why| piece(why.into_owned(), theme::danger_text)),
            )
            .chain(
                sketch
                    .checking
                    .then(|| piece("Checking…".to_owned(), theme::muted_text)),
            )
            .map(Element::from)
            .collect();
        if pieces.is_empty() {
            return None;
        }
        let mut line = Vec::with_capacity(pieces.len() * 2);
        for (i, element) in pieces.drain(..).enumerate() {
            if i > 0 {
                line.push(piece("·".to_owned(), theme::muted_text).into());
            }
            line.push(element);
        }
        return Some(row(line).spacing(4).into());
    }
    if let Some(operation) = &state.operation
        && let (Some((_, title, _)), Some(detail)) = (operation.heading(), operation.status_info())
    {
        return operation_status(title, &detail, state);
    }
    let notes = status_notes(state);
    (!notes.is_empty()).then(|| bar_note(notes.join(" · ")).into())
}

/// The feature selected in the Timeline, for the status bar's box of the
/// selection: its icon, its name and [`feature_info`], or for one that
/// failed an alert, its name and why. Nothing in a sketch,
/// setting up an extrude or a revolve or picking a plane, which the bar
/// tells of instead.
fn selection<'a>(state: &DocumentState<'a>) -> Option<Element<'a, Message>> {
    if let Some(sketch) = &state.sketch {
        return sketch_selection(sketch);
    }
    if state.picking_plane.is_some() || state.operation.is_some() {
        return None;
    }
    let document = state.editor.document();
    let Some(feature) = state.selected_feature.and_then(|id| document.feature(id)) else {
        return model_selection(state);
    };
    // A feature that failed says why, after an alert in place of its icon.
    let failed = (state.failed.iter()).find(|failed| failed.feature == feature.id);
    if let Some(failed) = failed {
        return Some(
            row![
                icons::tinted(Icon::Alert, 14.0, |p| p.danger),
                bar_title(feature.name.as_str()),
                bar_note(crate::chrome::sentence(&failed.message)),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
        );
    }
    Some(
        row![
            icons::icon(feature_icon(feature), icons::INLINE),
            bar_title(feature.name.as_str()),
            bar_note(feature_info(feature, document)),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    )
}

/// What's selected in the sketch being edited, for the status bar's box
/// of the selection, while no tool is in use nor the value field open:
/// one item by name ("Line 3"), or how many, and what they measure
/// together as a dimension of them would ("Length 40 mm", see
/// [`dimension::selected`](crate::dimension::selected)); or a rectangle,
/// by its width and height (see
/// [`dimension::rectangle`](crate::dimension::rectangle)).
fn sketch_selection<'a>(sketch: &SketchState<'a>) -> Option<Element<'a, Message>> {
    if sketch.tool.is_some() || sketch.constraining || sketch.value.is_some() {
        return None;
    }
    let ids: Vec<Selectable> = sketch.selection.iter().copied().collect();
    let items: Option<Vec<Id>> = ids.iter().map(|target| target.item()).collect();
    let rectangle = items.and_then(|items| crate::dimension::rectangle(sketch.sketch, &items));
    let title = match ids[..] {
        [] => return None,
        _ if rectangle.is_some() => "Rectangle".to_owned(),
        [one] => (sketch.sketch.selectable_name(one)).unwrap_or_else(|| "1 selected".to_owned()),
        _ => format!("{} selected", ids.len()),
    };
    let measured = match rectangle {
        Some((width, height)) => {
            let unit = Some(varde_expr::Unit::Length(sketch.units));
            let [width, height] =
                [width, height].map(|size| varde_expr::Shown::current().format(size, unit));
            Some(format!("Width {width} · Height {height}"))
        }
        None => crate::dimension::selected(sketch.sketch, &ids)
            .map(|(measure, value)| crate::dimension::shown_measure(&measure, value, sketch.units)),
    };
    let measured = measured.map(bar_text);
    Some(
        row![
            icons::icon(Icon::Sketch, icons::INLINE),
            bar_title(title),
            measured,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    )
}

/// What's selected in the model, for the status bar's box of the
/// selection: one face, as "Face", what surface it's on (or "Rectangle",
/// for a flat face of four straight edges square at its corners) and its
/// body's name; one edge or vertex, as "Edge" or "Vertex" and its body's; one
/// body, by name; or how many, of each kind. Before the body's name, what
/// one or two items measure once the answer has it (see
/// [`measure::brief`](crate::measure::brief)). Nothing if nothing is
/// selected, or the cursor doesn't pick the model.
fn model_selection<'a>(state: &DocumentState<'a>) -> Option<Element<'a, Message>> {
    use crate::{Picked, Selected};

    let selection = state.model_selection;
    let picking = state.picking.as_ref()?;
    let document = state.editor.document();
    let body_name = |body: BodyId| document.body(body).map_or("", |body| body.name.as_str());
    let items: Vec<&Selected> = selection.items().collect();
    let fresh = selection.model() == Some(picking.index.model());
    let target = selection.targets().next().filter(|_| fresh);
    // The body a face or edge is of in the model: another than the one
    // it was picked in where a join merged that one into it.
    let drawn_in = |body: BodyId| {
        let drawn = target.and_then(|target| picking.index.body(target));
        body_name(drawn.unwrap_or(body))
    };
    let (title, info, note): (String, String, &str) = match items[..] {
        [] => return None,
        [Selected::Face { body, .. }] => {
            let summary = target.and_then(|target| {
                let Picked::Face(face) = target else {
                    return None;
                };
                picking.index.picking().faces().get(face as usize)
            });
            let surface = summary.map_or("", |face| surface_name(&face.summary));
            // A rectangle is named so once measured, its width and
            // height then shown.
            let rectangle = (state.selection_measured)
                .and_then(|inspected| inspected.first.as_ref().ok())
                .is_some_and(|probed| {
                    matches!(
                        probed.measure,
                        Ok(varde_regen::Measure::Face {
                            rectangle: Some(_),
                            ..
                        })
                    )
                });
            let surface = if rectangle { "Rectangle" } else { surface };
            ("Face".into(), surface.into(), drawn_in(*body))
        }
        [Selected::Edge { body, .. }] => ("Edge".into(), String::new(), drawn_in(*body)),
        [Selected::Vertex { body, .. }] => ("Vertex".into(), String::new(), drawn_in(*body)),
        [Selected::Body(body)] => (body_name(*body).into(), String::new(), "Body"),
        // "Line 3" of "Sketch 2".
        [Selected::SketchItem(SketchItem { sketch, item })] => {
            let feature = document.feature(*sketch);
            let name = feature.and_then(|feature| match &feature.kind {
                FeatureKind::Sketch { sketch, .. } => sketch.name(*item),
                _ => None,
            });
            let of = feature.map_or("", |feature| feature.name.as_str());
            (name.unwrap_or_default(), String::new(), of)
        }
        _ => {
            let count =
                |kind: fn(&Selected) -> bool| items.iter().filter(|item| kind(item)).count();
            let faces = count(|item| matches!(item, Selected::Face { .. }));
            let edges = count(|item| matches!(item, Selected::Edge { .. }));
            let vertices = count(|item| matches!(item, Selected::Vertex { .. }));
            let bodies = count(|item| matches!(item, Selected::Body(_)));
            let sketch_items = count(|item| matches!(item, Selected::SketchItem(_)));
            let parts = counts([
                (faces, "face", "faces"),
                (edges, "edge", "edges"),
                (vertices, "vertex", "vertices"),
                (bodies, "body", "bodies"),
                (sketch_items, "sketch item", "sketch items"),
            ]);
            (format!("{} selected", items.len()), parts.join(" · "), "")
        }
    };
    let info = (!info.is_empty()).then(|| bar_text(info));
    let measured = (state.selection_measured)
        .map(|inspected| crate::measure::brief(inspected, document.units()))
        .filter(|values| !values.is_empty())
        .map(|values| {
            let values: Vec<String> = (values.into_iter())
                .map(|value| format!("{} {}", value.label, value.shown))
                .collect();
            bar_text(values.join(" · "))
        });
    let note = (!note.is_empty()).then(|| bar_note(note));
    Some(
        row![
            icons::icon(Icon::Body, icons::INLINE),
            bar_title(title),
            info,
            measured,
            note,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    )
}

/// What a face of `summary` is on, in a word or two.
fn surface_name(summary: &varde_regen::Summary) -> &'static str {
    use varde_regen::Summary;
    match summary {
        Summary::Plane { .. } => "Plane",
        Summary::Cylinder { .. } => "Cylinder",
        Summary::Cone { .. } => "Cone",
        Summary::Sphere { .. } => "Sphere",
        Summary::Torus { .. } => "Torus",
        Summary::ConicCylinder { .. } | Summary::Revolved { .. } | Summary::Other => "Curved",
    }
}

/// Where `sketch` stands, for the status bar, once that's known, and
/// whether that's trouble: "Fully constrained", "4 degrees of freedom
/// left", or that it doesn't solve or holds constraints in conflict.
fn standing(sketch: &SketchState<'_>) -> Option<(String, bool)> {
    // Nothing drawn is nothing to constrain.
    if sketch.sketch.points.is_empty() {
        return None;
    }
    let Some(analysis) = sketch.analysis else {
        return sketch.unsolved.then(|| ("Doesn't solve".to_owned(), true));
    };
    Some(if !analysis.solved {
        ("Doesn't solve".to_owned(), true)
    } else if !analysis.redundant.is_empty() {
        ("Over-constrained".to_owned(), true)
    } else if analysis.freedom == 0 {
        ("Fully constrained".to_owned(), false)
    } else {
        let freedom = counted(analysis.freedom, "degree", "degrees");
        (format!("{freedom} of freedom left"), false)
    })
}

/// How many profiles `sketch` has, for the status bar, once the app has
/// looked: "2 profiles", or that it's too complex to say. Nothing for a
/// sketch without curves.
fn profile_count(sketch: &SketchState<'_>) -> Option<String> {
    if sketch.sketch.curves.is_empty() {
        return None;
    }
    Some(match sketch.profiles? {
        Ok(profiles) => counted(profiles.regions.len(), "profile", "profiles"),
        Err(TooComplex) => "Too complex for profiles".to_owned(),
    })
}

/// What the status bar tells of the model and the file, whatever else
/// it says: regenerating, why the last edit failed, saving, exporting.
fn status_notes(state: &DocumentState<'_>) -> Vec<String> {
    let regenerating = match state.mesh_status {
        // Shown over the viewport once it's slow, see `regenerating`.
        MeshStatus::Current | MeshStatus::Regenerating => None,
        MeshStatus::Failed(error) => Some(format!("Couldn't regenerate: {error}")),
    };
    let edit_error = state
        .edit_error
        .map(|error| format!("Couldn't edit: {error}"));
    let notice = state.notice.map(str::to_owned);
    let saving = state.file.saving.then(|| "Saving…".to_owned());
    let exporting = state.file.exporting.then(|| "Exporting…".to_owned());
    [regenerating, edit_error, notice, saving, exporting]
        .into_iter()
        .flatten()
        .collect()
}

/// [`status_notes`] after what the status bar says of the sketch or the
/// extrude.
fn status_suffix(state: &DocumentState<'_>) -> String {
    status_notes(state)
        .iter()
        .map(|note| format!(" · {note}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use glam::DVec2;
    use varde_document::{Document, Feature};
    use varde_sketch::{Analysis, Curve, Profiles, Sketch, TooComplex};

    use super::{left_orbits, profile_count, standing};
    use crate::Tool;
    use crate::document::SketchState;
    use crate::feature_text::sketch_summary;
    use crate::feature_text::{feature_icon, feature_info};
    use crate::icons::Icon;

    /// The status bar's mouse hints say the left button orbits outside a
    /// sketch and in one while Project or Intersect picks outside it.
    #[test]
    fn the_left_button_orbits_outside_a_sketch_and_picking_outside_it() {
        let sketch = Sketch::default();
        let none = BTreeSet::new();
        let with = |tool: Option<Tool>| {
            let tool = tool.map(|tool| crate::testing::tool(tool, &[], &[]));
            left_orbits(Some(&SketchState::plain(&sketch, &none, tool)))
        };
        assert!(left_orbits(None));
        assert!(!with(None));
        assert!(!with(Some(Tool::Line)));
        assert!(!with(Some(Tool::Trim)));
        assert!(with(Some(Tool::Project)));
        assert!(with(Some(Tool::Intersect)));
    }

    #[test]
    fn a_sketch_stands_as_its_analysis_says() {
        let mut sketch = Sketch::default();
        let none = BTreeSet::new();
        let mut analysis = Analysis {
            freedom: 0,
            ..Analysis::default()
        };
        let standing_of = |sketch: &Sketch, analysis: Option<&Analysis>, unsolved| {
            standing(&SketchState {
                analysis,
                unsolved,
                ..SketchState::plain(sketch, &none, None)
            })
        };
        assert_eq!(standing_of(&sketch, Some(&analysis), false), None);
        let point = sketch.add_point(DVec2::ZERO).unwrap();
        assert_eq!(standing_of(&sketch, None, false), None);
        assert_eq!(
            standing_of(&sketch, None, true),
            Some(("Doesn't solve".to_owned(), true))
        );
        analysis.solved = true;
        assert_eq!(
            standing_of(&sketch, Some(&analysis), false),
            Some(("Fully constrained".to_owned(), false))
        );
        analysis.freedom = 1;
        assert_eq!(
            standing_of(&sketch, Some(&analysis), false),
            Some(("1 degree of freedom left".to_owned(), false))
        );
        analysis.redundant.insert(point);
        assert_eq!(
            standing_of(&sketch, Some(&analysis), false),
            Some(("Over-constrained".to_owned(), true))
        );
        analysis.solved = false;
        assert_eq!(
            standing_of(&sketch, Some(&analysis), true),
            Some(("Doesn't solve".to_owned(), true))
        );
    }

    #[test]
    fn a_sketch_is_summed_up_by_kind() {
        let mut sketch = Sketch::default();
        assert_eq!(sketch_summary(&sketch), "Empty");
        let a = sketch.add_point(DVec2::ZERO).unwrap();
        assert_eq!(sketch_summary(&sketch), "1 point");
        let b = sketch.add_point(DVec2::X).unwrap();
        let c = sketch.add_point(DVec2::Y).unwrap();
        sketch
            .add_curve(Curve::Line { start: a, end: b }, false)
            .unwrap();
        sketch
            .add_curve(Curve::Line { start: b, end: c }, true)
            .unwrap();
        sketch
            .add_curve(
                Curve::Circle {
                    center: a,
                    radius: 1.0,
                },
                false,
            )
            .unwrap();
        assert_eq!(sketch_summary(&sketch), "2 lines · 1 circle · 3 points");
        // A chamfer is counted apart from the lines.
        let (start, end) = (
            sketch.add_point(DVec2::new(0.5, 0.0)).unwrap(),
            sketch.add_point(DVec2::new(0.5, 0.5)).unwrap(),
        );
        let chamfer = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        let lines = [sketch.curves[0].id, sketch.curves[1].id];
        sketch.curve_mut(chamfer).unwrap().corner = Some(varde_sketch::Corner {
            a: lines[0],
            b: lines[1],
            at: b,
            equal: false,
        });
        assert_eq!(
            sketch_summary(&sketch),
            "2 lines · 1 circle · 1 chamfer · 5 points"
        );
    }

    #[test]
    fn the_profiles_are_counted_once_found() {
        let mut sketch = Sketch::default();
        let none = BTreeSet::new();
        let count = |sketch: &Sketch, profiles| {
            profile_count(&SketchState {
                profiles,
                ..SketchState::plain(sketch, &none, None)
            })
        };
        let found = Ok(Arc::new(sketch.profiles().unwrap()));
        // Nothing drawn, nothing to count.
        assert_eq!(count(&sketch, Some(&found)), None);
        let center = sketch.add_point(DVec2::ZERO).unwrap();
        for radius in [1.0, 2.0] {
            let circle = Curve::Circle { center, radius };
            sketch.add_curve(circle, false).unwrap();
        }
        // Not until the app has looked.
        assert_eq!(count(&sketch, None), None);
        // The disc and the ring round it.
        let found = Ok(Arc::new(sketch.profiles().unwrap()));
        assert_eq!(count(&sketch, Some(&found)), Some("2 profiles".to_owned()));
        let one = Ok(Arc::new(Profiles {
            regions: found.as_ref().unwrap().regions[..1].to_vec(),
            ..Profiles::default()
        }));
        assert_eq!(count(&sketch, Some(&one)), Some("1 profile".to_owned()));
        assert_eq!(
            count(&sketch, Some(&Err(TooComplex))),
            Some("Too complex for profiles".to_owned())
        );
    }

    #[test]
    fn the_status_bar_sums_up_the_feature_selected() {
        use varde_document::{Document, Extent, FeatureKind, Operation};

        let document = Document::example();
        let [sketch, extrude] = [0, 1].map(|k| &document.features()[k]);
        assert_eq!(
            feature_info(sketch, &document),
            "4 lines · 1 circle · 5 points · on XY"
        );
        assert_eq!(
            feature_info(extrude, &document),
            "Distance 10 mm · New body"
        );

        let FeatureKind::Extrude(mut changed) = extrude.kind.clone() else {
            panic!("the example's second feature is its extrude");
        };
        let Extent::OneSide(distance) = changed.extent.clone() else {
            panic!("the example's extrude goes one side");
        };
        let mut info = |extent: Extent, operation: Operation| {
            changed.extent = extent;
            changed.operation = operation;
            let feature = Feature {
                kind: FeatureKind::Extrude(changed.clone()),
                ..extrude.clone()
            };
            feature_info(&feature, &document)
        };
        let cut = Operation::Cut(Default::default());
        assert_eq!(
            info(Extent::Symmetric(distance.clone()), cut.clone()),
            "Symmetric 10 mm · Cut"
        );
        assert_eq!(
            info(Extent::TwoSides(distance.clone(), distance), cut.clone()),
            "Two sides 10 mm + 10 mm · Cut"
        );
        assert_eq!(info(Extent::ThroughAll, cut.clone()), "Through all · Cut");
        changed.extent = Extent::ThroughAll;
        changed.operation = cut;
        changed.taper = Some(varde_expr::Value {
            text: "1.5".to_owned(),
            value: 1.5f64.to_radians(),
        });
        let tapered = Feature {
            kind: FeatureKind::Extrude(changed),
            ..extrude.clone()
        };
        assert_eq!(
            feature_info(&tapered, &document),
            "Through all · Taper 1.5° · Cut"
        );
    }

    #[test]
    fn a_revolve_s_row_says_its_total_turn_and_the_status_bar_its_axis() {
        use varde_document::{AxisLine, FeatureKind, Operation, Revolve, Turn};

        let document = Document::example();
        let [sketch, extrude] = [0, 1].map(|k| &document.features()[k]);
        let FeatureKind::Extrude(extrude_kind) = &extrude.kind else {
            panic!("the example's second feature is its extrude");
        };
        let ask = Turn::ask(&document.design());
        let angle = |text| varde_expr::Value::new(text, &ask).unwrap();
        let revolve = |extent: Turn, axis: AxisLine| Feature {
            kind: FeatureKind::Revolve(Revolve {
                sketch: sketch.id,
                regions: extrude_kind.regions.clone(),
                axis,
                extent,
                flip: false,
                operation: Operation::Cut(Default::default()),
            }),
            ..extrude.clone()
        };
        let x = AxisLine::SketchX;
        let cases = [
            (Turn::Full, "360°", "Full 360° · Cut · about X axis"),
            (
                Turn::OneSide(angle("90")),
                "90°",
                "One side 90° · Cut · about X axis",
            ),
            (
                Turn::Symmetric(angle("90")),
                "90°",
                "Symmetric 90° · Cut · about X axis",
            ),
            (
                Turn::TwoSides(angle("90"), angle("45")),
                "135°",
                "Two sides 90° + 45° · Cut · about X axis",
            ),
        ];
        for (extent, note, info) in cases {
            assert_eq!(crate::feature_text::revolve::turn_note(&extent), note);
            assert_eq!(feature_info(&revolve(extent, x), &document), info);
        }
        // The Y axis, and a line of the sketch, by their names.
        let y = revolve(Turn::Full, AxisLine::SketchY);
        assert_eq!(
            feature_info(&y, &document),
            "Full 360° · Cut · about Y axis"
        );
        let FeatureKind::Sketch { sketch: drawn, .. } = &sketch.kind else {
            panic!("the example's first feature is its sketch");
        };
        let line = drawn.curves.first().unwrap();
        let on_line = revolve(Turn::Full, AxisLine::Curve(line.id));
        assert_eq!(
            feature_info(&on_line, &document),
            format!("Full 360° · Cut · about {}", line.name())
        );
        assert_eq!(feature_icon(&y), Icon::Revolve);
    }
}
