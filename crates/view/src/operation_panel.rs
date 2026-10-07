//! The floating panel an operation is set up in, over the right of the
//! viewport, as the tool rail's cards are (the UI mock's `.opp`): an
//! accent line along its top, a head with the operation's icon and title
//! and its Cancel and OK as small square buttons, and a recessed well
//! under it holding the body with the options and, at its foot, the
//! message (why OK waits, or the draft failing, with Add anyway). The
//! head and the foot always show; the body scrolls when the panel would
//! run past the room it has, so OK and Cancel stay on screen however many
//! options there are or however short the window is.
//!
//! The extrude (`extrude::panel`), the revolve (`revolve::panel`) and the
//! combine (`combine::panel`) are set up in it, and the measure tool
//! shows its values in it (`measure::panel`, with only Close), from the
//! parts here they share: what a session hands the view of the sketches
//! whose regions it picks, the labels, choices as tiles, options as icon
//! toggles, fields picked into by clicks in the viewport, typed fields,
//! the Bodies list of a join, cut or intersect, and the foot's message.
//! Its controls are the Timeline's rows' size: 28 px tall, 12.5 px words,
//! 8 px in.

use std::borrow::Cow;
use std::sync::Arc;

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
    Space, Text, button, checkbox, column, container, hover, mouse_area, opaque, row, text,
    text_input,
};
use iced::{Alignment, Border, Color, Element, Event, Length, Padding, Rectangle, Size, Vector};
use varde_document::{BodyId, FeatureId, Placement};
use varde_sketch::{Profiles, Sketch};

use crate::chrome::{hrule, scrolled, sentence, tip};
use crate::controls::CONTROLS_HEIGHT;
use crate::escape::OnEscape;
use crate::icons::{self, Icon};
use crate::status::STATUS_BAR_ROOM;
use crate::theme::{self, BOLD, SEMIBOLD};
use crate::viewport::CONTROLS_TOP;
use crate::{Look, Message};

/// How wide the panel is, in pixels.
pub(crate) const PANEL_WIDTH: f32 = 288.0;

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
/// [`PANEL_TOP`], in pixels: its head and foot and a few rows of its
/// body. A shorter viewport lifts the panel over the camera controls
/// rather than squeeze its body to nothing or its buttons away.
const PANEL_ROOM: f32 = 200.0;

/// The scrollable holding the panel's body.
pub const PANEL_BODY: iced::widget::Id = iced::widget::Id::new("operation-panel-body");

/// The head's buttons, which have no words of their own: what each is
/// called, which the tests' probe reports as their text.
pub(crate) const OK_BUTTON: iced::widget::Id = iced::widget::Id::new("OK");
pub(crate) const CANCEL_BUTTON: iced::widget::Id = iced::widget::Id::new("Cancel");
pub(crate) const CLOSE_BUTTON: iced::widget::Id = iced::widget::Id::new("Close");

/// The head's buttons and what each is called, see [`OK_BUTTON`].
#[cfg(any(test, feature = "probe"))]
pub(crate) const BUTTON_NAMES: [(iced::widget::Id, &str); 3] = [
    (OK_BUTTON, "OK"),
    (CANCEL_BUTTON, "Cancel"),
    (CLOSE_BUTTON, "Close"),
];

/// How tall a message at the foot gets at most, in pixels: about five
/// lines. A longer one scrolls, so it can't push the body away and none
/// of it is lost.
const MESSAGE_HEIGHT: f32 = 80.0;

/// The horizontal padding of the body and the foot, in pixels. The
/// scrollbars of the body and the message float in its right padding,
/// clear of the text.
const SIDE: f32 = 9.0;

/// How thick the accent line along the card's top is, in pixels.
const ACCENT_LINE: f32 = 3.0;

/// The corner radius of the card, and of the well's top corners.
const CARD_RADIUS: f32 = 8.0;
const WELL_RADIUS: f32 = 6.0;

/// How tall the panel's controls are, as the Timeline's rows, and the
/// size of their words.
pub(crate) const CONTROL_HEIGHT: f32 = 28.0;
pub(crate) const CONTROL_TEXT: f32 = 12.5;

/// How wide the measure tool's labels are, left of their values.
pub(crate) const FIELD_INDENT: f32 = 68.0;

/// A sketch whose regions can be picked, and where they are.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub feature: FeatureId,
    /// Where its plane is: a sketch that isn't placed isn't a candidate.
    pub placement: Placement,
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
    /// The design's parameters, whose names are offered as they're typed
    /// (see the `suggest` module).
    pub params: crate::ParamsIn<'a>,
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

    /// Its choice's icon: the booleans as circles.
    pub(crate) fn icon(self) -> Icon {
        match self {
            OperationKind::NewBody => Icon::BoNew,
            OperationKind::Join => Icon::BoJoin,
            OperationKind::Cut => Icon::BoCut,
            OperationKind::Intersect => Icon::BoInt,
        }
    }

    /// Whether it works on bodies already there, which the panel then
    /// lists.
    pub fn has_targets(self) -> bool {
        self != OperationKind::NewBody
    }
}

/// What the cursor is over in an operation's panel, for the viewport to
/// light up too: a picked region's row, a revolve's axis's, or a body's (a
/// combine's target or tool, or one a join, cut or intersect touches).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelHover {
    Region {
        sketch: FeatureId,
        region: usize,
    },
    Axis,
    Body(BodyId),
    /// A chamfer's edge, by its place in the list.
    Edge(usize),
    /// A shell's face, by its place in the list.
    Face(usize),
    /// A sweep's path part of a sketch's curves, by its place among
    /// them; a loft's rail, by its place among them.
    Part(usize),
    /// A loft's section, by its place among them.
    Section(usize),
}

impl PanelHover {
    /// The region, and its sketch, if it's one.
    pub fn region(self) -> Option<(FeatureId, usize)> {
        match self {
            PanelHover::Region { sketch, region } => Some((sketch, region)),
            PanelHover::Axis
            | PanelHover::Body(_)
            | PanelHover::Edge(_)
            | PanelHover::Face(_)
            | PanelHover::Part(_)
            | PanelHover::Section(_) => None,
        }
    }

    /// The body, if it's one.
    pub fn body(self) -> Option<BodyId> {
        match self {
            PanelHover::Body(body) => Some(body),
            PanelHover::Region { .. }
            | PanelHover::Axis
            | PanelHover::Edge(_)
            | PanelHover::Face(_)
            | PanelHover::Part(_)
            | PanelHover::Section(_) => None,
        }
    }
}

/// The messages telling the app `hover` is entered, and left.
fn hovering(hover: PanelHover) -> (Message, Message) {
    (
        Message::Look(Look::HoverPanel(Some(hover))),
        Message::Look(Look::LeavePanel(hover)),
    )
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
    /// The operation's icon, before its title.
    pub icon: Icon,
    /// The operation's name, or the feature edited: one line, clipped.
    pub title: &'a str,
    /// The options, scrolled when they don't fit.
    pub body: Element<'a, Message>,
    /// At the well's foot: why OK can't be pressed, say, or the draft
    /// failing.
    pub message: Option<Footer<'a>>,
    /// What OK sends, or nothing while it can't be pressed.
    pub ok: Option<Message>,
    /// What Cancel sends.
    pub cancel: Message,
    /// Whether the head has only a Close button, sending `cancel`, in
    /// place of Cancel and OK: for a tool that changes nothing, as the
    /// measure tool.
    pub close: bool,
}

/// The message at the panel's foot.
pub(crate) enum Footer<'a> {
    /// Words (see [`message_text`]), scrolled past about five lines.
    Text(Element<'a, Message>),
    /// The draft failing: "Extrude fails" (`noun`) over why, with
    /// `show`'s button, if its geometry has a box, and Add anyway sending
    /// `accept`, if it can be pressed, which keeps the operation with its
    /// error (marked failed in the Timeline, to fix later). OK waits
    /// meanwhile.
    Fails {
        noun: &'a str,
        error: Cow<'a, str>,
        show: Option<Framing>,
        accept: Option<Message>,
    },
}

/// The button left of Add anyway framing the camera on where a draft
/// fails: Show, which turns into Go back once pressed, turning the camera
/// back to where it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// Show, sending [`Look::ShowFailure`].
    Show,
    /// Go back, sending [`Look::BackFromFailure`].
    GoBack,
}

impl Framing {
    /// The button's icon, words and what it sends.
    fn button(self) -> (Icon, &'static str, Message) {
        match self {
            Framing::Show => (Icon::Locate, "Show", Message::Look(Look::ShowFailure)),
            Framing::GoBack => (Icon::Back, "Go back", Message::Look(Look::BackFromFailure)),
        }
    }
}

/// The panel showing `parts`. It's `opaque`: clicks and the wheel over it
/// don't reach the scene under it.
pub(crate) fn operation_panel(parts: Parts<'_>) -> Element<'_, Message> {
    let Parts {
        icon,
        title,
        body,
        message,
        ok,
        cancel,
        close,
    } = parts;
    let title = container(text(title).size(13).font(SEMIBOLD).wrapping(Wrapping::None))
        .width(Length::Fill)
        .clip(true);
    let buttons: Element<'_, Message> = if close {
        head_button(
            Icon::Cancel,
            CLOSE_BUTTON,
            false,
            Some(cancel),
            "Close (Esc)",
        )
    } else {
        row![
            head_button(
                Icon::Cancel,
                CANCEL_BUTTON,
                false,
                Some(cancel),
                "Cancel (Esc)"
            ),
            head_button(Icon::Confirm, OK_BUTTON, true, ok, "OK (Enter)"),
        ]
        .spacing(6)
        .into()
    };
    let header = container(
        row![icons::icon(icon, icons::INLINE), title, buttons]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([7.0, 6.0]).left(8.0));
    // Both scrollbars float in the right padding, clear of the text.
    let margin = (SIDE - theme::SCROLLBAR_WIDTH) / 2.0;
    let body = scrolled(
        container(body)
            .width(Length::Fill)
            .padding(Padding::from([6.0, SIDE]).bottom(10.0)),
        margin,
    )
    .id(PANEL_BODY)
    .width(Length::Fill);
    let footer: Element<'_, Message> = match message {
        None => Space::new().into(),
        Some(Footer::Text(message)) => container(
            container(
                scrolled(
                    container(message).width(Length::Fill).padding([0.0, SIDE]),
                    margin,
                )
                .width(Length::Fill),
            )
            .max_height(MESSAGE_HEIGHT),
        )
        .padding(Padding::ZERO.top(2.0).bottom(8.0))
        .into(),
        Some(Footer::Fails {
            noun,
            error,
            show,
            accept,
        }) => container(fail_box(noun, error, show, accept))
            .padding(Padding::from([0.0, SIDE]).top(2.0).bottom(8.0))
            .into(),
    };
    let sections = Sections {
        width: PANEL_WIDTH,
        parts: [header.into(), body.into(), footer],
        well: true,
    };
    opaque(sections)
}

/// A square icon button of the head, `icon` in it, with the id `id`
/// naming it, sending `message`, or disabled without one; `primary` is
/// OK's look. Hovering it tells `tip`.
pub(crate) fn head_button<'a>(
    icon: Icon,
    id: iced::widget::Id,
    primary: bool,
    message: Option<Message>,
    tip_text: &'a str,
) -> Element<'a, Message> {
    let enabled = message.is_some();
    // 16 px in the 26 px square sits on whole pixels: at 15 the centring
    // puts it half a pixel off, which blurs the cross's middle wide.
    let glyph: Element<'a, Message> = Element::from(icons::tinted(icon, 16.0, move |p| {
        match (primary, enabled) {
            (true, _) => theme::Emphasis::Primary.content(p),
            (false, true) => p.muted,
            (false, false) => p.faint,
        }
    }));
    let button = button(container(glyph).center(Length::Fill))
        .width(26)
        .height(26)
        .padding(0)
        .style(theme::head_button(primary))
        .on_press_maybe(message);
    container(tip(button, text(tip_text))).id(id).into()
}

/// The box of a draft that fails, at the foot: a title, "Extrude fails"
/// (`noun`), with the alert in the danger colour on its wash, `error`
/// under it in muted words, scrolled past about five lines, and under
/// that `show`'s button at the left, if there's geometry to frame, and
/// Add anyway at the right sending `accept`, if it can be pressed.
fn fail_box<'a>(
    noun: &'a str,
    error: Cow<'a, str>,
    show: Option<Framing>,
    accept: Option<Message>,
) -> Element<'a, Message> {
    let title = container(
        row![
            icons::tinted(Icon::Alert, 14.0, |p| p.danger),
            container(
                text(format!("{noun} fails"))
                    .size(CONTROL_TEXT)
                    .font(SEMIBOLD)
                    .wrapping(Wrapping::WordOrGlyph),
            )
            .width(Length::Fill),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([5, 8])
    .style(theme::fail_title);
    let words = container(
        scrolled(
            container(
                text(error)
                    .size(CONTROL_TEXT)
                    .wrapping(Wrapping::WordOrGlyph)
                    .style(theme::muted_text),
            )
            .width(Length::Fill)
            .padding(Padding::from([5.0, 8.0]).bottom(7.0)),
            2.0,
        )
        .width(Length::Fill),
    )
    .max_height(MESSAGE_HEIGHT);
    let show = show.map(|show| {
        let (icon, words, message) = show.button();
        let content = row![
            icons::tinted(icon, 14.0, |p| p.text),
            text(words).size(CONTROL_TEXT).font(SEMIBOLD),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        fail_button(content.into(), false, message)
    });
    let add = accept.map(|accept| {
        fail_button(
            text("Add anyway").size(CONTROL_TEXT).font(SEMIBOLD).into(),
            true,
            accept,
        )
    });
    let buttons = (show.is_some() || add.is_some()).then(|| {
        container(row![show, Space::new().width(Length::Fill), add].spacing(6))
            .width(Length::Fill)
            .padding(Padding::from([0.0, 8.0]).bottom(8.0))
    });
    container(column![title, words, buttons])
        .width(Length::Fill)
        .style(theme::fail_box)
        .into()
}

/// A button in a [`fail_box`] showing `content`, its words in the danger
/// colour if `danger`, sending `message`.
fn fail_button(
    content: Element<'_, Message>,
    danger: bool,
    message: Message,
) -> Element<'_, Message> {
    button(content)
        .padding([5, 12])
        .style(theme::fail_button(danger))
        .on_press(message)
        .into()
}

/// `panel` placed over a viewport: at its right, [`PANEL_MARGIN`] in from
/// its right and [`PANEL_BOTTOM`] up from its bottom, and [`PANEL_TOP`]
/// down from its top, or higher, down to [`PANEL_MARGIN`], where the
/// viewport is too short to leave it [`PANEL_ROOM`] below that. The layer takes only what's over the panel
/// and lets the rest through.
pub(crate) fn placed(panel: Element<'_, Message>) -> Element<'_, Message> {
    placed_from(panel, PANEL_MARGIN)
}

/// `panel` placed as [`placed`] places one, but `right` in from the
/// viewport's right: the parameters' popup, left of an operation's
/// panel. It keeps [`PANEL_MARGIN`] from the viewport's left, and gets
/// narrower where it has to.
pub(crate) fn placed_from(panel: Element<'_, Message>, right: f32) -> Element<'_, Message> {
    Element::new(Placed { panel, right })
}

/// See [`placed`].
struct Placed<'a> {
    panel: Element<'a, Message>,
    /// How far in from the viewport's right the panel's right edge is.
    right: f32,
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
            (size.width - self.right - PANEL_MARGIN).max(0.0),
            (size.height - top - PANEL_BOTTOM).max(0.0),
        );
        let panel = self.panel.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, room),
        );
        let x = (size.width - self.right - panel.size().width).max(0.0);
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

/// A label of the panel's, a field's or a section's alike: 11 px, bold,
/// faint.
pub(crate) fn label(label: &str) -> Text<'_> {
    text(label).size(11).font(BOLD).style(theme::faint_text)
}

/// `content` under its `label`: a field, or a section.
pub(crate) fn field<'a>(
    label_text: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![label(label_text), content.into()].spacing(3).into()
}

/// Choices among a few, as tiles side by side, sharing the width.
pub(crate) fn tiles<'a>(
    tiles: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    row(tiles).spacing(4).into()
}

/// A choice of the panel's as a tile, `icon` over `label`, picked while
/// `on`, sending `message`, or disabled without one.
pub(crate) fn tile<'a>(
    icon: Icon,
    label: &'a str,
    on: bool,
    message: Option<Message>,
) -> Element<'a, Message> {
    let glyph: Element<'a, Message> = if message.is_some() {
        icons::icon(icon, 22.0)
    } else {
        icons::tinted(icon, 22.0, |p| p.faint).into()
    };
    button(
        column![
            glyph,
            text(label)
                .size(10.5)
                .line_height(LineHeight::Relative(1.15))
                .wrapping(Wrapping::WordOrGlyph)
                .align_x(Alignment::Center)
                .width(Length::Fill),
        ]
        .spacing(3)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding::from([6.0, 2.0]).bottom(4.0))
    .style(theme::tile(on))
    .on_press_maybe(message)
    .into()
}

/// An option of the panel's as an icon toggle: a square button with the
/// option's `icon` beside its `label`, lit while `on`, the row and the
/// button tinted on hover, sending `message`, or disabled without one.
/// Hovering it tells `note`, if there is one.
pub(crate) fn toggle<'a>(
    icon: Icon,
    label: &'a str,
    on: bool,
    message: Option<Message>,
    note: Option<&'a str>,
) -> Element<'a, Message> {
    let enabled = message.is_some();
    let square = move |hovered: bool| -> Element<'a, Message> {
        let glyph: Element<'a, Message> =
            Element::from(icons::tinted(icon, 18.0, move |p| {
                match (enabled, on, hovered) {
                    (false, ..) => p.faint,
                    (true, true, _) => p.accent,
                    (true, false, true) => p.text,
                    (true, false, false) => p.muted,
                }
            }));
        container(glyph)
            .center(CONTROL_HEIGHT)
            .style(theme::toggle_square(on, hovered))
            .into()
    };
    let padding = Padding::from([3.0, 4.0]);
    let base = button(
        row![square(false), text(label).size(CONTROL_TEXT)]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(padding)
    .style(theme::toggle_row)
    .on_press_maybe(message);
    // Hovered, the square is drawn again over itself, lit: the label
    // isn't, so it shows once.
    let toggle: Element<'a, Message> = if enabled {
        hover(base, container(square(true)).padding(padding))
    } else {
        base.into()
    };
    match note {
        Some(note) => tip(toggle, text(note)),
        None => toggle,
    }
}

/// A field picked into by clicks in the viewport: what's picked as
/// `rows` ([`picked_row`]), then where to click for more, `place`, if
/// there is one, under a rule (or alone, the field's one row). Outlined
/// in the accent while it's the one picking (`on`); a click on it makes
/// it so, sending `message`.
pub(crate) fn pick_field<'a>(
    rows: Vec<Element<'a, Message>>,
    place: Option<String>,
    on: bool,
    message: Option<Message>,
) -> Element<'a, Message> {
    let alone = rows.is_empty();
    let place = place.map(|place| {
        let line = button(
            row![
                // In the column of the rows' icons, so its words start
                // where their names do.
                container(icons::tinted(Icon::Plus, 13.0, move |p| if on {
                    p.accent
                } else {
                    p.muted
                }))
                .center_x(icons::INLINE),
                text(place)
                    .size(11.5)
                    .wrapping(Wrapping::WordOrGlyph)
                    .style(move |theme: &iced::Theme| text::Style {
                        color: on.then_some(theme::palette(theme).accent),
                    }),
            ]
            .spacing(8)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(if alone {
            CONTROL_HEIGHT - 2.0
        } else {
            CONTROL_HEIGHT - 4.0
        })
        // The rows' icons sit 2 + 6 in from the box's inside.
        .padding([0, 8])
        .style(theme::place_line(alone))
        .on_press_maybe(message.clone());
        if alone {
            Element::from(line)
        } else {
            column![hrule(), line].into()
        }
    });
    let rows = (!alone).then(|| container(column(rows)).padding(2));
    let field = container(column![rows, place])
        .width(Length::Fill)
        .padding(1)
        .style(theme::pick_box(on));
    match message {
        Some(message) => mouse_area(field)
            .on_press(message)
            .interaction(mouse::Interaction::Pointer)
            .into(),
        None => field.into(),
    }
}

/// A row of what's picked in a [`pick_field`], as the Timeline's: `icon`,
/// `name`, `meta` at the right, and a cross taking it out sending
/// `remove`, if it can be. A click on the row sends `press`. It's
/// `what` to the viewport, which lights it up while the row is hovered,
/// and shows hovered while `hovered` is it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn picked_row<'a>(
    icon: Icon,
    name: impl text::IntoFragment<'a>,
    meta: Option<String>,
    remove: Option<Message>,
    press: Option<Message>,
    what: PanelHover,
    hovered: Option<PanelHover>,
) -> Element<'a, Message> {
    ordered_row(icon, name, meta, None, remove, press, what, hovered)
}

/// A [`picked_row`] of a list whose order counts, a loft's sections:
/// with `moves`, an up and a down chevron before its cross, each sending
/// its message, or shown faint without one (the first row's up, the
/// last's down).
#[allow(clippy::too_many_arguments)]
pub(crate) fn ordered_row<'a>(
    icon: Icon,
    name: impl text::IntoFragment<'a>,
    meta: Option<String>,
    moves: Option<[Option<Message>; 2]>,
    remove: Option<Message>,
    press: Option<Message>,
    what: PanelHover,
    hovered: Option<PanelHover>,
) -> Element<'a, Message> {
    let chevron = |glyph: Icon, message: Option<Message>| {
        let enabled = message.is_some();
        let tint = icons::tinted(
            glyph,
            14.0,
            move |p| if enabled { p.muted } else { p.faint },
        );
        button(container(tint).center(22))
            .padding(0)
            .style(theme::remove_button)
            .on_press_maybe(message)
    };
    let moves = moves
        .map(|[up, down]| row![chevron(Icon::ChevUp, up), chevron(Icon::Chev, down)].spacing(0));
    let cross = remove.map(|remove| {
        button(container(icons::tinted(Icon::Remove, 14.0, |p| p.faint)).center(22))
            .padding(0)
            .style(theme::remove_button)
            .on_press(remove)
    });
    let meta = meta.map(|meta| {
        text(meta)
            .size(11.5)
            .wrapping(Wrapping::None)
            .style(theme::faint_text)
    });
    let row = button(
        row![
            icons::icon(icon, icons::INLINE),
            container(text(name).size(CONTROL_TEXT).wrapping(Wrapping::None))
                .width(Length::Fill)
                .clip(true),
            meta,
            moves,
            cross,
        ]
        .spacing(8)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(CONTROL_HEIGHT)
    .padding(Padding::from([0.0, 3.0]).left(6.0))
    .style(theme::picked_row(hovered == Some(what)))
    .on_press_maybe(press);
    let (enter, exit) = hovering(what);
    mouse_area(row).on_enter(enter).on_exit(exit).into()
}

/// A typed value's field, named `label`, with the id `id`, showing why its
/// text is refused under it, and offering the design's parameters' names
/// as they're typed (see the `suggest` module). It sends `input` of the text typed, if the
/// document can be changed; `Enter` in it sends `submit` (OK), `Esc`
/// `cancel`, whatever has the focus: once, however many of a panel's
/// fields send it (`OnEscape`).
pub(crate) fn value_field<'a>(
    label_text: &'a str,
    id: iced::widget::Id,
    field_text: TypedField<'a>,
    input: Option<impl Fn(String) -> Message + 'a>,
    submit: Message,
    cancel: Message,
) -> Element<'a, Message> {
    let field_input = text_input(label_text, field_text.text)
        .id(id)
        .size(CONTROL_TEXT)
        // 28 px tall with its border, as the panel's other controls.
        .line_height(LineHeight::Absolute(16.0.into()))
        .padding([6, 8])
        .width(Length::Fill)
        .style(theme::field_input(field_text.error.is_some()));
    let field_input: Element<'a, Message> = match input {
        Some(on_input) => {
            let on_input = std::rc::Rc::new(on_input);
            let typing = std::rc::Rc::clone(&on_input);
            let typed = field_input
                .on_input(move |text| typing(text))
                .on_submit(submit);
            let params = field_text.params;
            crate::suggest::suggesting(typed, field_text.text, params, Some(&*on_input))
        }
        None => field_input.into(),
    };
    let field_input = OnEscape::new(field_input, cancel);
    let error = field_text.error.map(|error| {
        text(sentence(&error.to_string()).into_owned())
            .size(11.5)
            .wrapping(Wrapping::WordOrGlyph)
            .style(theme::danger_text)
    });
    column![label(label_text), field_input, error]
        .spacing(3)
        .into()
}

/// The Bodies list of a join, cut or intersect: a row per body of
/// `targets` in a box, as a [`pick_field`]'s, a checkbox before the
/// body's icon and name (muted while taken out), a click anywhere on it
/// sending `toggle` of it if the document can be changed; a body an
/// earlier join merged into another with "in Body 1" at the right; and
/// for a join ticked for two or more, which body it merges them into.
/// Hovering a row lights its body in the viewport, and it shows hovered
/// while `hovered` is it. None for a new body, or with nothing to list.
pub(crate) fn bodies<'a>(
    operation: OperationKind,
    targets: &[BodyTarget<'a>],
    toggle: impl Fn(BodyId) -> Option<Message>,
    hovered: Option<PanelHover>,
) -> Option<Element<'a, Message>> {
    if !operation.has_targets() || targets.is_empty() {
        return None;
    }
    let rows = targets.iter().map(|&target| {
        let message = toggle(target.body);
        let check = checkbox(target.included)
            .size(15)
            .style(theme::tick)
            .on_toggle_maybe(message.clone().map(|message| move |_| message.clone()));
        let included = target.included;
        let name = text(target.name)
            .size(CONTROL_TEXT)
            .wrapping(Wrapping::None)
            .style(move |theme: &iced::Theme| text::Style {
                color: (!included).then_some(theme::palette(theme).muted),
            });
        // Faint, as the Objects list notes a merged body.
        let holder = target.holder.map(|holder| {
            text(format!("in {holder}"))
                .size(11.5)
                .wrapping(Wrapping::None)
                .style(theme::faint_text)
        });
        let what = PanelHover::Body(target.body);
        let row = container(
            row![
                check,
                icons::icon(Icon::Body, icons::INLINE),
                container(name).width(Length::Fill).clip(true),
                holder,
            ]
            .spacing(8)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(CONTROL_HEIGHT)
        .padding(Padding::from([0.0, 8.0]).left(6.0))
        .style(theme::body_row(hovered == Some(what)));
        let (enter, exit) = hovering(what);
        let row = mouse_area(row).on_enter(enter).on_exit(exit);
        Element::from(match message {
            Some(message) => row
                .on_press(message)
                .interaction(mouse::Interaction::Pointer),
            None => row,
        })
    });
    let merging = joined_into(operation, targets).map(|holder| {
        // The mock's panel note: faint.
        text(format!("Joined into {holder}"))
            .size(12)
            .wrapping(Wrapping::WordOrGlyph)
            .style(theme::faint_text)
    });
    let list = container(column(rows))
        .width(Length::Fill)
        .padding(3)
        .style(theme::pick_box(false));
    Some(
        column![label("Bodies"), column![list, merging].spacing(6)]
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

/// The foot's message: why OK can't be pressed (`refused`, by the
/// operation's own check), else the draft failing (`error`), as "Extrude
/// fails" (`noun`) with `show`'s button, framing the camera on where
/// or going back, if its geometry has a box, and Add anyway sending
/// `accept` if it can be pressed, else, if `checking`, that OK waits on
/// the solver.
pub(crate) fn footer_message<'a>(
    noun: &'a str,
    refused: Option<String>,
    error: Option<&'a str>,
    show: Option<Framing>,
    accept: Option<Message>,
    checking: bool,
) -> Option<Footer<'a>> {
    match (refused, error) {
        // As the kernel's, but with nothing to add: the document would
        // refuse it.
        (Some(refused), _) => Some(Footer::Fails {
            noun,
            error: sentence(&refused).into_owned().into(),
            show: None,
            accept: None,
        }),
        (None, Some(error)) => Some(Footer::Fails {
            noun,
            error: sentence(error),
            show,
            accept,
        }),
        (None, None) => {
            checking.then(|| Footer::Text(message_text("Checking the sketch…", theme::muted_text)))
        }
    }
}

/// The text of a message in the panel, in `style`, broken within words
/// where they don't fit, as a message may quote a name.
pub(crate) fn message_text<'a>(
    message: impl text::IntoFragment<'a>,
    style: fn(&iced::Theme) -> text::Style,
) -> Element<'a, Message> {
    text(message)
        .size(CONTROL_TEXT)
        .wrapping(Wrapping::WordOrGlyph)
        .style(style)
        .into()
}

/// The card: a header, a body and a footer, one above the other, `width`
/// wide, under the accent line, the body and the footer in the recessed
/// well: the header and footer as tall as they are, the body in what's
/// left of the height the panel may take, at most as tall as it is.
///
/// A column can't do this: it lays its children out in order, so the
/// footer would get only what the body leaves, and a body filling the
/// rest would make the panel always as tall as it may be.
pub(crate) struct Sections<'a> {
    pub(crate) width: f32,
    pub(crate) parts: [Element<'a, Message>; 3],
    /// Whether the body sits in the recessed well; without it, the
    /// panel's colour goes on under the head, as the parameters' popup
    /// has it.
    pub(crate) well: bool,
}

/// How far in from the card's sides and bottom its parts are: inside its
/// 1 px border.
const BORDER: f32 = 1.0;

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
        let inner = (max.width - 2.0 * BORDER).max(0.0);
        let within = |height: f32| layout::Limits::new(Size::ZERO, Size::new(inner, height));
        // The header first: where even the header and footer don't fit,
        // its buttons keep their height and the message gives way.
        let room = (max.height - ACCENT_LINE - BORDER).max(0.0);
        let header = header
            .as_widget_mut()
            .layout(header_tree, renderer, &within(room));
        let left = (room - header.size().height).max(0.0);
        let footer = footer
            .as_widget_mut()
            .layout(footer_tree, renderer, &within(left));
        let left = (left - footer.size().height).max(0.0);
        let body = body
            .as_widget_mut()
            .layout(body_tree, renderer, &within(left));
        let body_top = ACCENT_LINE + header.size().height;
        let footer_top = body_top + body.size().height;
        let height = footer_top + footer.size().height + BORDER;
        let size = limits.resolve(
            Length::Fixed(self.width),
            Length::Shrink,
            Size::new(self.width, height),
        );
        layout::Node::with_children(
            size,
            vec![
                header.move_to((BORDER, ACCENT_LINE)),
                body.move_to((BORDER, body_top)),
                footer.move_to((BORDER, footer_top)),
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
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let p = theme::palette(theme);
        let card = layout.bounds();
        let radius = |top: f32, bottom: f32| iced::border::Radius::new(top).bottom(bottom);
        let quad = |bounds: Rectangle, radius, border: Border| renderer::Quad {
            bounds,
            border: Border { radius, ..border },
            ..renderer::Quad::default()
        };
        // Its shadow, then the accent line along its top, round at the
        // card's corners, the panel's colour over the rest of it.
        renderer.fill_quad(
            renderer::Quad {
                shadow: theme::CARD_SHADOW,
                ..quad(card, CARD_RADIUS.into(), Border::default())
            },
            Color::TRANSPARENT,
        );
        let accent = Rectangle {
            height: (ACCENT_LINE + CARD_RADIUS).min(card.height),
            ..card
        };
        renderer.fill_quad(
            quad(accent, radius(CARD_RADIUS, 0.0), Border::default()),
            p.accent,
        );
        let below = Rectangle {
            y: card.y + ACCENT_LINE,
            height: (card.height - ACCENT_LINE).max(0.0),
            ..card
        };
        renderer.fill_quad(
            quad(
                below,
                radius(CARD_RADIUS - ACCENT_LINE, CARD_RADIUS),
                Border {
                    color: p.line,
                    width: BORDER,
                    ..Border::default()
                },
            ),
            p.panel,
        );
        // The well, from the body's top to the card's border.
        if let Some(body) = layout.children().nth(1).filter(|_| self.well) {
            let top = body.bounds().y;
            let well = Rectangle {
                x: card.x + BORDER,
                y: top,
                width: (card.width - 2.0 * BORDER).max(0.0),
                height: (card.y + card.height - BORDER - top).max(0.0),
            };
            renderer.fill_quad(
                quad(
                    well,
                    radius(WELL_RADIUS, CARD_RADIUS - BORDER),
                    Border::default(),
                ),
                theme::well(p),
            );
        }
        let style = renderer::Style { text_color: p.text };
        for ((part, tree), layout) in self.parts.iter().zip(&tree.children).zip(layout.children()) {
            part.as_widget()
                .draw(tree, renderer, theme, &style, layout, cursor, viewport);
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
