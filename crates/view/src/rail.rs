//! The tool rail: a column of cards floating over the viewport's left
//! edge, one per tool set of the mode, each with the set's icon on its
//! head and its first tools on a recessed strip along its bottom.
//! Pointing at a head, clicking it or pressing the set's key (`Q`, `W`,
//! ...) opens the set's list beside its card, where a letter picks a
//! tool. Only what the app has is on it: a set with nothing yet isn't
//! shown.
//!
//! The app keeps which set is open, and closes it as the cursor leaves
//! (see [`RailLook`]); the layer ([`Rail`]) places the cards and the list
//! and closes the list on a click anywhere else.

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::{button, column, container, mouse_area, responsive, row, space, text};
use iced::{Alignment, Element, Event, Font, Length, Padding, Rectangle, Size, Vector};

use crate::chrome::{ChipSize, key_chip, scrolled, side_tip};
use crate::icons::{self, Icon};
use crate::shortcut::{
    Binding, DocumentKeys, Shortcut, combine_binding, constrain_binding, constraint_binding,
    extrude_binding, measure_binding, revolve_binding, sketch_binding, tool_binding,
};
use crate::status::STATUS_BAR_ROOM;
use crate::theme::{self, SEMIBOLD};
use crate::toolbar::tool_icon;
use crate::{ConstraintKind, DocumentState, Look, Message, Tool};

/// The gap between the cards, and between the rail and the viewport's
/// edges, and the list, in pixels.
const GAP: f32 = 6.0;

/// How wide a card is, in pixels, its border included.
const CARD_WIDTH: f32 = 48.0;

/// How far from the viewport's left the list of the open set starts.
const LIST_LEFT: f32 = GAP + CARD_WIDTH + GAP;

/// How far from the viewport's bottom the list stays at least: clear of
/// the floating status bar, in pixels.
const LIST_BOTTOM: f32 = STATUS_BAR_ROOM + GAP;

/// The padding inside the list, in pixels. The scrollbar of its rows
/// floats in the right padding, clear of their letters.
const LIST_PADDING: f32 = 8.0;

/// How wide the list of the open set is, in pixels.
const LIST_WIDTH: f32 = 230.0;

/// How wide and tall a card's tool is, in pixels, and the gap under it
/// but the last's.
const TOOL_WIDTH: f32 = 34.0;
const TOOL_HEIGHT: f32 = 32.0;
const TOOL_GAP: f32 = 1.0;

/// The padding above and below the tools in a card's strip, in pixels.
const STRIP_PADDING: f32 = 2.0;

/// The size of a head's icon and of a tool's, in pixels.
const ICON: f32 = 24.0;

/// The padding above and below a head's icon, in pixels.
const HEAD_PADDING: [f32; 2] = [7.0, 5.0];

/// How tall a card's head shows, in pixels: its icon and padding.
const HEAD_HEIGHT: f32 = HEAD_PADDING[0] + ICON + HEAD_PADDING[1];

/// What changes about the rail, from the rail, the keys and the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailLook {
    /// Opens the list of the set at this index: its head clicked.
    Open(usize),
    /// Opens the list of the set at this index, or closes it if it's the
    /// one open: the set's key.
    Toggle(usize),
    /// Closes the list: a click anywhere but on the rail and the list.
    Close,
    /// Moves the keys up the open list, from the first row to the last.
    Up,
    /// Moves the keys down the open list, from the last row to the first.
    Down,
    /// Puts the keys on this row of the open list: the cursor came over
    /// it, so the row highlighted is the one `Enter` picks.
    Row(usize),
    /// The cursor came over `.0` (`true`) or left it. Over a head opens
    /// its set's list, over a tool on a card closes it at once; off a
    /// head and the list closes it after a moment, unless the cursor is
    /// back on one by then, so it can cross to the list.
    Hover(RailSpot, bool),
}

/// The rail's set whose list is open, by its index among the mode's
/// sets, and the row of the list the keys are on, which `Enter` picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RailOpen {
    pub set: usize,
    pub row: usize,
}

/// The scrollable holding the rows of the open list.
pub const RAIL_LIST: iced::widget::Id = iced::widget::Id::new("rail-list");

/// A part of the rail the cursor can be over, see [`RailLook::Hover`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailSpot {
    /// The head of the set at this index.
    Head(usize),
    /// A tool on a card.
    Tool,
    /// The open set's list.
    List,
}

/// A tool of a set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entry {
    /// Picking the plane for a new sketch.
    Sketch,
    Extrude,
    Revolve,
    Combine,
    Measure,
    /// A sketch's tool.
    Tool(Tool),
    /// The Constrain tool.
    Constrain,
    /// Applying a constraint to the selection: one with a key.
    Constraint(ConstraintKind),
}

impl Entry {
    pub(crate) fn icon(self) -> Icon {
        match self {
            Entry::Sketch => Icon::Sketch,
            Entry::Extrude => Icon::Extrude,
            Entry::Revolve => Icon::Revolve,
            Entry::Combine => Icon::Combine,
            Entry::Measure => Icon::Measure,
            Entry::Tool(tool) => tool_icon(tool),
            Entry::Constrain => Icon::Constrain,
            Entry::Constraint(kind) => kind.icon(),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Entry::Sketch => "Sketch",
            Entry::Extrude => "Extrude",
            Entry::Revolve => "Revolve",
            Entry::Combine => "Combine",
            Entry::Measure => "Measure",
            Entry::Tool(tool) => tool.label(),
            Entry::Constrain => "Constrain",
            Entry::Constraint(kind) => kind.label(),
        }
    }

    /// Its label as `keys` show it: Sketch is "Sketch on face" while a
    /// face alone is selected, which it puts the new sketch on.
    pub(crate) fn shown_label(self, keys: DocumentKeys) -> &'static str {
        match self {
            Entry::Sketch if keys.face_selected => "Sketch on face",
            _ => self.label(),
        }
    }

    /// What it does and its own key: the toolbar's binding for it, so the
    /// rail does what the toolbar does, and is disabled where it is.
    pub(crate) fn binding(self, keys: DocumentKeys) -> Binding {
        match self {
            Entry::Sketch => sketch_binding(keys),
            Entry::Extrude => extrude_binding(keys),
            Entry::Revolve => revolve_binding(keys),
            Entry::Combine => combine_binding(keys),
            Entry::Measure => measure_binding(keys),
            Entry::Tool(tool) => tool_binding(tool, keys),
            Entry::Constrain => constrain_binding(keys),
            Entry::Constraint(kind) => {
                constraint_binding(kind, keys).expect("the rail's constraints have keys")
            }
        }
    }

    /// Whether it's in use: highlighted, as on the toolbar.
    fn on(self, using: Using) -> bool {
        match self {
            Entry::Sketch => using.picking_plane,
            Entry::Extrude => using.extruding,
            Entry::Revolve => using.revolving,
            Entry::Combine => using.combining,
            Entry::Measure => using.measuring,
            Entry::Tool(tool) => using.tool == Some(tool),
            Entry::Constrain => using.constraining,
            Entry::Constraint(_) => false,
        }
    }
}

/// What's in use, which the rail highlights.
#[derive(Debug, Clone, Copy)]
struct Using {
    /// Whether the plane for a new sketch is being picked.
    picking_plane: bool,
    extruding: bool,
    revolving: bool,
    combining: bool,
    measuring: bool,
    /// The sketch's tool, if one is.
    tool: Option<Tool>,
    constraining: bool,
}

impl Using {
    fn of(state: &DocumentState<'_>) -> Self {
        let sketch = state.sketch.as_ref();
        Self {
            picking_plane: state
                .picking_plane
                .is_some_and(|pick| pick.sketch.is_none()),
            extruding: state.extrude.is_some(),
            revolving: state.revolve.is_some(),
            combining: state.combine.is_some(),
            measuring: state.measure.is_some(),
            tool: sketch.and_then(|s| s.tool).map(|t| t.tool),
            constraining: sketch.is_some_and(|s| s.constraining),
        }
    }
}

/// A tool set: its name, its icon on its card's head, whose category
/// colours its list, and its tools, the first of which its card shows, as
/// many as fit (see [`fitting`]).
#[derive(Debug)]
pub(crate) struct ToolSet {
    pub(crate) name: &'static str,
    pub(crate) icon: Icon,
    pub(crate) entries: &'static [Entry],
}

/// The sets outside a sketch: a new sketch is made with the solids.
/// Transform and Construct join them once they have tools, before
/// Inspect, as the mock orders them.
const MODEL: [ToolSet; 3] = [
    ToolSet {
        name: "Create",
        icon: Icon::CatCreate,
        entries: &[Entry::Sketch, Entry::Extrude, Entry::Revolve],
    },
    ToolSet {
        name: "Modify",
        icon: Icon::CatModify,
        entries: &[Entry::Combine],
    },
    ToolSet {
        name: "Inspect",
        icon: Icon::CatInspect,
        entries: &[Entry::Measure],
    },
];

/// The sets in a sketch.
const SKETCH: [ToolSet; 4] = [
    ToolSet {
        name: "Draw",
        icon: Icon::CatDraw,
        entries: &[
            Entry::Tool(Tool::Line),
            Entry::Tool(Tool::Rectangle),
            Entry::Tool(Tool::Circle),
            Entry::Tool(Tool::Arc),
            Entry::Tool(Tool::Polygon),
            Entry::Tool(Tool::Spline),
            Entry::Tool(Tool::Point),
        ],
    },
    ToolSet {
        name: "Modify",
        icon: Icon::CatSketchModify,
        entries: &[
            Entry::Tool(Tool::Trim),
            Entry::Tool(Tool::Extend),
            Entry::Tool(Tool::Offset),
            Entry::Tool(Tool::Mirror),
            Entry::Tool(Tool::Fillet),
            Entry::Tool(Tool::Chamfer),
        ],
    },
    ToolSet {
        name: "Constraints",
        icon: Icon::CatConstrain,
        entries: &[
            Entry::Constrain,
            Entry::Constraint(ConstraintKind::Coincident),
            Entry::Constraint(ConstraintKind::Horizontal),
            Entry::Constraint(ConstraintKind::Vertical),
            Entry::Constraint(ConstraintKind::Parallel),
            Entry::Constraint(ConstraintKind::Perpendicular),
            Entry::Constraint(ConstraintKind::Tangent),
            Entry::Constraint(ConstraintKind::Smooth),
            Entry::Constraint(ConstraintKind::Equal),
            Entry::Constraint(ConstraintKind::Concentric),
            Entry::Constraint(ConstraintKind::Midpoint),
            Entry::Constraint(ConstraintKind::Symmetric),
            Entry::Constraint(ConstraintKind::Fix),
        ],
    },
    ToolSet {
        name: "Dimension",
        icon: Icon::CatDimension,
        entries: &[Entry::Tool(Tool::Dimension)],
    },
];

/// The rail's sets in a sketch if `sketching`, else outside one.
pub(crate) fn sets(sketching: bool) -> &'static [ToolSet] {
    if sketching { &SKETCH } else { &MODEL }
}

/// How many sets the rail has in a sketch if `sketching`, else outside
/// one: their keys and indices go up to it.
pub fn rail_sets(sketching: bool) -> usize {
    sets(sketching).len()
}

/// How many rows the list of the set at index `set` has, in a sketch if
/// `sketching`, else outside one: none past the sets.
pub fn rail_rows(sketching: bool, set: usize) -> usize {
    sets(sketching).get(set).map_or(0, |set| set.entries.len())
}

/// The letter picking each of `entries` from its set's list, in lower
/// case: its own key's where that's a letter alone, else the first free
/// one of its key's (with Shift) and its label's, if any is free, never
/// one of the mode's sets' keys (`sketching` or not).
pub(crate) fn letters(entries: &[Entry], sketching: bool) -> Vec<Option<char>> {
    let shortcuts: Vec<Shortcut> = entries
        .iter()
        .map(|entry| entry.binding(DocumentKeys::default()).shortcut)
        .collect();
    let mut taken: Vec<char> = SET_KEYS[..rail_sets(sketching).min(SET_KEYS.len())].to_vec();
    let mut letters: Vec<Option<char>> = shortcuts
        .iter()
        .map(|shortcut| {
            let own = shortcut.letter_key().filter(|_| shortcut.is_plain())?;
            (!taken.contains(&own)).then(|| {
                taken.push(own);
                own
            })
        })
        .collect();
    for ((letter, entry), shortcut) in letters.iter_mut().zip(entries).zip(&shortcuts) {
        if letter.is_some() {
            continue;
        }
        let label = entry.label().chars().map(|c| c.to_ascii_lowercase());
        *letter = (shortcut.letter_key().into_iter())
            .chain(label)
            .find(|c| c.is_ascii_lowercase() && !taken.contains(c));
        taken.extend(*letter);
    }
    letters
}

/// The open set's keys: its letters, and `Enter` for the row the keys
/// are on, each picking its tool where `keys` let it be used and doing
/// nothing where they don't, and the arrows moving up and down it: none
/// without a set open.
pub(crate) fn letter_bindings(keys: DocumentKeys) -> Vec<Binding> {
    let Some(open) = keys.rail else {
        return Vec::new();
    };
    let Some(set) = sets(keys.sketching).get(open.set) else {
        return Vec::new();
    };
    let letters = set
        .entries
        .iter()
        .zip(letters(set.entries, keys.sketching))
        .filter_map(|(entry, letter)| {
            Some(entry.binding(keys).claiming(Shortcut::letter(letter?)))
        });
    let enter =
        (set.entries.get(open.row)).map(|entry| entry.binding(keys).claiming(Shortcut::ENTER));
    let step = |shortcut, step| Binding::new(shortcut, Message::Look(Look::Rail(step)), true);
    letters
        .chain(enter)
        .chain([
            step(Shortcut::UP, RailLook::Up),
            step(Shortcut::DOWN, RailLook::Down),
        ])
        .collect()
}

/// The keys opening the rail's sets, in order: the keyboard's top row
/// from its left. No tool's key is one of them, and an open list's
/// letters aren't, so another set can be opened from one.
pub(crate) const SET_KEYS: [char; 9] = ['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o'];

/// The key opening the set at index `set`, if it has one.
pub(crate) fn set_key(set: usize) -> Option<Shortcut> {
    SET_KEYS.get(set).map(|&key| Shortcut::letter(key))
}

/// The sets' keys, from `Q`, each opening its set's list or closing it.
pub(crate) fn set_bindings(sketching: bool) -> impl Iterator<Item = Binding> {
    (0..rail_sets(sketching)).filter_map(|i| {
        let message = Message::Look(Look::Rail(RailLook::Toggle(i)));
        Some(Binding::new(set_key(i)?, message, true))
    })
}

/// How tall a card showing `tools` of its set's tools is, in pixels, its
/// border included.
fn card_height(tools: usize) -> f32 {
    let head = 2.0 + HEAD_HEIGHT;
    if tools == 0 {
        return head;
    }
    let tools = tools as f32;
    head + 2.0 * STRIP_PADDING + tools * TOOL_HEIGHT + (tools - 1.0) * TOOL_GAP
}

/// How many of its tools each of `sets` shows on its card for the rail to
/// fit in `room` pixels of height: tools are added one at a time to the
/// set showing the fewest that has more (the first of those), until the
/// next doesn't fit, so the room is shared evenly. Heads always show, so
/// a rail too tall even without tools runs past the room.
pub(crate) fn fitting(sets: &[ToolSet], room: f32) -> Vec<usize> {
    let mut shown = vec![0; sets.len()];
    let gaps = GAP * sets.len().saturating_sub(1) as f32;
    let mut height = gaps + shown.iter().map(|&n| card_height(n)).sum::<f32>();
    loop {
        let next = (0..sets.len())
            .filter(|&i| shown[i] < sets[i].entries.len())
            .min_by_key(|&i| shown[i]);
        let Some(i) = next else {
            return shown;
        };
        let grown = height - card_height(shown[i]) + card_height(shown[i] + 1);
        if grown > room {
            return shown;
        }
        shown[i] += 1;
        height = grown;
    }
}

/// The rail of `state`'s mode, and the list of its open set, if one is,
/// as a layer over the viewport, its cards showing as many tools as fit
/// in its height, clear of the status bar.
pub(crate) fn rail<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    let keys = state.keys();
    let using = Using::of(state);
    let sets = sets(state.sketch.is_some());
    let open = state.rail.filter(|open| open.set < sets.len());
    responsive(move |size| {
        let shown = fitting(sets, size.height - GAP - LIST_BOTTOM);
        let cards = sets.iter().zip(shown).enumerate().map(|(i, (set, shown))| {
            let open = open.is_some_and(|open| open.set == i);
            card(i, set, shown, open, keys, using)
        });
        let list = open.map(|open| list(open, &sets[open.set], keys, using));
        Element::new(Rail {
            open: open.map(|open| open.set),
            children: std::iter::once(column(cards).spacing(GAP).into())
                .chain(list)
                .collect(),
        })
    })
    .into()
}

fn hover(spot: RailSpot, over: bool) -> Message {
    Message::Look(Look::Rail(RailLook::Hover(spot, over)))
}

/// The card of the set `set` at index `i`, showing its first `shown`
/// tools, its head highlighted while it's `open`.
fn card<'a>(
    i: usize,
    set: &'static ToolSet,
    shown: usize,
    open: bool,
    keys: DocumentKeys,
    using: Using,
) -> Element<'a, Message> {
    let shown = &set.entries[..set.entries.len().min(shown)];
    let strip = !shown.is_empty();
    let head = button(
        container(icons::icon(set.icon, ICON))
            .center_x(Length::Fill)
            .padding(Padding::ZERO.top(HEAD_PADDING[0])),
    )
    .width(Length::Fill)
    .height(HEAD_HEIGHT)
    .padding(0)
    .style(theme::rail_head(open, strip))
    .on_press(Message::Look(Look::Rail(RailLook::Open(i))));
    let head = mouse_area(head)
        .on_enter(hover(RailSpot::Head(i), true))
        .on_exit(hover(RailSpot::Head(i), false));
    let strip = strip.then(|| {
        let tools = column(shown.iter().map(|&entry| card_tool(entry, keys, using)))
            .spacing(TOOL_GAP)
            .padding([STRIP_PADDING, 0.0])
            .width(Length::Fill)
            .align_x(Alignment::Center);
        // The head's highlight runs on behind the strip's rounded top
        // corners, so it leaves no wedges of the card there; the head is
        // highlighted while its set is open, which pointing at it does.
        let strip = container(
            container(tools)
                .width(Length::Fill)
                .style(theme::rail_strip),
        )
        .style(theme::rail_strip_backing(open));
        // Its gaps don't reach the scene.
        mouse_area(strip).interaction(mouse::Interaction::Idle)
    });
    let content = column![head, strip].width(Length::Fill);
    // Clicks on the card's border don't reach the scene either.
    mouse_area(
        container(content)
            .width(CARD_WIDTH)
            .padding(1)
            .style(theme::rail_card),
    )
    .interaction(mouse::Interaction::Idle)
    .into()
}

/// A tool on a card: its icon, with its name and key beside it while
/// hovered.
fn card_tool<'a>(entry: Entry, keys: DocumentKeys, using: Using) -> Element<'a, Message> {
    let binding = entry.binding(keys);
    let tool = button(
        container(icons::icon(entry.icon(), ICON))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(TOOL_WIDTH)
    .height(TOOL_HEIGHT)
    .padding(0)
    .style(theme::flat_button(entry.on(using)))
    .on_press_maybe(binding.sends());
    let tool = mouse_area(tool)
        .on_enter(hover(RailSpot::Tool, true))
        .on_exit(hover(RailSpot::Tool, false));
    side_tip(tool, entry.shown_label(keys), binding.shortcut)
}

/// The list of the set `set`, open as `open` says: its name, its key
/// and `Esc`, then a row per tool, with the letter picking it, the row
/// the keys are on highlighted.
fn list<'a>(
    open: RailOpen,
    set: &'static ToolSet,
    keys: DocumentKeys,
    using: Using,
) -> Element<'a, Message> {
    let set_key = set_key(open.set).map(|key| key_chip(key, ChipSize::Normal));
    let header = row![
        icons::icon(set.icon, icons::INLINE),
        text(set.name).font(SEMIBOLD),
        set_key,
        space::horizontal(),
        key_chip(Shortcut::ESCAPE, ChipSize::Normal),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .padding(Padding::from([2, 4]).bottom(6).right(4.0 + LIST_PADDING));
    let rows = set
        .entries
        .iter()
        .zip(letters(set.entries, keys.sketching))
        .enumerate()
        .map(|(row, (&entry, letter))| {
            // Pointing at a row puts the keys on it, so one row shows
            // highlighted.
            mouse_area(list_row(entry, letter, row == open.row, keys, using))
                .on_enter(Message::Look(Look::Rail(RailLook::Row(row))))
        });
    let margin = (LIST_PADDING - theme::SCROLLBAR_WIDTH) / 2.0;
    let rows = column(rows.map(Element::from)).padding(Padding::ZERO.right(LIST_PADDING));
    let body = column![header, scrolled(rows, margin).id(RAIL_LIST)];
    let category = set.icon.category().expect("a set's icon has a category");
    let list = container(
        container(body)
            .width(LIST_WIDTH)
            .padding(Padding::new(LIST_PADDING).right(0))
            .style(theme::rail_list),
    )
    .padding(Padding::ZERO.top(theme::RAIL_LIST_BAND))
    .style(theme::rail_list_band(category));
    mouse_area(list)
        .interaction(mouse::Interaction::Idle)
        .on_enter(hover(RailSpot::List, true))
        .on_exit(hover(RailSpot::List, false))
        .into()
}

/// A row of a set's list: `entry`'s icon and label, and the `letter`
/// picking it from the list, highlighted while the keys are on it
/// (`focused`).
fn list_row<'a>(
    entry: Entry,
    letter: Option<char>,
    focused: bool,
    keys: DocumentKeys,
    using: Using,
) -> Element<'a, Message> {
    let letter = letter.map(|letter| {
        container(
            text(letter.to_ascii_uppercase())
                .size(10.5)
                .font(Font::MONOSPACE)
                .style(theme::faint_text),
        )
        .align_right(Length::Fill)
    });
    button(
        row![
            icons::icon(entry.icon(), icons::INLINE),
            text(entry.shown_label(keys)),
            letter,
        ]
        .spacing(9)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(28)
    .padding([0, 6])
    .style(theme::rail_row(entry.on(using), focused))
    .on_press_maybe(entry.binding(keys).sends())
    .into()
}

/// The rail's layer: the cards at the viewport's top left, and the list
/// of the open set, if one is, beside its card, its top level with the
/// card's, moved up as far as it must to stay in the viewport, clear of
/// the status bar, and scrolled if it's taller than that. It takes only what's over them, and
/// a click anywhere else closes the list.
struct Rail<'a> {
    /// The index of the open set, whose list is the second child.
    open: Option<usize>,
    /// The cards' column, then the list, if a set is open.
    children: Vec<Element<'a, Message>>,
}

impl Rail<'_> {
    /// Whether `layout`'s cards or list are under `cursor`.
    fn over(layout: Layout<'_>, cursor: mouse::Cursor) -> bool {
        let mut children = layout.children();
        let cards = children.next().into_iter().flat_map(|rail| rail.children());
        cards
            .chain(children)
            .any(|part| cursor.is_over(part.bounds()))
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for Rail<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);
        let mut parts = self.children.iter_mut().zip(&mut tree.children);
        let (rail, rail_tree) = parts.next().expect("the rail");
        // A rail taller than the viewport runs past its bottom.
        let room = Size::new((size.width - GAP).max(0.0), f32::INFINITY);
        let rail = rail
            .as_widget_mut()
            .layout(rail_tree, renderer, &layout::Limits::new(Size::ZERO, room))
            .move_to((GAP, GAP));
        let mut nodes = vec![];
        if let (Some(open), Some((list, list_tree))) = (self.open, parts.next()) {
            let card_top = rail
                .children()
                .get(open)
                .map_or(0.0, |card| card.bounds().y);
            let room = Size::new(
                (size.width - LIST_LEFT - GAP).max(0.0),
                (size.height - GAP - LIST_BOTTOM).max(0.0),
            );
            let list = list.as_widget_mut().layout(
                list_tree,
                renderer,
                &layout::Limits::new(Size::ZERO, room),
            );
            let top = (size.height - LIST_BOTTOM - list.size().height)
                .min(GAP + card_top)
                .max(GAP);
            nodes.push(list.move_to((LIST_LEFT, top)));
        }
        nodes.insert(0, rail);
        layout::Node::with_children(size, nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        for ((child, tree), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
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
        // The list over the cards.
        for ((child, tree), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .rev()
        {
            child.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
        // Wherever it is, over the scene or the side panel; the click
        // goes on to what's there.
        if self.open.is_some()
            && let Event::Mouse(mouse::Event::ButtonPressed(_)) = event
            && cursor.land().position().is_some()
            && !Self::over(layout, cursor.land())
        {
            shell.publish(Message::Look(Look::Rail(RailLook::Close)));
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
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .rev()
            .map(|((child, tree), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .find(|&interaction| interaction != mouse::Interaction::None)
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
        for ((child, tree), layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            child
                .as_widget()
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
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests;
