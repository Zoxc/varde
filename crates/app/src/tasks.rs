//! The tasks the app starts: widget operations (focus, scrolling),
//! pickers, and awaited answers from the viewport.

use iced::Task;
use varde_io::Picked;

use crate::doc::{DocId, Focus};
use crate::message::{ForDoc, Message};

/// Waits for the thumbnail tagged `tag` of document `id` to come back on
/// `answer`, see `doc/thumbnail.rs`: `None` if it couldn't be rendered,
/// its sender dropped uncalled.
pub(crate) fn thumbnail(
    id: DocId,
    tag: u64,
    answer: iced::futures::channel::oneshot::Receiver<Option<varde_view::ThumbnailImages>>,
) -> Task<Message> {
    Task::perform(answer, move |images| {
        let thumbnail = images.ok().flatten().and_then(crate::thumbnail_of);
        Message::Doc(id, ForDoc::Thumbnail(tag, thumbnail))
    })
}

/// Has the value field take the focus, and select its text as `focus`
/// says: all of it to overtype, or the part a refusal is about. An
/// operation panel's body, where the field may be, scrolls back to its
/// top if the field shows from there, else down to the field
/// ([`RevealField`]): an extrude edited while another's panel was
/// scrolled keeps that panel, and so its scroll.
pub(crate) fn focus_field(focus: Focus) -> Task<Message> {
    use iced::widget::operation;

    let select = match focus {
        Focus::All => operation::select_all(varde_view::VALUE_FIELD),
        Focus::Range(start, end) => operation::select_range(varde_view::VALUE_FIELD, start, end),
    };
    operation::focus(varde_view::VALUE_FIELD)
        .chain(select)
        .chain(iced::advanced::widget::operate(RevealField::default()))
}

/// Scrolls an operation panel's body so the value field in it shows with
/// its label over it: to the body's top where the field shows from
/// there, else with the label at the top. A field outside the body (a
/// dimension's, in a sketch) leaves it be.
#[derive(Default)]
struct RevealField {
    /// The body's bounds and its content's.
    body: Option<(iced::Rectangle, iced::Rectangle)>,
    field: Option<iced::Rectangle>,
}

impl RevealField {
    /// How far above the field its label starts, with a little room.
    const LABEL: f32 = 24.0;
}

impl<T: Send + 'static> iced::advanced::widget::Operation<T> for RevealField {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<T>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: iced::Rectangle,
        content_bounds: iced::Rectangle,
        _translation: iced::Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&varde_view::PANEL_BODY) {
            self.body = Some((bounds, content_bounds));
        }
    }

    fn focusable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: iced::Rectangle,
        _state: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        if id == Some(&varde_view::VALUE_FIELD) {
            self.field = Some(bounds);
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<T> {
        use iced::advanced::widget::operation::{Outcome, scrollable};

        let (Some((body, content)), Some(field)) = (self.body, self.field) else {
            return Outcome::None;
        };
        // Where the field is in the content, which isn't moved by the
        // scroll in layout.
        let (top, bottom) = (field.y - content.y, field.y + field.height - content.y);
        if top < 0.0 || bottom > content.height {
            return Outcome::None;
        }
        let y = if bottom <= body.height {
            0.0
        } else {
            (top - Self::LABEL).max(0.0)
        };
        let offset = scrollable::AbsoluteOffset {
            x: None,
            y: Some(y),
        };
        Outcome::Chain(Box::new(scrollable::scroll_to(
            varde_view::PANEL_BODY,
            offset,
        )))
    }
}

/// Has the text field `id` take the focus, its text selected to
/// overtype: the rename field, a parameter's name, the Save As dialog's
/// name.
pub(crate) fn focus_selected(id: iced::widget::Id) -> Task<Message> {
    use iced::widget::operation;

    operation::focus(id.clone()).chain(operation::select_all(id))
}

/// Asks the browser what it says of its storage, answering with
/// [`Message::StorageState`]; natively it says nothing.
pub(crate) fn storage_state() -> Task<Message> {
    Task::perform(
        async {
            let persisted = varde_io::storage::persisted().await;
            let space = varde_io::storage::estimate().await;
            (persisted, space)
        },
        |(persisted, space)| Message::StorageState { persisted, space },
    )
}

/// Scrolls the scrolled list `id` (the rail's open list, the Timeline)
/// to `share` of the way to its end.
pub(crate) fn snap_list(id: iced::widget::Id, share: f32) -> Task<Message> {
    use iced::widget::operation::{self, RelativeOffset};

    let offset = RelativeOffset {
        x: None,
        y: Some(share),
    };
    operation::snap_to(id, offset)
}

/// Asks the user for a design to open, answering with
/// [`Message::Picked`]: see `varde_io::pick`.
pub(crate) fn pick_open() -> Task<Message> {
    Task::perform(varde_io::pick::pick_open(), Message::Picked)
}

/// Asks the user where to save the document `id`, suggesting `name`,
/// answering with [`ForDoc::SaveAsPicked`].
pub(crate) fn pick_save_as(id: DocId, name: &str) -> Task<Message> {
    let name = name.to_owned();
    Task::perform(
        async move { varde_io::pick::pick_save(&name).await },
        move |chosen| Message::Doc(id, ForDoc::SaveAsPicked(chosen)),
    )
}

/// Asks the user where to export the document `id`'s bodies, suggesting
/// `name`, answering with [`ForDoc::ExportPicked`].
pub(crate) fn pick_export(id: DocId, name: &str) -> Task<Message> {
    let name = name.to_owned();
    Task::perform(
        async move { varde_io::pick::pick_export(&name).await },
        move |chosen| Message::Doc(id, ForDoc::ExportPicked(chosen)),
    )
}

/// Asks the browser whether the document `id` may be saved to the file
/// the user `picked`, answering with [`ForDoc::Writable`]: see
/// `varde_io::pick`.
pub(crate) fn ask_writable(id: DocId, picked: Picked) -> Task<Message> {
    Task::perform(
        async move { varde_io::pick::writable(&picked).await },
        move |result| Message::Doc(id, ForDoc::Writable(result)),
    )
}
