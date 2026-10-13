//! The banners over the document screen's viewport and its prompts:
//! unsaved changes, naming a design, deleting.

use std::borrow::Cow;

use iced::widget::text::Wrapping;
use iced::widget::{Button, Space, column, container, row, space, text, text_input};
use iced::{Alignment, Element, Length};
use varde_document::{APP_NAME, EXTENSION};
use varde_sketch::{Failure, Rejected, Sketch};

use crate::chrome::{self, dialog, dialog_button, small_button};
use crate::feature_text::feature_icon;
use crate::icons::{self, Icon};
use crate::shortcut::Held;
use crate::theme::Emphasis;
use crate::{Edit, File, Look, Message, SavePlace, Unsaved, panels, theme};

use super::{
    Damage, DamagedFile, DeletePrompt, FileShown, NamePrompt, RecoveredChanges, Refusal,
    RefusedEdit,
};
use crate::words::{counts, listed};

/// The banner over the viewport saying a sketch edit was refused after
/// its sketch was left, and why, with Dismiss: "An edit of Sketch 1
/// wasn't kept — Would over-constrain the sketch".
pub(super) fn refused_banner(refused: RefusedEdit<'_>) -> Element<'_, Message> {
    banner(
        text(format!("An edit of {} wasn't kept", refused.name))
            .font(theme::SEMIBOLD)
            .wrapping(Wrapping::None)
            .style(theme::warning_text),
        &edit_refused(refused.refusal, refused.sketch),
        Some(dismiss(Edit::DismissRefusedEdit).into()),
    )
}

/// The banners under the toolbar about the open `file`, and that it's
/// read-only, for `read_only`'s reason, if it is.
pub(super) fn file_banners<'a>(
    file: &FileShown<'a>,
    read_only: Option<&'a str>,
) -> Element<'a, Message> {
    let read_only =
        read_only.map(|reason| banner(text("Read-only").font(theme::SEMIBOLD), reason, None));
    let save_error = file.save_error.as_deref().map(|error| {
        let save_as =
            small_button("Save As…", Emphasis::Primary).on_press(Message::File(File::SaveAs));
        banner(
            text("Couldn't save")
                .font(theme::SEMIBOLD)
                .style(theme::danger_text),
            error,
            Some(
                row![save_as, dismiss(Edit::DismissSaveError)]
                    .spacing(6)
                    .into(),
            ),
        )
    });

    let export_error = file.export_error.map(|error| {
        banner(
            text("Couldn't export")
                .font(theme::SEMIBOLD)
                .style(theme::danger_text),
            error,
            Some(dismiss(Edit::DismissExportError).into()),
        )
    });

    let recovered = file.recovered.as_ref().map(|recovered| {
        // Nothing to restore of what can't be read.
        let restore = (!recovered.unreadable).then(|| {
            small_button("Restore", Emphasis::Primary).on_press(Message::File(File::RestoreChanges))
        });
        let discard = small_button("Discard", Emphasis::Secondary)
            .on_press(Message::File(File::DiscardChanges));
        banner(
            text("Unsaved changes found").font(theme::SEMIBOLD),
            &recovered_detail(recovered),
            Some(row![restore, discard].spacing(6).into()),
        )
    });

    let damage = file.damage.as_ref().map(|damaged| {
        banner(
            text("Damaged file")
                .font(theme::SEMIBOLD)
                .style(theme::warning_text),
            &damage_detail(damaged),
            Some(dismiss(Edit::DismissDamage).into()),
        )
    });

    // On the panel's colour, the window's, rather than over whatever's
    // behind: a banner's own is translucent.
    container(column![
        read_only,
        damage,
        recovered,
        save_error,
        export_error
    ])
    .width(Length::Fill)
    .style(theme::toolbar)
    .into()
}

/// A banner's Dismiss button, sending `edit`.
fn dismiss(edit: Edit) -> Button<'static, Message> {
    small_button("Dismiss", Emphasis::Secondary).on_press(Message::Edit(edit))
}

/// Why the solver refused an edit of `sketch`, as the status bar and the
/// banner over the viewport say it.
pub(super) fn edit_refused(refusal: Refusal<'_>, sketch: &Sketch) -> Cow<'static, str> {
    match refusal {
        Refusal::Rejected(why) => refusal_text(why, sketch),
        Refusal::Failed(error) => format!("Couldn't check the edit: {error}").into(),
    }
}

/// Why an edit of `sketch` was refused, as the status bar and the
/// banner say it. A driving dimension refused is told how to have it as
/// a reference: a new one placed with the reference modifier held, one
/// made driving stays one.
fn refusal_text(why: &Rejected, sketch: &Sketch) -> Cow<'static, str> {
    match why {
        Rejected::Edit(why) => format!("Couldn't edit: {why}").into(),
        Rejected::Redundant { .. } => "Would over-constrain the sketch".into(),
        Rejected::Driving { dimensions, .. }
            if dimensions.iter().all(|&id| sketch.dimension(id).is_some()) =>
        {
            "Would over-constrain the sketch, so it stays a reference".into()
        }
        Rejected::Driving { .. } => format!(
            "Would over-constrain the sketch: place it with {} held to add it as a reference",
            Held::REFERENCE.label()
        )
        .into(),
        Rejected::Unsolved(Failure::NotConverged { .. }) => "Can't be solved with the rest".into(),
        Rejected::Unsolved(Failure::Degenerate { .. }) => "Would collapse the sketch".into(),
        Rejected::Unsolved(Failure::OutOfTime) => "Took too long to solve".into(),
    }
}

/// A strip under the toolbar, or over the viewport, telling something
/// about the whole document: `title`, then `detail`, then any `actions`
/// at the right.
fn banner<'a>(
    title: impl Into<Element<'a, Message>>,
    detail: &str,
    actions: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        container(
            // The detail takes what's left and wraps, so a long one
            // doesn't push the actions off a small window.
            row![
                title.into(),
                text(format!("— {detail}"))
                    .style(theme::muted_text)
                    .width(Length::Fill),
                actions,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([6, 12])
        .style(theme::banner),
        chrome::hrule(),
    ]
    .into()
}

/// What the banner offering the changes a crashed session left says of
/// them: that there are some, and whether the design changed since, or
/// they were made to a newer save of it than could be read, how damaged
/// the file holding them is, or that they can't be read.
fn recovered_detail(recovered: &RecoveredChanges) -> String {
    let found =
        format!("{APP_NAME} closed unexpectedly while this design had changes that weren't saved");
    if recovered.unreadable {
        return format!("{found}, but the auto-save is damaged and can't be read");
    }
    let base = if recovered.newer_base {
        ", auto-saved from a newer save than could be read"
    } else if recovered.design_changed {
        ", but the design has changed since: restoring them may undo newer changes"
    } else {
        ""
    };
    let damage = match &recovered.damage {
        None => String::new(),
        Some(Damage::Bridged) => "; some earlier auto-saves of them are damaged".to_owned(),
        Some(Damage::NewestDamaged { from }) => {
            format!("; the newest auto-save of them is damaged, so these are from {from}")
        }
        Some(Damage::Damaged { from }) => format!(
            "; the auto-save of them is damaged, so these are the newest that can be read, \
             from {from}"
        ),
    };
    format!("{found}{base}{damage}")
}

/// What the banner about a damaged file says: which save is open.
fn damage_detail(damaged: &DamagedFile) -> String {
    let save = if damaged.auto_saves {
        "auto-save"
    } else {
        "save"
    };
    match &damaged.damage {
        Damage::Bridged => format!("Some earlier {save}s in this file are damaged"),
        Damage::NewestDamaged { from } => {
            format!("The newest {save} in this file is damaged; opened the one from {from}")
        }
        Damage::Damaged { from } => format!("This file is damaged; opened the {save} from {from}"),
    }
}

/// Asks whether to save the changes to the document `name` before it's
/// closed, as a dialog over the whole screen, which dims the rest and
/// keeps it from being clicked.
pub(super) fn unsaved_prompt(name: &str) -> Element<'_, Message> {
    let choice = |label, choice, emphasis: Emphasis| {
        dialog_button(
            label,
            emphasis.button_style(),
            Some(Message::File(File::Unsaved(choice))),
        )
    };
    dialog(
        column![
            text(format!("Save the changes to {name}.{EXTENSION}?"))
                .size(14)
                .font(theme::SEMIBOLD),
            text("Your changes are lost if you don't save them.").style(theme::muted_text),
            Space::new().height(4),
            row![
                choice("Don't save", Unsaved::Discard, Emphasis::Secondary),
                space::horizontal(),
                choice("Cancel", Unsaved::Cancel, Emphasis::Secondary),
                choice("Save", Unsaved::Save, Emphasis::Primary),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
}

/// What confirming the name prompt does, its button's label and the
/// `Enter` key's, and whether it's blocked (it then does nothing):
/// renaming to a name taken, or keeping in browser storage, which keeps a
/// design by its name, with none typed.
pub(super) fn name_action(prompt: &NamePrompt<'_>) -> (&'static str, bool) {
    let browser = prompt.in_browser();
    let action = match (prompt.rename, browser, prompt.taken.is_some()) {
        (true, _, _) => "Rename",
        (false, false, _) => "Choose file…",
        (false, true, true) => "Replace",
        (false, true, false) => "Save",
    };
    let blocked =
        (prompt.rename && prompt.taken.is_some()) || (browser && prompt.name.trim().is_empty());
    (action, blocked)
}

/// The app's own Save As dialog, see [`NamePrompt`], as a dialog over the
/// whole screen like [`unsaved_prompt`]: the name, with `.vrdp` after it,
/// where to save it if there's a choice, and what's in the way if the
/// name's taken. `Enter` in the field saves, as the primary button does.
pub(super) fn name_prompt(prompt: NamePrompt<'_>) -> Element<'_, Message> {
    let browser = prompt.in_browser();
    let confirm = Message::File(File::ConfirmName);
    let field = text_input("Name", prompt.name)
        .id(crate::NAME_FIELD)
        .on_input(|name| Message::File(File::Name(name)))
        .on_submit(confirm.clone())
        .padding([6, 8])
        .style(theme::field_input(prompt.taken.is_some() && browser));
    let name = row![
        container(field).width(Length::Fill),
        text(format!(".{EXTENSION}")).style(theme::faint_text),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let places = (prompt.places && !prompt.rename).then(|| {
        let place = |label, place| {
            let chosen = prompt.place == place;
            crate::menu::choice_item(
                match place {
                    SavePlace::Browser => Icon::Browser,
                    SavePlace::Computer => Icon::Computer,
                },
                label,
                chosen,
                Message::File(File::Place(place)),
            )
        };
        column![
            place("Browser storage", SavePlace::Browser),
            place("A file on your computer…", SavePlace::Computer),
        ]
    });
    // Where it's going, unless it stays where it is.
    let place = (!prompt.rename).then(|| {
        text(if browser {
            "Saved in this browser's storage."
        } else {
            "Saved to a file on your computer: you choose where next."
        })
        .size(12)
        .style(theme::muted_text)
    });
    let note = match (prompt.taken, prompt.rename) {
        (Some(taken), true) if browser => Some(
            text(format!(
                "A design in browser storage is called {taken} already."
            ))
            .style(theme::danger_text),
        ),
        (Some(taken), false) if browser => Some(
            text(format!(
                "A design in browser storage is called {taken} already. Save again to replace it."
            ))
            .style(theme::warning_text),
        ),
        _ => None,
    };
    let title = if prompt.rename {
        "Rename design"
    } else {
        "Save design as"
    };
    let (action, blocked) = name_action(&prompt);
    dialog(
        column![
            text(title).size(14).font(theme::SEMIBOLD),
            name,
            places,
            place,
            note,
            Space::new().height(4),
            row![
                space::horizontal(),
                dialog_button(
                    "Cancel",
                    Emphasis::Secondary.button_style(),
                    Some(Message::File(File::CancelName)),
                ),
                dialog_button(
                    action,
                    Emphasis::Primary.button_style(),
                    (!blocked).then_some(confirm),
                ),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
}

/// Asks whether to delete what `prompt` lists, as a dialog over the
/// whole screen like [`unsaved_prompt`]: [`delete_question`], then the
/// features, in the Timeline's order, and the bodies, scrolling past
/// about ten rows, [`delete_warning`] if there's one, and Cancel and
/// Delete, or, if the features depending on it can stay, Cancel, Delete
/// only (it, keeping them) and Delete all.
pub(super) fn delete_prompt<'a>(prompt: &DeletePrompt<'a>) -> Element<'a, Message> {
    /// The rows shown before the list scrolls.
    const ROWS: f32 = 10.5;
    let question = delete_question(prompt);
    let warning = delete_warning(prompt).map(|warning| text(warning).style(theme::warning_text));
    let item = |icon, name: &'a str| {
        row![icons::icon(icon, icons::INLINE), text(name)]
            .spacing(8)
            .height(panels::ROW_HEIGHT)
            .align_y(Alignment::Center)
            .into()
    };
    let features = prompt
        .features
        .iter()
        .map(|feature| item(feature_icon(feature), feature.name.as_str()));
    let bodies = prompt
        .bodies
        .iter()
        .map(|body| item(Icon::Body, body.name.as_str()));
    let rows = prompt.features.len().saturating_add(prompt.bodies.len());
    // Counts are bounded by the document's, far below f32's exact range.
    let shown = (rows as f32).min(ROWS);
    let list = chrome::scrolled(column(features.chain(bodies)), 0.0)
        .height(shown * panels::ROW_HEIGHT)
        .width(Length::Fill);
    let cancel = dialog_button(
        "Cancel",
        theme::secondary_button,
        Some(Message::Look(Look::CancelDelete)),
    );
    let delete = dialog_button(
        if prompt.keeping {
            "Delete all"
        } else {
            "Delete"
        },
        theme::danger_button,
        Some(Message::Edit(Edit::ConfirmDelete)),
    );
    let only = prompt.keeping.then(|| {
        dialog_button(
            "Delete only",
            theme::secondary_button,
            Some(Message::Edit(Edit::ConfirmDeleteOnly)),
        )
    });
    let note = prompt.keeping.then(|| {
        text(delete_only_note(prompt))
            .size(12)
            .style(theme::muted_text)
    });
    dialog(
        column![
            text(question).size(14).font(theme::SEMIBOLD),
            list,
            warning,
            note,
            Space::new().height(4),
            row![space::horizontal(), cancel, only, delete].spacing(8),
        ]
        .spacing(8),
    )
}

/// What the delete prompt asks, counting what goes besides what was
/// asked to be deleted: "Delete Sketch 1 with the 1 feature and 1 body
/// that depend on it?", or for a body, "Delete Body 1 with the 2
/// features that go with it?", counting the one making it; a body going
/// with only that one names it: "Delete Body 1 and Extrude 1, which
/// makes it?".
fn delete_question(prompt: &DeletePrompt<'_>) -> String {
    if prompt.body
        && let ([maker], [_]) = (&prompt.features[..], &prompt.bodies[..])
    {
        return format!("Delete {} and {}, which makes it?", prompt.name, maker.name);
    }
    // A body asked for is one of the bodies listed, a feature one of the
    // features.
    let (features, bodies) = if prompt.body {
        (prompt.features.len(), prompt.bodies.len().saturating_sub(1))
    } else {
        (prompt.features.len().saturating_sub(1), prompt.bodies.len())
    };
    let parts = counts([
        (features, "feature", "features"),
        (bodies, "body", "bodies"),
    ]);
    if parts.is_empty() {
        return format!("Delete {}?", prompt.name);
    }
    let one = features.saturating_add(bodies) == 1;
    let tie = match (prompt.body, one) {
        (true, true) => "goes with",
        (true, false) => "go with",
        (false, true) => "depends on",
        (false, false) => "depend on",
    };
    format!(
        "Delete {} with the {} that {tie} it?",
        prompt.name,
        parts.join(" and ")
    )
}

/// What Delete only does, under the list: "Delete only keeps the
/// features that depend on Sketch 1; they fail until they're given
/// another." A body goes with the feature making it either way.
fn delete_only_note(prompt: &DeletePrompt<'_>) -> String {
    let what = match (prompt.body, prompt.features.first()) {
        (true, Some(maker)) => maker.name.as_str(),
        _ => prompt.name,
    };
    format!(
        "Delete only keeps the features that depend on {what}; they fail until they're given \
         another."
    )
}

/// What the delete prompt warns of, if a join, cut or intersect that
/// stays worked only on bodies that go: "Extrude 2 works on Body 1 and
/// stays, so it may fail with nothing to work on."
fn delete_warning(prompt: &DeletePrompt<'_>) -> Option<String> {
    if prompt.worked.is_empty() {
        return None;
    }
    let features = listed(prompt.worked.iter().map(|feature| feature.name.as_str()));
    let bodies = listed(prompt.worked_on.iter().map(|body| body.name.as_str()));
    let (works, stays, it) = if prompt.worked.len() == 1 {
        ("works", "stays", "it")
    } else {
        ("work", "stay", "they")
    };
    Some(format!(
        "{features} {works} on {bodies} and {stays}, so {it} may fail with nothing to work on."
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use glam::DVec2;
    use varde_document::{Body, Feature};
    use varde_sketch::{Curve, Measure, Rejected, Sketch};

    use super::{Held, delete_question, delete_warning, refusal_text};
    use crate::document::DeletePrompt;

    #[test]
    fn the_delete_prompt_counts_the_features_and_the_bodies() {
        fn prompt<'a>(
            name: &'a str,
            body: bool,
            features: Vec<&'a Feature>,
            bodies: Vec<&'a Body>,
        ) -> DeletePrompt<'a> {
            DeletePrompt {
                name,
                body,
                features,
                bodies,
                worked: Vec::new(),
                worked_on: Vec::new(),
                keeping: false,
            }
        }
        let document = varde_document::Document::example();
        let [sketch, extrude] = [0, 1].map(|k| &document.features()[k]);
        let body = &document.bodies()[0];
        assert_eq!(
            delete_question(&prompt(
                "Sketch 1",
                false,
                vec![sketch, extrude],
                vec![body]
            )),
            "Delete Sketch 1 with the 1 feature and 1 body that depend on it?"
        );
        assert_eq!(
            delete_question(&prompt("Extrude 1", false, vec![extrude], vec![body])),
            "Delete Extrude 1 with the 1 body that depends on it?"
        );
        assert_eq!(
            delete_question(&prompt(
                "Sketch 1",
                false,
                vec![sketch, extrude, extrude],
                vec![]
            )),
            "Delete Sketch 1 with the 2 features that depend on it?"
        );
        assert_eq!(
            delete_question(&prompt("Sketch 1", false, vec![sketch], vec![])),
            "Delete Sketch 1?"
        );
        // A body goes with the feature making it, named if it's the only
        // one, else counted, and the other bodies that one makes.
        assert_eq!(
            delete_question(&prompt("Body 1", true, vec![extrude], vec![body])),
            "Delete Body 1 and Extrude 1, which makes it?"
        );
        assert_eq!(
            delete_question(&prompt("Body 1", true, vec![extrude, extrude], vec![body])),
            "Delete Body 1 with the 2 features that go with it?"
        );
        assert_eq!(
            delete_question(&prompt(
                "Body 1",
                true,
                vec![extrude, extrude],
                vec![body, body]
            )),
            "Delete Body 1 with the 2 features and 1 body that go with it?"
        );
    }

    #[test]
    fn the_delete_prompt_warns_of_the_joins_and_cuts_that_stay() {
        let document = varde_document::Document::example();
        let extrude = document.features()[1].clone();
        let body = document.bodies()[0].clone();
        let named = |name: &str| varde_document::Feature {
            name: name.to_owned(),
            ..extrude.clone()
        };
        let [cut, join, other] = ["Extrude 2", "Extrude 3", "Extrude 4"].map(named);
        let second = varde_document::Body {
            name: "Body 2".to_owned(),
            ..body.clone()
        };
        let prompt = |worked, worked_on| DeletePrompt {
            name: "Body 1",
            body: true,
            features: vec![&extrude],
            bodies: vec![&body],
            worked,
            worked_on,
            keeping: false,
        };
        assert_eq!(delete_warning(&prompt(vec![], vec![])), None);
        assert_eq!(
            delete_warning(&prompt(vec![&cut], vec![&body])).as_deref(),
            Some("Extrude 2 works on Body 1 and stays, so it may fail with nothing to work on.")
        );
        assert_eq!(
            delete_warning(&prompt(vec![&cut, &join], vec![&body, &second])).as_deref(),
            Some(
                "Extrude 2 and Extrude 3 work on Body 1 and Body 2 and stay, so they may fail \
                 with nothing to work on."
            )
        );
        assert_eq!(
            delete_warning(&prompt(vec![&cut, &join, &other], vec![&body])).as_deref(),
            Some(
                "Extrude 2, Extrude 3 and Extrude 4 work on Body 1 and stay, so they may fail \
                 with nothing to work on."
            )
        );
    }

    #[test]
    fn a_refusal_says_why() {
        let mut sketch = Sketch::default();
        let point = sketch.add_point(DVec2::ZERO).unwrap();
        let why = Rejected::Redundant {
            involved: BTreeSet::new(),
        };
        assert_eq!(
            refusal_text(&why, &sketch),
            "Would over-constrain the sketch"
        );
        // A driving dimension refused says how to have it as a reference:
        // a new one placed so, one made driving staying one.
        let refused = |dimension| Rejected::Driving {
            dimensions: BTreeSet::from([dimension]),
            involved: BTreeSet::from([dimension]),
        };
        let new = refusal_text(&refused(point), &sketch);
        let held = Held::REFERENCE.label();
        assert!(new.contains(&format!("place it with {held} held")), "{new}");
        let b = sketch.add_point(DVec2::X).unwrap();
        let line = sketch
            .add_curve(
                Curve::Line {
                    start: point,
                    end: b,
                },
                false,
            )
            .unwrap();
        let measure = Measure::Length(line);
        let placed = crate::testing::dimension(&mut sketch, measure, "1", false, DVec2::ZERO);
        let made_driving = refusal_text(&refused(placed), &sketch);
        assert_eq!(
            made_driving,
            "Would over-constrain the sketch, so it stays a reference"
        );
    }
}
