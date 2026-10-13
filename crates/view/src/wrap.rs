//! What the widgets wrapping one `content` share: the [`Widget`] methods
//! they pass straight on to it, as the first of their tree's children.
//!
//! [`Widget`]: iced::advanced::Widget

/// Writes, in an `impl Widget<Message, $theme, $renderer>`, each of the
/// `methods` named as passing straight on to `self.$field`, the first of
/// the widget's tree's children: `size`, `size_hint`, `children` and
/// `diff` (`$field` its only child), `layout`, `operate`, `update`,
/// `mouse_interaction` and `draw`.
///
/// `pass_to!(@update_if $passed; $field, $theme, $renderer)` writes an
/// `update` passing on only the events for which `$passed(event)` holds.
macro_rules! pass_to {
    (@update_if $passed:expr; $field:ident, $theme:ty, $renderer:ty) => {
        fn update(
            &mut self,
            tree: &mut iced::advanced::widget::Tree,
            event: &iced::Event,
            layout: iced::advanced::Layout<'_>,
            cursor: iced::advanced::mouse::Cursor,
            renderer: &$renderer,
            clipboard: &mut dyn iced::advanced::Clipboard,
            shell: &mut iced::advanced::Shell<'_, Message>,
            viewport: &iced::Rectangle,
        ) {
            if !$passed(event) {
                return;
            }
            self.$field.as_widget_mut().update(
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
    };
    ($field:ident, $theme:ty, $renderer:ty; $($method:ident),* $(,)?) => {
        $($crate::wrap::pass_to!(@$method $field $theme, $renderer);)*
    };
    (@size $field:ident $theme:ty, $renderer:ty) => {
        fn size(&self) -> iced::Size<iced::Length> {
            self.$field.as_widget().size()
        }
    };
    (@size_hint $field:ident $theme:ty, $renderer:ty) => {
        fn size_hint(&self) -> iced::Size<iced::Length> {
            self.$field.as_widget().size_hint()
        }
    };
    (@children $field:ident $theme:ty, $renderer:ty) => {
        fn children(&self) -> Vec<iced::advanced::widget::Tree> {
            vec![iced::advanced::widget::Tree::new(&self.$field)]
        }
    };
    (@diff $field:ident $theme:ty, $renderer:ty) => {
        fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
            tree.diff_children(std::slice::from_ref(&self.$field));
        }
    };
    (@layout $field:ident $theme:ty, $renderer:ty) => {
        fn layout(
            &mut self,
            tree: &mut iced::advanced::widget::Tree,
            renderer: &$renderer,
            limits: &iced::advanced::layout::Limits,
        ) -> iced::advanced::layout::Node {
            (self.$field.as_widget_mut()).layout(&mut tree.children[0], renderer, limits)
        }
    };
    (@operate $field:ident $theme:ty, $renderer:ty) => {
        fn operate(
            &mut self,
            tree: &mut iced::advanced::widget::Tree,
            layout: iced::advanced::Layout<'_>,
            renderer: &$renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            let tree = &mut tree.children[0];
            (self.$field.as_widget_mut()).operate(tree, layout, renderer, operation);
        }
    };
    (@update $field:ident $theme:ty, $renderer:ty) => {
        $crate::wrap::pass_to!(@update_if |_| true; $field, $theme, $renderer);
    };
    (@mouse_interaction $field:ident $theme:ty, $renderer:ty) => {
        fn mouse_interaction(
            &self,
            tree: &iced::advanced::widget::Tree,
            layout: iced::advanced::Layout<'_>,
            cursor: iced::advanced::mouse::Cursor,
            viewport: &iced::Rectangle,
            renderer: &$renderer,
        ) -> iced::advanced::mouse::Interaction {
            self.$field.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
    };
    (@draw $field:ident $theme:ty, $renderer:ty) => {
        fn draw(
            &self,
            tree: &iced::advanced::widget::Tree,
            renderer: &mut $renderer,
            theme: &$theme,
            style: &iced::advanced::renderer::Style,
            layout: iced::advanced::Layout<'_>,
            cursor: iced::advanced::mouse::Cursor,
            viewport: &iced::Rectangle,
        ) {
            self.$field.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        }
    };
}

pub(crate) use pass_to;
