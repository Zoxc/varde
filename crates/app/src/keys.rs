//! The screens' shortcuts, matched against the view's bindings.

use iced::keyboard;

use crate::Message;

/// The welcome screen's shortcuts, see [`varde_view::welcome_bindings`],
/// if they're `on`: not while a prompt shows over it.
pub(crate) fn welcome_key((on, event): (bool, keyboard::Event)) -> Option<Message> {
    on.then(|| pressed(event, varde_view::welcome_bindings()))
        .flatten()
}

/// The document screen's shortcuts, given what they depend on, see
/// [`varde_view::document_bindings`]: none without it, see [`Doc::keys`].
///
/// [`Doc::keys`]: crate::doc::Doc::keys
pub(crate) fn document_key(
    (keys, event): (Option<varde_view::DocumentKeys>, keyboard::Event),
) -> Option<Message> {
    pressed(event, varde_view::document_bindings(keys?))
}

/// The message of the first enabled binding of `bindings` that `event`
/// presses, if any.
fn pressed(
    event: keyboard::Event,
    bindings: impl IntoIterator<Item = varde_view::Binding>,
) -> Option<Message> {
    let keyboard::Event::KeyPressed {
        key,
        modifiers,
        repeat: false,
        ..
    } = event
    else {
        return None;
    };
    varde_view::pressed(bindings, &key, modifiers).map(Message::Ui)
}
