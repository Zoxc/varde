//! Parameter names offered under a typed value's field while a name is
//! typed: those starting with the word before the caret, each with its
//! value, the first lit. Tab or a click puts one in, in place of the word.
//!
//! The view sees a field's text, not its caret, so the word is the one
//! the text ends with ([`suggest`]); the list shows, and Tab takes the
//! first, only while the field has the focus and its caret is at the end
//! ([`Suggesting`]). Putting one in sends the field's input of the text
//! with it, as typing does, and moves the caret to the end.

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::keyboard::{self, key::Named};
use iced::widget::text::Wrapping;
use iced::widget::text_input::{self, Value, cursor};
use iced::widget::{button, column, container, row, text};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};
use varde_expr::{LengthUnit, Params};

use crate::{Message, theme};

/// The most names offered at once.
pub const MAX_SUGGESTIONS: usize = 6;

/// The design's parameters a field reads names with, and its units, which
/// lengths show in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamsIn<'a> {
    pub params: &'a Params,
    pub units: LengthUnit,
}

impl ParamsIn<'static> {
    /// No parameters: nothing is offered.
    pub const NONE: ParamsIn<'static> = ParamsIn {
        params: Params::EMPTY,
        units: LengthUnit::Mm,
    };
}

/// What [`suggest`] offers: where the word starts in the text, by byte,
/// and the names, in the design's order, each with its value as shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion<'a> {
    pub start: usize,
    pub names: Vec<(&'a str, String)>,
}

/// The names `params` has that start with the word `text` ends with, but
/// aren't it, at most [`MAX_SUGGESTIONS`]: none where it doesn't end with
/// one (a word starts with a letter or `_`; "10mm" is a number and a
/// unit), nor where the word is where a unit goes, right after a number
/// or a `)`. A parameter in error shows "error" for its value.
pub fn suggest<'a>(text: &str, params: ParamsIn<'a>) -> Option<Suggestion<'a>> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let start = text
        .char_indices()
        .rev()
        .take_while(|&(_, c)| word(c))
        .last()
        .map(|(at, _)| at)?;
    let typed = &text[start..];
    if !typed.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    // A word right after a number or a `)` is a unit, not a name.
    let before = text[..start].trim_end();
    if before.ends_with(')') {
        return None;
    }
    if before.ends_with(|c: char| c.is_ascii_digit() || c == '.') {
        let run = before
            .char_indices()
            .rev()
            .take_while(|&(_, c)| word(c) || c == '.')
            .last()
            .map_or(before, |(at, _)| &before[at..]);
        if !run.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            return None;
        }
    }
    let names: Vec<_> = (params.params.iter())
        .filter(|(name, _)| name.len() > typed.len() && name.starts_with(typed))
        .take(MAX_SUGGESTIONS)
        .map(|(name, result)| {
            let shown = match result {
                Ok(value) => varde_expr::format(value.value, value.quantity.unit(params.units)),
                Err(_) => "error".to_owned(),
            };
            (name, shown)
        })
        .collect();
    (!names.is_empty()).then_some(Suggestion { start, names })
}

/// `text` with `name` in place of the word from `start` on.
pub fn take(text: &str, start: usize, name: &str) -> String {
    let kept = text.get(..start).unwrap_or(text);
    format!("{kept}{name}")
}

/// `input`, a field showing `text`, offering `params`' names as typed
/// (see the module's docs), putting one in with `on_input`. With
/// `on_input` the field is wrapped whether or not any names are offered,
/// so that its state, the focus and caret, outlives names coming and
/// going.
pub(crate) fn suggesting<'a>(
    input: impl Into<Element<'a, Message>>,
    typed: &'a str,
    params: ParamsIn<'a>,
    on_input: Option<&dyn Fn(String) -> Message>,
) -> Element<'a, Message> {
    let Some(on_input) = on_input else {
        return input.into();
    };
    let Some(found) = suggest(typed, params) else {
        return Element::new(Suggesting {
            input: input.into(),
            list: None,
            text: typed,
        });
    };
    let first = take(typed, found.start, found.names[0].0);
    let rows = found
        .names
        .iter()
        .enumerate()
        .map(|(index, (name, shown))| {
            let line = row![
                container(text(*name).size(12.5).font(theme::SEMIBOLD))
                    .width(Length::Fill)
                    .clip(true),
                text(shown.clone())
                    .size(12.0)
                    .wrapping(Wrapping::None)
                    .style(theme::muted_text),
            ]
            .spacing(12);
            button(line)
                .width(Length::Fill)
                .padding([4, 8])
                .style(theme::suggestion_row(index == 0))
                .on_press(on_input(take(typed, found.start, name)))
                .into()
        });
    let hint =
        container(text("Tab puts it in").size(11.5).style(theme::muted_text)).padding([2, 8]);
    let list = container(column(rows).push(hint).spacing(1))
        .padding(3)
        .style(theme::menu);
    Element::new(Suggesting {
        input: input.into(),
        list: Some((list.into(), on_input(first))),
        text: typed,
    })
}

/// A text field with names offered under it, see the module's docs.
struct Suggesting<'a> {
    input: Element<'a, Message>,
    /// The names offered, if any, and the field's input with the first
    /// put in, for Tab.
    list: Option<(Element<'a, Message>, Message)>,
    text: &'a str,
}

type InputState = text_input::State<<iced::Renderer as iced::advanced::text::Renderer>::Paragraph>;

/// Whether the field whose state is in `tree` has the focus, its caret at
/// the end of `text`.
fn ready(tree: &Tree, text: &str) -> bool {
    let Some(state) = tree.state.downcast_ref_checked() else {
        return false;
    };
    let state: &InputState = state;
    let value = Value::new(text);
    state.is_focused() && state.cursor().state(&value) == cursor::State::Index(value.len())
}

trait DowncastChecked {
    fn downcast_ref_checked<T: 'static>(&self) -> Option<&T>;
}

impl DowncastChecked for iced::advanced::widget::tree::State {
    fn downcast_ref_checked<T: 'static>(&self) -> Option<&T> {
        match self {
            iced::advanced::widget::tree::State::Some(state) => state.downcast_ref::<T>(),
            iced::advanced::widget::tree::State::None => None,
        }
    }
}

/// Moves the caret of the field whose state is in `tree` to the end.
fn caret_to_end(tree: &mut Tree) {
    if let iced::advanced::widget::tree::State::Some(state) = &mut tree.state
        && let Some(state) = state.downcast_mut::<InputState>()
    {
        state.move_cursor_to_end();
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for Suggesting<'_> {
    fn size(&self) -> Size<Length> {
        self.input.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.input.as_widget().size_hint()
    }

    fn children(&self) -> Vec<Tree> {
        let list = self.list.iter().map(|(list, _)| Tree::new(list));
        std::iter::once(Tree::new(&self.input))
            .chain(list)
            .collect()
    }

    fn diff(&self, tree: &mut Tree) {
        // The field is always the first child, so its state is kept.
        match &self.list {
            Some((list, _)) => tree.diff_children(&[&self.input, list]),
            None => tree.diff_children(&[&self.input]),
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        (self.input.as_widget_mut()).layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        (self.input.as_widget_mut()).operate(&mut tree.children[0], layout, renderer, operation);
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
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Tab),
            modifiers,
            ..
        }) = event
            && modifiers.is_empty()
            && !shell.is_event_captured()
            && let Some((_, on_tab)) = &self.list
            && ready(&tree.children[0], self.text)
        {
            shell.publish(on_tab.clone());
            shell.capture_event();
            caret_to_end(&mut tree.children[0]);
            return;
        }
        self.input.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
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
        (self.input.as_widget()).mouse_interaction(
            &tree.children[0],
            layout,
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
        self.input.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let (list_element, _) = self.list.as_mut()?;
        if !ready(&tree.children[0], self.text) {
            return None;
        }
        let bounds = layout.bounds();
        let [input, list] = tree.children.as_mut_slice() else {
            return None;
        };
        Some(overlay::Element::new(Box::new(List {
            at: Point::new(bounds.x, bounds.y + bounds.height + 3.0) + translation,
            width: bounds.width,
            list: list_element,
            tree: list,
            input,
        })))
    }
}

/// The names offered, under the field.
struct List<'a, 'b> {
    at: Point,
    /// The field's width, the least the list's is.
    width: f32,
    list: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    /// The field's state, whose caret goes to the end as a name is put in.
    input: &'b mut Tree,
}

impl List<'_, '_> {
    fn list_layout<'l>(layout: Layout<'l>) -> Layout<'l> {
        layout.children().next().expect("the list's layout")
    }
}

impl overlay::Overlay<Message, iced::Theme, iced::Renderer> for List<'_, '_> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::new(self.width, 0.0), bounds);
        let list = (self.list.as_widget_mut()).layout(self.tree, renderer, &limits);
        let size = list.size();
        let x = self.at.x.min(bounds.width - size.width).max(0.0);
        let y = self.at.y.min(bounds.height - size.height).max(0.0);
        layout::Node::with_children(bounds, vec![list.move_to(Point::new(x, y))])
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.list.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            Self::list_layout(layout),
            cursor,
            &layout.bounds(),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let list = Self::list_layout(layout);
        self.list.as_widget_mut().update(
            self.tree,
            event,
            list,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        );
        // A press on the list keeps the field's focus, as the field never
        // sees it.
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event
            && cursor.is_over(list.bounds())
        {
            shell.capture_event();
            caret_to_end(self.input);
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let list = Self::list_layout(layout);
        if !cursor.is_over(list.bounds()) {
            return mouse::Interaction::None;
        }
        self.list
            .as_widget()
            .mouse_interaction(self.tree, list, cursor, &layout.bounds(), renderer)
    }
}

#[cfg(test)]
mod tests;
