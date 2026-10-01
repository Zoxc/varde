//! What the crate's tests build sketches with, and how they lay widgets out
//! and draw them headless.

use glam::DVec2;
use varde_expr::{LengthUnit, Value};
use varde_sketch::{Curve, Design, Dimension, Handle, Id, Measure, Sketch, Spline};

use crate::typed::{DEFAULT_SIDES, Field};
use crate::{ActiveTool, Target, Tool};

/// A design in millimetres.
pub(crate) const DESIGN: Design = Design {
    max: 1e6,
    units: LengthUnit::Mm,
};

pub(crate) fn at(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

pub(crate) fn point(sketch: &mut Sketch, x: f64, y: f64) -> Id {
    sketch.add_point(at(x, y)).unwrap()
}

pub(crate) fn line(sketch: &mut Sketch, start: Id, end: Id) -> Id {
    sketch.add_curve(Curve::Line { start, end }, false).unwrap()
}

/// An open spline through fit points at `places`: it and its fit points.
pub(crate) fn spline(sketch: &mut Sketch, places: &[(f64, f64)]) -> (Id, Vec<Id>) {
    let fit: Vec<Id> = places.iter().map(|&(x, y)| point(sketch, x, y)).collect();
    let curve = Curve::Spline(Spline::through(fit.clone(), false));
    (sketch.add_curve(curve, false).unwrap(), fit)
}

/// A spline through (0, 0), (10, 4) and (20, 0) with a handle at its
/// middle fit point, its tip at (13, 7): the spline, its fit points and
/// the tip.
pub(crate) fn handled_spline(sketch: &mut Sketch) -> (Id, Vec<Id>, Id) {
    let (spline, fit) = spline(sketch, &[(0.0, 0.0), (10.0, 4.0), (20.0, 0.0)]);
    let tip = point(sketch, 13.0, 7.0);
    if let Some(Curve::Spline(shape)) = sketch.curve_mut(spline).map(|entry| &mut entry.curve) {
        shape.handles.push(Handle { at: fit[1], tip });
    }
    (spline, fit, tip)
}

/// A plate from (-`x`, -`y`) to (`x`, `y`) with a hole of `radius` at the
/// origin: four lines and a circle.
pub(crate) fn plate(x: f64, y: f64, radius: f64) -> Sketch {
    let mut sketch = Sketch::default();
    let corners = [(-x, -y), (x, -y), (x, y), (-x, y)].map(|(x, y)| point(&mut sketch, x, y));
    for (i, &start) in corners.iter().enumerate() {
        line(&mut sketch, start, corners[(i + 1) % 4]);
    }
    let center = point(&mut sketch, 0.0, 0.0);
    sketch
        .add_curve(Curve::Circle { center, radius }, false)
        .unwrap();
    sketch
}

/// Adds a dimension of `measure`, `text` read in millimetres, on the side
/// the geometry is on, its label `label` from its anchor.
pub(crate) fn dimension(
    sketch: &mut Sketch,
    measure: Measure,
    text: &str,
    driving: bool,
    label: DVec2,
) -> Id {
    let value = Value::new(text, &measure.ask(&DESIGN)).unwrap();
    let side = sketch.side(&measure);
    let dimension = Dimension {
        measure,
        value,
        driving,
        label,
        side,
    };
    sketch.add_dimension(dimension).unwrap()
}

/// `tool` in use with `placed` placed, snapped to `targets`, drawing
/// normal geometry with nothing typed.
pub(crate) fn tool<'a>(
    tool: Tool,
    placed: &'a [DVec2],
    targets: &'a [Option<Target>],
) -> ActiveTool<'a> {
    ActiveTool {
        tool,
        placed,
        targets,
        construction: false,
        picked: &[],
        about: false,
        switched: false,
        typed: &[],
        sides: DEFAULT_SIDES,
        centered: false,
        control: false,
    }
}

/// `text` typed in `field`, read in millimetres.
pub(crate) fn typed(field: Field, text: &str) -> (Field, Value) {
    (field, Value::new(text, &field.ask(&DESIGN)).unwrap())
}

/// A headless tiny-skia renderer, which measures text as the app does.
pub(crate) fn renderer() -> iced::Renderer {
    use iced::advanced::renderer::Headless;
    pollster::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.0),
        Some("tiny-skia"),
    ))
    .expect("a headless renderer")
}

/// An element laid out headless at the top left of `max`.
pub(crate) struct Laid<'a> {
    pub element: iced::Element<'a, crate::Message>,
    pub tree: iced::advanced::widget::Tree,
    pub node: iced::advanced::layout::Node,
    pub renderer: iced::Renderer,
}

impl<'a> Laid<'a> {
    pub(crate) fn new(
        element: impl Into<iced::Element<'a, crate::Message>>,
        max: iced::Size,
    ) -> Self {
        use iced::advanced::layout::Limits;
        let mut element = element.into();
        let renderer = renderer();
        let mut tree = iced::advanced::widget::Tree::new(&element);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &Limits::new(iced::Size::ZERO, max),
        );
        Laid {
            element,
            tree,
            node,
            renderer,
        }
    }

    /// Each text shown and where it is: its bounds moved by the
    /// scrollables it's in, and the part of them they let show.
    pub(crate) fn texts(&mut self) -> Vec<Shown> {
        let mut find = Find::default();
        self.element.as_widget_mut().operate(
            &mut self.tree,
            iced::advanced::Layout::new(&self.node),
            &self.renderer,
            &mut find,
        );
        find.shown
    }

    /// The RGBA pixels of it drawn in light mode on a `size` window.
    pub(crate) fn pixels(&mut self, size: iced::Size<u32>) -> Vec<u8> {
        use iced::advanced::renderer::Headless;
        use iced::theme::Base;
        let theme = crate::iced_theme(crate::Mode::Light);
        let base = theme.base();
        let viewport =
            iced::Rectangle::with_size(iced::Size::new(size.width as f32, size.height as f32));
        self.element.as_widget().draw(
            &self.tree,
            &mut self.renderer,
            &theme,
            &iced::advanced::renderer::Style {
                text_color: base.text_color,
            },
            iced::advanced::Layout::new(&self.node),
            iced::mouse::Cursor::Unavailable,
            &viewport,
        );
        self.renderer.screenshot(size, 1.0, base.background_color)
    }
}

/// A text shown, see [`Laid::texts`].
#[derive(Debug, Clone)]
pub(crate) struct Shown {
    pub text: String,
    pub bounds: iced::Rectangle,
    /// The part of `bounds` its scrollables let show, if any.
    pub visible: Option<iced::Rectangle>,
}

impl Shown {
    /// Whether its scrollables show all of it, to a rounding error.
    pub(crate) fn whole(&self) -> bool {
        self.visible
            .is_none_or(|visible| visible.height >= self.bounds.height - 0.01)
    }

    /// Whether its scrollables hide all of it.
    pub(crate) fn hidden(&self) -> bool {
        self.visible.is_some_and(|visible| visible.height <= 0.0)
    }
}

/// The part of `bounds` inside `clip`, empty where it's outside.
fn within(clip: iced::Rectangle, bounds: iced::Rectangle) -> iced::Rectangle {
    clip.intersection(&bounds)
        .unwrap_or(iced::Rectangle::new(bounds.position(), iced::Size::ZERO))
}

/// An operation finding the texts shown.
#[derive(Default)]
struct Find {
    /// How far the scrollables entered have scrolled their content.
    offset: iced::Vector,
    /// Where the scrollables entered show their content.
    clip: Option<iced::Rectangle>,
    /// The scrollable just met, entered by the next traverse.
    entering: Option<(iced::Vector, iced::Rectangle)>,
    shown: Vec<Shown>,
}

impl iced::advanced::widget::Operation for Find {
    fn traverse(
        &mut self,
        operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<()>),
    ) {
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
        _id: Option<&iced::advanced::widget::Id>,
        bounds: iced::Rectangle,
        _content_bounds: iced::Rectangle,
        translation: iced::Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        self.entering = Some((translation, bounds));
    }

    fn text(
        &mut self,
        _id: Option<&iced::advanced::widget::Id>,
        bounds: iced::Rectangle,
        text: &str,
    ) {
        let bounds = bounds - self.offset;
        let visible = self.clip.map(|clip| within(clip, bounds));
        self.shown.push(Shown {
            text: text.to_owned(),
            bounds,
            visible,
        });
    }
}
