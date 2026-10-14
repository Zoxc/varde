//! The sketch tools: [`Tool`] and how a click with one goes, [`ToolClick`].

use glam::DVec2;
use varde_sketch::{Id, Selectable, Sketch};

use crate::icons;
use crate::{Inference, Snap, Target};

/// Declares [`Tool`] from rows of a variant, its label and its icon, in
/// the order of [`Tool::ALL`], so a tool can't be left out of any of them.
macro_rules! tools {
    ($($(#[doc = $doc:literal])* $tool:ident => $label:literal, $icon:expr;)*) => {
        /// A tool drawing in a sketch.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Tool {
            $($(#[doc = $doc])* $tool,)*
        }

        impl Tool {
            /// Every tool, drawing ones first.
            pub const ALL: [Tool; [$(Tool::$tool),*].len()] = [$(Tool::$tool),*];

            /// Its icon.
            pub(crate) fn icon(self) -> icons::Icon {
                use icons::Icon;
                match self {
                    $(Tool::$tool => $icon,)*
                }
            }

            /// The tool as the user sees it.
            pub fn label(self) -> &'static str {
                match self {
                    $(Tool::$tool => $label,)*
                }
            }
        }
    };
}

tools! {
    /// Connected lines, click after click.
    Line => "Line", Icon::Line;
    /// Two opposite corners, or the centre and a corner: four lines, held
    /// horizontal and vertical.
    Rectangle => "Rectangle", Icon::Rectangle;
    /// Centre, then a point on the circle.
    Circle => "Circle", Icon::Circle;
    /// Start, end, then a point on the arc.
    Arc => "Arc", Icon::Arc;
    /// The centre, then a corner: equal lines with their corners on a
    /// construction circle.
    Polygon => "Polygon", Icon::Polygon;
    /// A smooth curve through the points clicked, or by them as control
    /// points, until a double-click, `Enter` or its first point again.
    Spline => "Spline", Icon::Spline;
    /// A lone point.
    Point => "Point", Icon::Point;
    /// Measures what's clicked: see `dimension::measure`.
    Dimension => "Dimension", Icon::Dimension;
    /// Takes away the piece of a curve clicked, between the curves
    /// cutting it.
    Trim => "Trim", Icon::Trim;
    /// Lengthens a line, an arc or a spline clicked, at the end nearer the
    /// click, to the next curve it meets.
    Extend => "Extend", Icon::Extend;
    /// Copies the chain of curves clicked (or selected) a distance to one
    /// side, the cursor's or typed, tied to it by that distance.
    Offset => "Offset", Icon::Offset;
    /// Mirrors geometry about a line: the selection, or what's clicked
    /// until `Enter`, then the line clicked.
    Mirror => "Mirror", Icon::Mirror;
    /// Rounds the corner clicked, where two lines end, with an arc tangent
    /// to both, of the radius the cursor's or typed.
    Fillet => "Fillet", Icon::Fillet;
    /// Cuts the corner clicked with a line across it, as far back as the
    /// cursor's, or the distances or the distance and the angle typed.
    Chamfer => "Chamfer", Icon::Chamfer;
    /// Picks what to project onto the sketch's plane, outside it: the
    /// model's edges and corners, and other sketches' curves and points,
    /// made before the sketch.
    Project => "Project", Icon::Project;
    /// Picks what to cut with the sketch's plane, outside it: the model's
    /// faces and edges, made before the sketch.
    Intersect => "Intersect", Icon::Intersect;
}

impl Tool {
    /// Those the sketch toolbar shows, in its order, as the UI mock's
    /// sketch bar: the rest are on the rail, as these are too.
    pub const BAR: [Tool; 7] = [
        Tool::Line,
        Tool::Rectangle,
        Tool::Circle,
        Tool::Arc,
        Tool::Trim,
        Tool::Offset,
        Tool::Dimension,
    ];

    /// Whether it draws shapes, which `X` can make construction geometry
    /// and which snap, rather than measuring or changing them.
    pub fn draws(self) -> bool {
        !matches!(
            self,
            Tool::Dimension
                | Tool::Trim
                | Tool::Extend
                | Tool::Offset
                | Tool::Mirror
                | Tool::Fillet
                | Tool::Chamfer
                | Tool::Project
                | Tool::Intersect
        )
    }

    /// Whether it picks outside the sketch, in the model and other
    /// sketches, rather than in it: Project and Intersect. While it's in
    /// use the sketch's own geometry isn't hit.
    pub fn picks_outside(self) -> bool {
        matches!(self, Tool::Project | Tool::Intersect)
    }

    /// Whether it works on a corner where two lines end: Fillet and
    /// Chamfer.
    pub fn corners(self) -> bool {
        matches!(self, Tool::Fillet | Tool::Chamfer)
    }

    /// Whether it picks something with a click, then places what it makes
    /// of it through the cursor, where the button's let go: Offset its
    /// copy of a chain, Fillet and Chamfer theirs on a corner.
    pub fn places(self) -> bool {
        matches!(self, Tool::Offset | Tool::Fillet | Tool::Chamfer)
    }

    /// What a tool that [`places`](Tool::places) picks with a click on
    /// `hit` at `at` in `sketch`: Offset the chain the curve is in
    /// ([`Sketch::chain_of`]), Fillet and Chamfer the corner at the point,
    /// as it and its lines ([`Sketch::corner_lines`]). Empty for nothing
    /// to pick, or another tool.
    pub fn pick(self, sketch: &Sketch, hit: Id, at: DVec2) -> Vec<Id> {
        match self {
            Tool::Offset => sketch.chain_of(hit),
            _ if self.corners() => sketch
                .corner_lines(hit, at)
                .map(|[a, b]| vec![hit, a, b])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}

/// A click in the sketch being edited with its tool, from the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolClick {
    /// Where, in sketch coordinates, snapped (see [`Snap`]), within
    /// `MAX_COORD` of zero.
    pub at: DVec2,
    /// What it snapped to, if anything: a drawing tool ties its point
    /// there to it.
    pub target: Option<Target>,
    /// How the shape runs from its start, if that snapped: a drawing tool
    /// holds it so.
    pub inference: Option<Inference>,
    /// The point or curve under the cursor, if there is one: what the
    /// Dimension tool picks. For Trim and Extend, and Offset picking its
    /// chain, the curve under it, points aside, for Mirror's line to
    /// mirror about the line (see [`Drawing::about`](crate::Drawing::about)), and for Fillet
    /// and Chamfer picking their corner the point where lines make one.
    pub hit: Option<Selectable>,
    /// A pixel's size at `at`, in sketch units, above zero: a shape
    /// smaller than that can't have been meant, and is refused.
    pub pixel: f64,
    /// Whether it's the second click of a double-click.
    pub double: bool,
    /// Whether the reference modifier (`Alt`) is held, which places the
    /// Dimension tool's dimension as a reference.
    pub reference: bool,
}

impl ToolClick {
    /// Where it goes, and what snapped it there.
    pub fn snap(&self) -> Snap {
        Snap {
            at: self.at,
            target: self.target,
            inference: self.inference,
        }
    }

    /// The same click, going where `snap` says, snapped as it says.
    pub fn snapped(self, snap: Snap) -> ToolClick {
        ToolClick {
            at: snap.at,
            target: snap.target,
            inference: snap.inference,
            ..self
        }
    }

    /// The point of the sketch's it snapped to, if any: the origin, or one
    /// a drawing tool takes as its own.
    pub fn point(&self) -> Option<Id> {
        match self.target {
            Some(Target::Point(id)) => Some(id),
            _ => None,
        }
    }
}
