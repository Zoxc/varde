//! The design's parameters: a popup listing them (the model mock's
//! `.pcard`), styled as an operation's panel but wider, hidden until the
//! Modify set's Parameters tool opens it. At the viewport's right under
//! the view cube, or left of an operation's panel while one is open.
//!
//! A table: a header row, then a banded row per parameter, its name and
//! expression to edit, its value, what uses it, and delete on hover; a
//! bad one says why under it, its expression boxed in red, and one the
//! feature selected uses is marked down its left edge. "+ Parameter" adds
//! one under them. What's typed is the app's draft until it's committed
//! (`Enter`, or acting elsewhere), and stays in the field with why if the
//! document refuses it.

use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{Space, button, column, container, hover, opaque, row, stack, text, text_input};
use iced::{Alignment, Element, Length, Padding};
use varde_document::{Document, FeatureId, ParamUses};

use crate::chrome::{hrule, scrolled, sentence};
use crate::escape::OnEscape;
use crate::icons::{self, Icon};
use crate::operation_panel::{
    CLOSE_BUTTON, PANEL_MARGIN, PANEL_WIDTH, Sections, head_button, placed_from,
};
use crate::theme::{self, SEMIBOLD};
use crate::{Edit, Look, Message};

/// How wide the popup is, in pixels, as the mock's: narrower only where
/// the viewport is.
pub(crate) const PARAMS_WIDTH: f32 = 540.0;

/// The columns' widths: the name, the value, what uses it and the delete
/// button; the expression takes what's left.
const NAME_WIDTH: f32 = 110.0;
const VALUE_WIDTH: f32 = 64.0;
const USES_WIDTH: f32 = 110.0;
const DELETE_WIDTH: f32 = 26.0;
/// The gap between the columns.
const COLUMN_GAP: f32 = 11.0;
/// A row's padding: 4 px above and below, 12 px left, 8 px right.
const ROW_PADDING: Padding = Padding {
    top: 4.0,
    right: 8.0,
    bottom: 4.0,
    left: 12.0,
};
/// How wide the mark down a row's left edge is.
const MARK_WIDTH: f32 = 3.0;
/// The size of the table's words.
const TEXT: f32 = 13.0;

/// One of a parameter's two fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamField {
    Name,
    Expression,
}

/// What's typed in a parameter's field and not in the document: being
/// typed, or refused, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDraft {
    /// The parameter's index in the document's list.
    pub index: usize,
    pub field: ParamField,
    pub text: String,
    /// Why the document refused it, once it has.
    pub error: Option<String>,
}

/// A change to the popup, see [`Look::Params`]: nothing in the document
/// changes until [`Edit::Param`].
#[derive(Debug, Clone, PartialEq)]
pub enum ParamsLook {
    /// Text typed in a parameter's field.
    Input {
        index: usize,
        field: ParamField,
        text: String,
    },
    /// Puts a field back as the document has it, dropping its draft:
    /// `Esc` in it.
    Revert { index: usize, field: ParamField },
}

/// A change to the design's parameters, see [`Edit::Param`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamEdit {
    /// Applies the field's draft, if it has one: `Enter` in it.
    Commit { index: usize, field: ParamField },
    /// Adds a parameter, `param1` up, of 10 mm, its name taking the
    /// focus: the "+ Parameter" row.
    Add,
    /// Removes the parameter, refused while something uses it.
    Remove(usize),
}

/// The popup as the app hands it to the view.
#[derive(Debug, Clone, Copy)]
pub struct ParamsState<'a> {
    pub document: &'a Document,
    /// The drafts of the fields, see [`ParamDraft`].
    pub drafts: &'a [ParamDraft],
    /// A parameter whose removal was refused, and why: said under its row.
    pub refused: Option<(usize, &'a str)>,
    /// What uses each parameter, by index:
    /// [`Document::all_param_uses`], which the app keeps for the
    /// document's generation rather than finding it every frame.
    pub uses: &'a [ParamUses],
    /// The feature selected in the Timeline, whose parameters are marked.
    pub selected: Option<FeatureId>,
    /// Whether the document can be changed.
    pub editable: bool,
}

/// A row as shown: what the fields hold, its value, what uses it and why
/// it's wrong, if it is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub(crate) name: String,
    pub(crate) text: String,
    /// The value in the design's units, or a dash.
    pub(crate) value: String,
    /// The features and other parameters using it, or "Not used".
    pub(crate) uses: String,
    pub(crate) error: Option<String>,
    /// Whether the feature selected uses it.
    pub(crate) lit: bool,
    /// Whether the expression's field is what's wrong.
    pub(crate) bad_text: bool,
}

impl<'a> ParamsState<'a> {
    fn draft(&self, index: usize, field: ParamField) -> Option<&'a ParamDraft> {
        (self.drafts.iter()).find(|draft| draft.index == index && draft.field == field)
    }

    /// Each parameter's row, in order.
    pub(crate) fn rows(&self) -> Vec<Row> {
        let document = self.document;
        let resolved = document.params_resolved();
        let units = document.units();
        (document.params().iter().enumerate())
            .map(|(index, param)| {
                let name = self.draft(index, ParamField::Name);
                let expression = self.draft(index, ParamField::Expression);
                let result = resolved.result(index);
                let value = match result {
                    Some(Ok(value)) => varde_expr::format(value.value, value.quantity.unit(units)),
                    _ => "\u{2014}".to_owned(),
                };
                let (features, users) = self.uses.get(index).map_or((&[][..], &[][..]), |uses| {
                    (uses.features.as_slice(), uses.params.as_slice())
                });
                let lit = self.selected.is_some_and(|id| features.contains(&id));
                let uses: Vec<&str> = (features.iter())
                    .filter_map(|&id| document.feature(id).map(|f| f.name.as_str()))
                    .chain(
                        (users.iter())
                            .filter_map(|&at| document.params().get(at))
                            .map(|user| user.name.as_str()),
                    )
                    .collect();
                let expression_error =
                    (expression.and_then(|draft| draft.error.clone())).or_else(|| match result {
                        Some(Err(why)) => Some(why.to_string()),
                        _ => None,
                    });
                let error = (name.and_then(|draft| draft.error.clone()))
                    .or_else(|| {
                        (self.refused)
                            .filter(|(at, _)| *at == index)
                            .map(|(_, why)| why.to_owned())
                    })
                    .or(expression_error.clone());
                Row {
                    name: name.map_or_else(|| param.name.clone(), |draft| draft.text.clone()),
                    text: expression.map_or_else(|| param.text.clone(), |draft| draft.text.clone()),
                    value,
                    uses: if uses.is_empty() {
                        "Not used".to_owned()
                    } else {
                        uses.join(", ")
                    },
                    error: error.map(|why| sentence(&why).into_owned()),
                    lit,
                    bad_text: expression_error.is_some(),
                }
            })
            .collect()
    }
}

/// The widget id of the name field of the parameter at `index`, which
/// takes the focus as it's added.
pub fn param_name_id(index: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("param-name-{index}"))
}

/// The popup showing `state`, placed over the viewport: left of an
/// operation's panel if `beside` one.
pub(crate) fn popup<'a>(state: ParamsState<'a>, beside: bool) -> Element<'a, Message> {
    let right = if beside {
        PANEL_MARGIN + PANEL_WIDTH + PANEL_MARGIN
    } else {
        PANEL_MARGIN
    };
    placed_from(card(state), right)
}

/// The popup's card, see [`popup`]. It's opaque: clicks and the wheel
/// over it don't reach the scene under it.
pub(crate) fn card<'a>(state: ParamsState<'a>) -> Element<'a, Message> {
    let rows = state.rows();
    let bad = rows.iter().any(|row| row.error.is_some());
    let count = row![
        text(rows.len().to_string())
            .size(12.5)
            .style(theme::muted_text),
        bad.then(|| icons::tinted(Icon::Alert, 14.0, |p| p.danger)),
    ]
    .spacing(3)
    .align_y(Alignment::Center);
    let close = head_button(
        Icon::Cancel,
        CLOSE_BUTTON,
        false,
        Some(Message::Look(Look::ToggleParams)),
        "Close",
    );
    let header = container(
        row![
            icons::icon(Icon::Params, icons::INLINE),
            text("Parameters").size(13).font(SEMIBOLD),
            container(count).width(Length::Fill),
            close,
        ]
        .spacing(7)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([7.0, 6.0]).left(9.0));
    let add = button(
        row![
            icons::tinted(Icon::Plus, 15.0, |p| p.accent),
            text("Parameter").size(TEXT).font(SEMIBOLD),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([10, 12])
    .style(theme::param_add)
    .on_press_maybe(
        state
            .editable
            .then_some(Message::Edit(Edit::Param(ParamEdit::Add))),
    );
    let list: Element<'a, Message> = if rows.is_empty() {
        column![
            container(
                text("No parameters yet.\nAdd one, then type its name in any length or angle.")
                    .size(12.5)
                    .line_height(1.5)
                    .style(theme::muted_text),
            )
            .padding([14, 12]),
            add,
        ]
        .into()
    } else {
        let head = container(
            row![
                head_label("Name", Length::Fixed(NAME_WIDTH)),
                head_label("Expression", Length::Fill),
                head_label("Value", Length::Fixed(VALUE_WIDTH)),
                head_label("Used by", Length::Fixed(USES_WIDTH)),
                Space::new().width(DELETE_WIDTH),
            ]
            .spacing(COLUMN_GAP),
        )
        .width(Length::Fill)
        .padding(ROW_PADDING.top(8.0).bottom(8.0))
        .style(theme::param_head);
        let rows =
            (rows.into_iter().enumerate()).map(|(index, shown)| table_row(&state, index, shown));
        column![head, hrule()]
            .extend(rows)
            .push(hrule())
            .push(add)
            .into()
    };
    let body = scrolled(list, 2.0).width(Length::Fill);
    let sections = Sections {
        width: PARAMS_WIDTH,
        parts: [
            header.into(),
            column![hrule(), body].into(),
            Space::new().into(),
        ],
        well: false,
    };
    opaque(sections)
}

/// A column's heading in the header row.
fn head_label(label: &str, width: Length) -> Element<'_, Message> {
    container(text(label).size(12).font(SEMIBOLD).wrapping(Wrapping::None))
        .width(width)
        .clip(true)
        .into()
}

/// The row of the parameter at `index`, banded every other one, see the
/// module's docs.
fn table_row<'a>(state: &ParamsState<'a>, index: usize, shown: Row) -> Element<'a, Message> {
    let editable = state.editable;
    let input = move |field: ParamField| {
        move |text: String| Message::Look(Look::Params(ParamsLook::Input { index, field, text }))
    };
    let commit = |field| Message::Edit(Edit::Param(ParamEdit::Commit { index, field }));
    let revert = |field| Message::Look(Look::Params(ParamsLook::Revert { index, field }));
    let field = |value: &str, field: ParamField| {
        let input_field = text_input(
            match field {
                ParamField::Name => "Name",
                ParamField::Expression => "Expression",
            },
            value,
        )
        .size(TEXT)
        .line_height(LineHeight::Absolute(16.0.into()))
        .padding([5, 8])
        .width(Length::Fill);
        let input_field = match field {
            ParamField::Name => input_field
                .id(param_name_id(index))
                .font(SEMIBOLD)
                .style(theme::param_name_input),
            ParamField::Expression => input_field.style(theme::field_input(shown.bad_text)),
        };
        let input_field = if editable {
            input_field.on_input(input(field)).on_submit(commit(field))
        } else {
            input_field
        };
        OnEscape::new(input_field, revert(field))
    };
    let main = row![
        container(field(&shown.name, ParamField::Name)).width(NAME_WIDTH),
        container(field(&shown.text, ParamField::Expression)).width(Length::Fill),
        container(
            text(shown.value)
                .size(12.5)
                .wrapping(Wrapping::None)
                .style(theme::muted_text)
        )
        .width(VALUE_WIDTH)
        .clip(true),
        container(
            text(shown.uses)
                .size(12)
                .wrapping(Wrapping::None)
                .style(theme::muted_text)
        )
        .width(USES_WIDTH)
        .clip(true),
        Space::new().width(DELETE_WIDTH),
    ]
    .spacing(COLUMN_GAP)
    .align_y(Alignment::Center);
    // Delete shows over the row's last column while it's hovered.
    let delete =
        button(container(icons::tinted(Icon::Close, 14.0, |p| p.faint)).center(Length::Fill))
            .width(24)
            .height(24)
            .padding(0)
            .style(theme::param_delete)
            .on_press_maybe(
                editable.then_some(Message::Edit(Edit::Param(ParamEdit::Remove(index)))),
            );
    let main = hover(
        main,
        container(delete)
            .align_right(Length::Fill)
            .center_y(Length::Fill),
    );
    let note = shown.error.as_ref().map(|why| {
        row![
            icons::tinted(Icon::Alert, 14.0, |p| p.danger),
            text(why.clone())
                .size(12)
                .wrapping(Wrapping::WordOrGlyph)
                .style(theme::danger_text),
        ]
        .spacing(5)
        .align_y(Alignment::Center)
        .padding(Padding::ZERO.top(3.0).left(2.0))
    });
    let content = container(column![main, note])
        .width(Length::Fill)
        .padding(ROW_PADDING)
        .style(theme::param_row(index % 2 == 1));
    // The mark down its left edge, over the band, as the mock's inset
    // shadow.
    let mark = (shown.error.is_some() || shown.lit).then(|| {
        Element::from(
            container(Space::new())
                .width(MARK_WIDTH)
                .height(Length::Fill)
                .style(theme::param_mark(shown.error.is_some())),
        )
    });
    stack![content].extend(mark).into()
}

#[cfg(test)]
mod tests;
