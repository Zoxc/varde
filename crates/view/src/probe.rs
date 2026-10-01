//! What tests find on a screen laid out headless: a renderer that measures
//! text as the app does, and the texts shown and where. Built for the
//! crate's tests and, with the `probe` feature, for other crates' tests.

use iced::advanced::widget::{Id, Operation, operation::Scrollable};
use iced::{Rectangle, Size, Vector};

/// A headless tiny-skia renderer, which measures text as the app does.
pub fn renderer() -> iced::Renderer {
    use iced::advanced::renderer::Headless;
    iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.0),
        Some("tiny-skia"),
    ))
    .expect("a headless renderer")
}

/// A text shown, see [`Texts`].
#[derive(Debug, Clone)]
pub struct Shown {
    pub text: String,
    /// Where it's laid out, moved by the scrollables it's in.
    pub bounds: Rectangle,
    /// The part of `bounds` those scrollables let show, if it's in any.
    pub visible: Option<Rectangle>,
}

impl Shown {
    /// The part of it shown, as far as its scrollables go.
    pub fn seen(&self) -> Rectangle {
        self.visible.unwrap_or(self.bounds)
    }

    /// Whether its scrollables show all of it, to a rounding error.
    pub fn whole(&self) -> bool {
        self.seen().height >= self.bounds.height - 0.01
    }

    /// Whether its scrollables hide all of it.
    pub fn hidden(&self) -> bool {
        self.seen().height <= 0.0
    }
}

/// An operation collecting each text shown: the labels of buttons,
/// checkboxes and the rest, which report their text and bounds to
/// operations. Run it, then read `shown`.
#[derive(Default)]
pub struct Texts {
    /// How far the scrollables entered have scrolled their content.
    offset: Vector,
    /// Where the scrollables entered show their content.
    clip: Option<Rectangle>,
    /// The scrollable just met, which the next traverse enters.
    entering: Option<(Vector, Rectangle)>,
    /// The texts found, in the order the widgets report them.
    pub shown: Vec<Shown>,
}

/// The part of `bounds` inside `clip`, empty where it's outside.
fn within(clip: Rectangle, bounds: Rectangle) -> Rectangle {
    clip.intersection(&bounds)
        .unwrap_or(Rectangle::new(bounds.position(), Size::ZERO))
}

impl Operation for Texts {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
        let (offset, clip) = (self.offset, self.clip);
        if let Some((translation, bounds)) = self.entering.take() {
            let bounds = bounds - self.offset;
            self.offset += translation;
            self.clip = Some(clip.map_or(bounds, |clip| within(clip, bounds)));
        }
        operate(self);
        (self.offset, self.clip) = (offset, clip);
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        self.entering = Some((translation, bounds));
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, text: &str) {
        let bounds = bounds - self.offset;
        self.shown.push(Shown {
            text: text.to_owned(),
            bounds,
            visible: self.clip.map(|clip| within(clip, bounds)),
        });
    }
}
