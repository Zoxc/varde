//! The floating panel an operation is set up in, over the right of the
//! viewport: a header with its title and a summary, a body with its
//! options, and a footer with its message and Cancel and OK. The header
//! and footer always show; the body scrolls when the panel would run past
//! the room it has, so OK and Cancel stay on screen however many options
//! there are or however short the window is.
//!
//! The extrude (`extrude::panel`) and the revolve (`revolve::panel`) are
//! set up in it, from the parts here they share: what a session hands the
//! view of the sketches whose regions it picks, the choices, ticks and
//! typed fields, the Bodies list of a join, cut or intersect, and the
//! footer's message. The other operations are to set themselves up in it
//! too.

use std::sync::Arc;

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::text::Wrapping;
use iced::widget::{button, checkbox, column, container, opaque, row, space, text, text_input};
use iced::{Alignment, Element, Event, Length, Rectangle, Size, Vector};
use varde_document::{BodyId, FeatureId, Plane};
use varde_sketch::{Profiles, Sketch};

use crate::Message;
use crate::chrome::{heading, hrule, scrolled, sentence, small_button};
use crate::controls::CONTROLS_HEIGHT;
use crate::escape::OnEscape;
use crate::status::STATUS_BAR_ROOM;
use crate::theme::{self, Emphasis, SEMIBOLD};
use crate::viewport::CONTROLS_TOP;

/// How wide the panel is, in pixels.
pub(crate) const PANEL_WIDTH: f32 = 264.0;

/// How far below the viewport's top the panel starts, clear of the
/// camera controls (the view cube and Home), in pixels.
pub(crate) const PANEL_TOP: f32 = CONTROLS_TOP + CONTROLS_HEIGHT + PANEL_MARGIN;

/// How far from the viewport's right the panel stays at least, and from
/// its top where it rises over the controls, in pixels.
pub(crate) const PANEL_MARGIN: f32 = 12.0;

/// How far from the viewport's bottom the panel stays at least: clear of
/// the floating status bar, in pixels.
pub(crate) const PANEL_BOTTOM: f32 = STATUS_BAR_ROOM + PANEL_MARGIN;

/// How tall the panel may get below its top before it rises above
/// [`PANEL_TOP`], in pixels: its header and footer and a few rows of its
/// body. A shorter viewport lifts the panel over the camera controls
/// rather than squeeze its body to nothing or its buttons away.
const PANEL_ROOM: f32 = 200.0;

/// The scrollable holding the panel's body.
pub const PANEL_BODY: iced::widget::Id = iced::widget::Id::new("operation-panel-body");

/// How tall the footer's message gets at most, in pixels: about five
/// lines. A longer one scrolls, so it can't push OK off the panel and
/// none of it is lost.
const MESSAGE_HEIGHT: f32 = 80.0;

/// The horizontal padding of the panel's sections, in pixels. The
/// scrollbars of the body and the message float in its right padding,
/// clear of the text.
const SIDE: f32 = 10.0;

/// How far in from the panel's side a typed value's field starts: its
/// label's width and the gap after it. Why its text is refused shows
/// under it, as far in.
pub(crate) const FIELD_INDENT: f32 = 68.0;

/// The gap between a typed value's label and its field.
const FIELD_GAP: f32 = 6.0;

/// A sketch whose regions can be picked, and where they are.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub feature: FeatureId,
    pub plane: Plane,
    /// The sketch, whose lines a revolve's axis is picked from.
    pub sketch: &'a Sketch,
    pub profiles: &'a Arc<Profiles>,
}

/// A typed value's field, a distance or an angle: its text, and why it's
/// refused, if it is.
#[derive(Debug, Clone, Copy)]
pub struct TypedField<'a> {
    pub text: &'a str,
    pub error: Option<&'a varde_expr::Error>,
    /// The last value it gave, in model units (millimetres or radians),
    /// which the preview and an extrude's handle show while the text is
    /// refused.
    pub value: Option<f64>,
}

/// What an extrude or a revolve does with its solid, see
/// `varde_document::Operation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperationKind {
    #[default]
    NewBody,
    Join,
    Cut,
    Intersect,
}

impl OperationKind {
    pub const ALL: [OperationKind; 4] = [
        OperationKind::NewBody,
        OperationKind::Join,
        OperationKind::Cut,
        OperationKind::Intersect,
    ];

    /// The kind of `operation`.
    pub fn of(operation: &varde_document::Operation) -> Self {
        use varde_document::Operation;
        match operation {
            Operation::NewBody(_) => OperationKind::NewBody,
            Operation::Join(_) => OperationKind::Join,
            Operation::Cut(_) => OperationKind::Cut,
            Operation::Intersect(_) => OperationKind::Intersect,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            OperationKind::NewBody => "New body",
            OperationKind::Join => "Join",
            OperationKind::Cut => "Cut",
            OperationKind::Intersect => "Intersect",
        }
    }

    /// Whether it works on bodies already there, which the panel then
    /// lists.
    pub fn has_targets(self) -> bool {
        self != OperationKind::NewBody
    }
}

/// A body a join, cut or intersect touches, or one taken out of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyTarget<'a> {
    pub body: BodyId,
    pub name: &'a str,
    /// Whether it's worked on: not taken out.
    pub included: bool,
    /// The name of the body an earlier join merged it into, if one did:
    /// it's listed only while it's taken out (or just put back), which
    /// does nothing then, so that can be seen and undone.
    pub holder: Option<&'a str>,
}

/// What an operation's panel shows.
pub(crate) struct Parts<'a> {
    /// The operation's name, or the feature edited: one line, clipped.
    pub title: &'a str,
    /// A short note right of the title, such as what's picked.
    pub summary: Option<Element<'a, Message>>,
    /// The options, scrolled when they don't fit.
    pub body: Element<'a, Message>,
    /// Above the buttons: why OK can't be pressed, say. It scrolls past
    /// about five lines.
    pub message: Option<Element<'a, Message>>,
    /// What OK sends, or nothing while it can't be pressed.
    pub ok: Option<Message>,
    /// What Cancel sends.
    pub cancel: Message,
}

/// The panel showing `parts`. It's `opaque`: clicks and the wheel over it
/// don't reach the scene under it.
pub(crate) fn operation_panel(parts: Parts<'_>) -> Element<'_, Message> {
    let Parts {
        title,
        summary,
        body,
        message,
        ok,
        cancel,
    } = parts;
    let title = container(text(title).size(13).font(SEMIBOLD).wrapping(Wrapping::None))
        .width(Length::Fill)
        .clip(true);
    let header = column![
        container(row![title, summary].spacing(8).align_y(Alignment::Center)).padding([9.0, SIDE]),
        hrule(),
    ];
    // Both scrollbars float in the right padding, clear of the text.
    let margin = (SIDE - theme::SCROLLBAR_WIDTH) / 2.0;
    let body = scrolled(
        container(body)
            .width(Length::Fill)
            .padding(iced::Padding::from([8.0, SIDE]).bottom(10.0)),
        margin,
    )
    .id(PANEL_BODY)
    .width(Length::Fill);
    let message = message.map(|message| {
        container(
            scrolled(
                container(message).width(Length::Fill).padding([0.0, SIDE]),
                margin,
            )
            .width(Length::Fill),
        )
        .max_height(MESSAGE_HEIGHT)
    });
    let buttons = row![
        space::horizontal(),
        small_button("Cancel", Emphasis::Secondary).on_press(cancel),
        small_button("OK", Emphasis::Primary).on_press_maybe(ok),
    ]
    .spacing(6);
    let footer = column![
        hrule(),
        column![message, container(buttons).padding([0.0, SIDE])]
            .spacing(6)
            .padding(iced::Padding::from([8.0, 0.0]).bottom(10.0)),
    ];
    let sections = Sections {
        width: PANEL_WIDTH,
        parts: [header.into(), body.into(), footer.into()],
    };
    opaque(container(sections).style(theme::operation_panel).clip(true))
}

/// `panel` placed over a viewport: at its right, [`PANEL_MARGIN`] in from
/// its right and [`PANEL_BOTTOM`] up from its bottom, and [`PANEL_TOP`]
/// down from its top, or higher, down to [`PANEL_MARGIN`], where the
/// viewport is too short to leave it [`PANEL_ROOM`] below that. The layer takes only what's over the panel
/// and lets the rest through.
pub(crate) fn placed(panel: Element<'_, Message>) -> Element<'_, Message> {
    Element::new(Placed { panel })
}

/// See [`placed`].
struct Placed<'a> {
    panel: Element<'a, Message>,
}

impl Placed<'_> {
    /// How far below the top of a viewport `height` tall the panel starts.
    fn top(height: f32) -> f32 {
        if !height.is_finite() {
            return PANEL_TOP;
        }
        (height - PANEL_BOTTOM - PANEL_ROOM).clamp(PANEL_MARGIN, PANEL_TOP)
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for Placed<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.panel)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.panel));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);
        let top = Self::top(size.height);
        let room = Size::new(
            (size.width - 2.0 * PANEL_MARGIN).max(0.0),
            (size.height - top - PANEL_BOTTOM).max(0.0),
        );
        let panel = self.panel.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, room),
        );
        let x = (size.width - PANEL_MARGIN - panel.size().width).max(0.0);
        layout::Node::with_children(size, vec![panel.move_to((x, top))])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let panel = layout.children().next().expect("the panel's layout");
        self.panel
            .as_widget_mut()
            .operate(&mut tree.children[0], panel, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let panel = layout.children().next().expect("the panel's layout");
        self.panel.as_widget_mut().update(
            &mut tree.children[0],
            event,
            panel,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let panel = layout.children().next().expect("the panel's layout");
        self.panel.as_widget().mouse_interaction(
            &tree.children[0],
            panel,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let panel = layout.children().next().expect("the panel's layout");
        self.panel.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            panel,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let panel = layout.children().next().expect("the panel's layout");
        self.panel.as_widget_mut().overlay(
            &mut tree.children[0],
            panel,
            renderer,
            viewport,
            translation,
        )
    }
}

/// Four choices in two rows of two.
pub(crate) fn grid<'a>(choices: [Element<'a, Message>; 4]) -> Element<'a, Message> {
    let [a, b, c, d] = choices;
    column![row![a, b].spacing(4), row![c, d].spacing(4)]
        .spacing(4)
        .into()
}

/// A choice of the panel's, highlighted while `on`, sending `message`, or
/// disabled without one.
pub(crate) fn choice<'a>(
    label: &'a str,
    on: bool,
    message: Option<Message>,
) -> Element<'a, Message> {
    let font = if on { SEMIBOLD } else { iced::Font::DEFAULT };
    button(
        text(label)
            .size(12)
            .font(font)
            .width(Length::Fill)
            .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([3, 6])
    .style(theme::choice(on))
    .on_press_maybe(message)
    .into()
}

/// A checkbox of the panel's, ticked while `on`, sending `message` when
/// clicked, or disabled without one.
pub(crate) fn tick<'a>(label: &'a str, on: bool, message: Option<Message>) -> Element<'a, Message> {
    checkbox(on)
        .label(label)
        .size(15)
        .spacing(7)
        .text_size(12)
        // A name with no spaces breaks where the panel ends.
        .text_wrapping(Wrapping::WordOrGlyph)
        .style(theme::tick)
        .on_toggle_maybe(message.map(|message| move |_| message.clone()))
        .into()
}

/// A labelled row of the panel: `label` as wide as a typed value's,
/// then `content`.
pub(crate) fn labelled<'a>(
    label: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let label = text(label).size(12).width(FIELD_INDENT - FIELD_GAP);
    row![label, content.into()]
        .spacing(FIELD_GAP)
        .align_y(Alignment::Center)
        .into()
}

/// A typed value's field, named `label`, with the id `id`, showing why its
/// text is refused under it. It sends `input` of the text typed, if the
/// document can be changed; `Enter` in it sends `submit` (OK), `Esc`
/// `cancel`.
pub(crate) fn value_field<'a>(
    label: &'a str,
    id: iced::widget::Id,
    field: TypedField<'a>,
    input: Option<impl Fn(String) -> Message + 'a>,
    submit: Message,
    cancel: Message,
) -> Element<'a, Message> {
    let field_input = text_input(label, field.text)
        .id(id)
        .size(12)
        .padding([2, 4])
        .width(Length::Fill);
    let field_input = match input {
        Some(input) => field_input.on_input(input).on_submit(submit),
        None => field_input,
    };
    let field_input = OnEscape::new(field_input, cancel);
    let error = field.error.map(|error| {
        container(
            text(sentence(&error.to_string()).into_owned())
                .size(11.5)
                .wrapping(Wrapping::WordOrGlyph)
                .style(theme::danger_text),
        )
        .padding(iced::Padding::ZERO.left(FIELD_INDENT))
    });
    column![labelled(label, field_input), error]
        .spacing(2)
        .into()
}

/// The Bodies list of a join, cut or intersect: a checkbox per body of
/// `targets`, each sending `toggle` of it if the document can be
/// changed, a body an earlier join merged into another with "in Body 1"
/// after it, and for a join ticked for two or more, which body it
/// merges them into. None for a new body, or with nothing to list.
pub(crate) fn bodies<'a>(
    operation: OperationKind,
    targets: &[BodyTarget<'a>],
    toggle: impl Fn(BodyId) -> Option<Message>,
) -> Option<Element<'a, Message>> {
    if !operation.has_targets() || targets.is_empty() {
        return None;
    }
    let rows = targets.iter().map(|&target| {
        let tick = tick(target.name, target.included, toggle(target.body));
        match target.holder {
            // Faint, as the Objects list notes a merged body.
            Some(holder) => row![
                tick,
                space::horizontal(),
                text(format!("in {holder}"))
                    .size(12)
                    .wrapping(Wrapping::None)
                    .style(theme::faint_text),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            None => tick,
        }
    });
    let merging = joined_into(operation, targets).map(|holder| {
        // The mock's panel note: faint, 12 px.
        text(format!("Joined into {holder}"))
            .size(12)
            .wrapping(Wrapping::WordOrGlyph)
            .style(theme::faint_text)
    });
    Some(
        column![heading("Bodies"), column(rows).spacing(4), merging]
            .spacing(6)
            .into(),
    )
}

/// The body a join merges the bodies it's ticked for into, if it's
/// ticked for two or more of `targets`: the first made of them, which
/// then holds them all. A body merged away before isn't one of them.
pub(crate) fn joined_into<'a>(
    operation: OperationKind,
    targets: &[BodyTarget<'a>],
) -> Option<&'a str> {
    if operation != OperationKind::Join {
        return None;
    }
    let mut included = (targets.iter()).filter(|target| target.included && target.holder.is_none());
    let first = included.next()?;
    included.next().map(|_| first.name)
}

/// The footer's message: why OK can't be pressed (`refused`, by the
/// operation's own check), else why the preview failed (`error`), else,
/// if `checking`, that OK waits on the solver.
pub(crate) fn footer_message<'a>(
    refused: Option<String>,
    error: Option<&'a str>,
    checking: bool,
) -> Option<Element<'a, Message>> {
    match (refused, error) {
        (Some(refused), _) => Some(message_text(
            sentence(&refused).into_owned(),
            theme::danger_text,
        )),
        (None, Some(error)) => Some(message_text(sentence(error), theme::danger_text)),
        (None, None) => checking.then(|| message_text("Checking the sketch…", theme::muted_text)),
    }
}

/// The text of a message in a panel's footer, in `style`, broken within
/// words where they don't fit, as a message may quote a name.
pub(crate) fn message_text<'a>(
    message: impl text::IntoFragment<'a>,
    style: fn(&iced::Theme) -> text::Style,
) -> Element<'a, Message> {
    text(message)
        .size(12)
        .wrapping(Wrapping::WordOrGlyph)
        .style(style)
        .into()
}

/// A header, a body and a footer, one above the other, `width` wide: the
/// header and footer as tall as they are, the body in what's left of the
/// height the panel may take, at most as tall as it is.
///
/// A column can't do this: it lays its children out in order, so the
/// footer would get only what the body leaves, and a body filling the
/// rest would make the panel always as tall as it may be.
struct Sections<'a> {
    width: f32,
    parts: [Element<'a, Message>; 3],
}

impl Widget<Message, iced::Theme, iced::Renderer> for Sections<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.width), Length::Shrink)
    }

    fn children(&self) -> Vec<Tree> {
        self.parts.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.parts);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(self.width).height(Length::Shrink);
        let max = limits.max();
        let [header, body, footer] = &mut self.parts;
        let [header_tree, body_tree, footer_tree] = &mut tree.children[..] else {
            unreachable!("three parts have three trees");
        };
        let within = |height: f32| layout::Limits::new(Size::ZERO, Size::new(max.width, height));
        // The footer first: where even the header and footer don't fit,
        // the buttons keep their height and the title gives way.
        let footer = footer
            .as_widget_mut()
            .layout(footer_tree, renderer, &within(max.height));
        let left = (max.height - footer.size().height).max(0.0);
        let header = header
            .as_widget_mut()
            .layout(header_tree, renderer, &within(left));
        let left = (left - header.size().height).max(0.0);
        let body = body
            .as_widget_mut()
            .layout(body_tree, renderer, &within(left));
        let body_top = header.size().height;
        let footer_top = body_top + body.size().height;
        let height = footer_top + footer.size().height;
        let size = limits.resolve(
            Length::Fixed(self.width),
            Length::Shrink,
            Size::new(self.width, height),
        );
        layout::Node::with_children(
            size,
            vec![
                header,
                body.move_to((0.0, body_top)),
                footer.move_to((0.0, footer_top)),
            ],
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((part, tree), layout) in self
                .parts
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                part.as_widget_mut()
                    .operate(tree, layout, renderer, operation);
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for ((part, tree), layout) in self
            .parts
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            part.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.parts
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((part, tree), layout)| {
                part.as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for ((part, tree), layout) in self.parts.iter().zip(&tree.children).zip(layout.children()) {
            part.as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        overlay::from_children(
            &mut self.parts,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<Sections<'a>> for Element<'a, Message> {
    fn from(sections: Sections<'a>) -> Self {
        Element::new(sections)
    }
}

#[cfg(test)]
mod tests;
