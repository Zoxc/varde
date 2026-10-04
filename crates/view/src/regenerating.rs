//! The card floating at the top of the viewport, under the toolbar, while
//! a regeneration is slow: the feature it's working on, or drawing the
//! model, and how many of its steps are done, as a bar.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, progress_bar, row, space, text};
use iced::{Alignment, Element, Length, Padding};
use varde_regen::{Progress, Stage};

use crate::Message;
use crate::theme::{self, SEMIBOLD};

/// How wide the card is, in pixels.
const WIDTH: f32 = 280.0;
/// How far the card floats in from the viewport's top, in pixels.
const TOP: f32 = 10.0;

/// The card's layer: centred at the top of the viewport it's put over,
/// telling of `progress`, if the lane has said how far it has got. It
/// takes nothing, so the viewport under it works as usual.
pub fn regenerating(progress: Option<&Progress>) -> Element<'_, Message> {
    let doing = progress.map(|progress| match &progress.stage {
        Stage::Feature(name) => name.as_str(),
        Stage::Drawing => "Drawing the model",
    });
    let count = progress.map(|progress| {
        text(format!(
            "{} of {}",
            progress.step.saturating_add(1),
            progress.steps
        ))
        .size(12)
        .style(theme::muted_text)
    });
    let head = row![
        text("Regenerating").font(SEMIBOLD).wrapping(Wrapping::None),
        doing.map(|doing| {
            text(doing)
                .style(theme::muted_text)
                .wrapping(Wrapping::None)
        }),
        space::horizontal(),
        count,
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    // Nothing done until the lane says.
    let done = progress.map_or(0.0, |progress| progress.step as f32);
    let steps = progress.map_or(1.0, |progress| progress.steps.max(1) as f32);
    let bar = progress_bar(0.0..=steps, done)
        .girth(4)
        .style(theme::regenerating_bar);
    let card = container(column![head, bar].spacing(8))
        .padding(Padding::from([8, 12]))
        .width(WIDTH)
        .clip(true)
        .style(theme::regenerating_card);
    container(card)
        .center_x(Length::Fill)
        .padding(Padding::ZERO.top(TOP))
        .into()
}
