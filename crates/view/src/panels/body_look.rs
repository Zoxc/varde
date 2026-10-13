//! The look part of a body's menu in the Objects tab: its opacity and
//! colour.

use iced::widget::{Row, Space, column, container, mouse_area, row, slider, space, stack, text};
use iced::{Alignment, Element, Length};
use varde_document::{BodyId, Opacity, Tint};
use varde_render::BodyTint;

use crate::chrome::{self, icon_button};
use crate::icons::Icon;
use crate::mouse_only::MouseOnly;
use crate::theme::{self, Tone};
use crate::{Edit, Look, Message};

/// How far the Opacity slider moves at a time, in percent.
pub(super) const OPACITY_STEP: f32 = 1.0;

/// The Opacity and Colour part of a body's context menu, `parts`, under
/// one wash while `hovered`. Entering and leaving it tell the app, which
/// leaves the selection out of the viewport meanwhile, so the body shows
/// as it's drawn.
pub(super) fn look_area<'a>(
    parts: Vec<Element<'a, Message>>,
    hovered: bool,
) -> Element<'a, Message> {
    let wash = if hovered {
        theme::hovered_row
    } else {
        |_: &iced::Theme| container::Style::default()
    };
    mouse_area(
        container(column(parts).spacing(4).padding([2, 0]))
            .width(Length::Fill)
            .style(wash),
    )
    .on_enter(Message::Look(Look::HoverBodyLook(true)))
    .on_exit(Message::Look(Look::HoverBodyLook(false)))
    .into()
}

/// A part of a body's context menu showing how it's drawn: a heading of
/// `title`, then `extra` and, rightmost, a reset icon sending `reset` if
/// it's not as it would be by default (inert if the document isn't
/// `editable`), over `rows`.
pub(super) fn look_part<'a>(
    title: &'static str,
    extra: Option<Element<'a, Message>>,
    reset: Option<Message>,
    editable: bool,
    rows: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let reset = reset.map(|reset| -> Element<'a, Message> {
        icon_button(Icon::Reset, Tone::Faint, editable.then_some(reset)).into()
    });
    let heading = row![chrome::heading(title), space::horizontal()]
        .push(extra)
        .push(reset)
        .spacing(8)
        .height(24)
        .padding([0, 8])
        .align_y(Alignment::Center);
    column![heading].extend(rows).into()
}

/// A slider line of a body's context menu: `slider`, the full width but
/// for `value` beside it.
pub(super) fn slider_line<'a>(
    slider: iced::widget::Slider<'a, f32, Message>,
    value: String,
    editable: bool,
) -> Element<'a, Message> {
    let slider = slider
        .height(2.0 * theme::SLIDER_HANDLE_RADIUS)
        .style(theme::slider(editable));
    value_line(MouseOnly::new(slider).into(), value)
}

/// A colour slider line of a body's context menu: `slider` over a rail
/// of the colours of `stops` (see [`theme::color_rail`]), its handle
/// filled with `handle`, `value` beside it.
pub(super) fn color_line<'a>(
    slider: iced::widget::Slider<'a, f32, Message>,
    stops: Vec<Option<BodyTint>>,
    handle: Option<BodyTint>,
    value: String,
    editable: bool,
) -> Element<'a, Message> {
    let slider = slider
        .height(2.0 * theme::SLIDER_HANDLE_RADIUS)
        .style(theme::color_slider(editable, handle));
    // The handle's middle runs a radius in from either end, and the rail's
    // colours with it.
    let border = container(Space::new())
        .width(Length::Fill)
        .height(theme::COLOR_RAIL_HEIGHT)
        .style(theme::color_rail(stops.clone(), editable));
    // iced draws no gradients on the web: solid strips under the border.
    let border: Element<'a, Message> = if cfg!(target_arch = "wasm32") {
        let strips = (0..theme::COLOR_RAIL_STRIPS).map(|index| {
            container(Space::new())
                .width(Length::Fill)
                .height(theme::COLOR_RAIL_HEIGHT)
                .style(theme::color_rail_strip(stops.clone(), index, editable))
                .into()
        });
        stack![Row::with_children(strips), border].into()
    } else {
        border.into()
    };
    let rail = container(border)
        .padding([0.0, theme::SLIDER_HANDLE_RADIUS])
        .center_y(Length::Fill);
    let slider = stack![rail, MouseOnly::new(slider)].height(2.0 * theme::SLIDER_HANDLE_RADIUS);
    value_line(slider.into(), value)
}

/// A slider line of a body's context menu: `slider`, `value` beside it.
pub(super) fn value_line<'a>(slider: Element<'a, Message>, value: String) -> Element<'a, Message> {
    row![
        slider,
        text(value)
            .width(OPACITY_VALUE_WIDTH)
            .align_x(Alignment::End)
            .style(theme::muted_text),
    ]
    .spacing(10)
    .height(28)
    .padding([0, 8])
    .align_y(Alignment::Center)
    .into()
}

/// The Opacity rows of `body`'s context menu ([`look_part`]): a slider
/// from [`Opacity::MIN`] to [`Opacity::MAX`] showing `opacity`, the
/// percentage beside it. Dragging previews, letting go commits; it takes
/// only the mouse ([`MouseOnly`]). Unless the document is `editable`, it's
/// faded and the app ignores it. While the body isn't opaque, the reset
/// icon makes it so.
pub(super) fn opacity_rows<'a>(
    body: BodyId,
    opacity: Opacity,
    editable: bool,
) -> Element<'a, Message> {
    let percent = |opacity: Opacity| f32::from(opacity.percent());
    let slider = slider(
        percent(Opacity::MIN)..=percent(Opacity::MAX),
        percent(opacity),
        move |percent| Message::Look(Look::PreviewOpacity(body, Opacity::clamped(percent))),
    )
    .step(OPACITY_STEP)
    .on_release(Message::Edit(Edit::CommitOpacity));
    look_part(
        "Opacity",
        None,
        (!opacity.is_opaque()).then_some(Message::Edit(Edit::ResetOpacity(body))),
        editable,
        vec![slider_line(slider, opacity.to_string(), editable)],
    )
}

/// The Colour rows of `body`'s context menu ([`look_part`]): a swatch of
/// `color` as the model is drawn in it in the heading, "Default" beside
/// it while the body has none of its own (the theme's model colour),
/// over a Hue slider (0 to 359°) and a Saturation one (0 to
/// [`Tint::MAX_SATURATION`] %), each with its value beside it, a dash
/// for the default. They work as the Opacity slider does: dragging
/// previews, letting go commits, the mouse only. Dragging the hue of a
/// body with no colour of its own, or none saturated, starts it at
/// [`Tint::DEFAULT_SATURATION`], so it shows. While the body has a
/// colour, the reset icon gives it the theme's back.
pub(super) fn color_rows(
    body: BodyId,
    color: Option<Tint>,
    editable: bool,
) -> Element<'static, Message> {
    // With no colour of its own, the saturation shows the default, where
    // dragging the hue starts, so its handle doesn't jump.
    let (hue, saturation) = color.map_or((0, Tint::DEFAULT_SATURATION), |tint| {
        (tint.hue(), tint.saturation())
    });
    let preview = move |tint| Message::Look(Look::PreviewColor(body, tint));
    let hue_slider = slider(0.0..=359.0, f32::from(hue), move |hue| {
        let saturation = match saturation {
            0 => Tint::DEFAULT_SATURATION,
            saturation => saturation,
        };
        preview(Tint::clamped(hue, f32::from(saturation)))
    })
    .on_release(Message::Edit(Edit::CommitColor));
    let max = f32::from(Tint::MAX_SATURATION);
    let saturation_slider = slider(0.0..=max, f32::from(saturation), move |saturation| {
        preview(Tint::clamped(f32::from(hue), saturation))
    })
    .on_release(Message::Edit(Edit::CommitColor));
    // The hue's rail runs round the wheel at the most saturation, whatever
    // the saturation, the saturation's from grey to the most at the hue.
    let tint = |hue: u16, saturation: u8| {
        Some(crate::document::body_tint(
            Tint::new(hue, saturation).expect("a hue below 360, a saturation in range"),
        ))
    };
    let hue_stops = (0..=6)
        .map(|sixth: u16| tint((sixth * 60).min(359), Tint::MAX_SATURATION))
        .collect();
    let saturation_stops = vec![tint(hue, 0), tint(hue, Tint::MAX_SATURATION)];
    let handle = color.map(crate::document::body_tint);
    let value = |shown: String| {
        if color.is_some() {
            shown
        } else {
            "–".to_owned()
        }
    };
    let swatch = container(Space::new())
        .width(COLOR_SWATCH_WIDTH)
        .height(COLOR_SWATCH_HEIGHT)
        .style(theme::body_swatch(color.map(crate::document::body_tint)));
    let default = color
        .is_none()
        .then(|| text("Default").size(11.5).style(theme::faint_text));
    let extra = row![]
        .push(default)
        .push(swatch)
        .spacing(6)
        .align_y(Alignment::Center);
    look_part(
        "Colour",
        Some(extra.into()),
        color
            .is_some()
            .then_some(Message::Edit(Edit::ResetColor(body))),
        editable,
        vec![
            color_line(
                hue_slider,
                hue_stops,
                handle,
                value(format!("{hue}°")),
                editable,
            ),
            color_line(
                saturation_slider,
                saturation_stops,
                handle,
                value(format!("{saturation} %")),
                editable,
            ),
        ],
    )
}

/// The size of the swatch in the Colour heading.
pub(super) const COLOR_SWATCH_WIDTH: f32 = 36.0;
pub(super) const COLOR_SWATCH_HEIGHT: f32 = 14.0;

/// The room the Opacity slider's percentage takes, "100 %" at the widest.
pub(super) const OPACITY_VALUE_WIDTH: f32 = 40.0;
