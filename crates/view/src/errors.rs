//! The failures' geometry the viewport draws: which ones, picked by the
//! app (see its `Doc::shown_errors`), and handed to the renderer as
//! [`ErrorParts`].

use std::any::Any;
use std::sync::{Arc, LazyLock, Weak};

use varde_kernel::RenderLines;
use varde_regen::ErrorGeometry;
use varde_render::ErrorParts;

/// A failure's geometry to show, and whether its curves are drawn.
#[derive(Debug, Clone)]
pub struct ShownError {
    pub geometry: Arc<ErrorGeometry>,
    /// Whether its curves are drawn: not while the sketch whose curves
    /// they are is edited, which marks those curves itself; its points
    /// and patches are drawn either way.
    pub lines: bool,
}

impl ShownError {
    /// All of `geometry`.
    pub fn whole(geometry: Arc<ErrorGeometry>) -> Self {
        Self {
            geometry,
            lines: true,
        }
    }

    /// Whether it's the very `Arc` of `other`, drawn the same.
    fn is(&self, other: &ShownError) -> bool {
        Arc::ptr_eq(&self.geometry, &other.geometry) && self.lines == other.lines
    }
}

/// The geometry of the failures shown, each with the `Weak` the renderer
/// keys its upload by: made once when what's shown changes, so a frame
/// only borrows its parts and the renderer uploads nothing again while
/// the same geometry shows.
#[derive(Debug, Default)]
pub struct ShownErrors {
    errors: Vec<(ShownError, Weak<dyn Any + Send + Sync>)>,
}

/// The curves of a failure drawn without them: none.
static NO_LINES: LazyLock<RenderLines> = LazyLock::new(RenderLines::default);

impl ShownErrors {
    /// Shows `errors`, in order. Each source is the live `Arc`'s `Weak`,
    /// never a dangling one: those compare equal whatever they stood
    /// for, so a change wouldn't be uploaded. One drawn without its curves
    /// takes a `Weak` of its own instead, made here, so that the same
    /// geometry drawn whole before or after is uploaded again; the
    /// renderer holding it keeps its allocation from being reused.
    pub fn new(errors: impl IntoIterator<Item = ShownError>) -> Self {
        let errors = (errors.into_iter())
            .map(|shown| {
                let erased: Arc<dyn Any + Send + Sync> = if shown.lines {
                    shown.geometry.clone()
                } else {
                    Arc::new(())
                };
                let source = Arc::downgrade(&erased);
                (shown, source)
            })
            .collect();
        Self { errors }
    }

    /// The failures shown, in order.
    pub fn shown(&self) -> impl Iterator<Item = &ShownError> {
        self.errors.iter().map(|(shown, _)| shown)
    }

    /// Whether it shows nothing.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Whether it shows the very `Arc`s of `errors`, in order, drawn the
    /// same.
    pub fn shows(&self, errors: &[ShownError]) -> bool {
        self.errors.len() == errors.len()
            && (self.shown().zip(errors)).all(|(shown, other)| shown.is(other))
    }

    /// What the renderer draws of it, see `Frame::errors`.
    pub(crate) fn parts(&self) -> Vec<ErrorParts<'_>> {
        (self.errors.iter())
            .map(|(shown, source)| ErrorParts {
                mesh: shown.geometry.mesh(),
                lines: if shown.lines {
                    shown.geometry.lines()
                } else {
                    &NO_LINES
                },
                points: shown.geometry.points(),
                source: source.clone(),
                halo_only: false,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_geometry_drawn_otherwise_is_another_source() {
        let geometry = Arc::new(ErrorGeometry::default());
        let whole = ShownError::whole(geometry.clone());
        let without = ShownError {
            geometry: geometry.clone(),
            lines: false,
        };
        let shown = ShownErrors::new([whole.clone()]);
        assert!(shown.shows(std::slice::from_ref(&whole)));
        assert!(!shown.shows(std::slice::from_ref(&without)));
        let other = ShownErrors::new([without.clone()]);
        let (a, b) = (shown.parts(), other.parts());
        assert!(!Weak::ptr_eq(&a[0].source, &b[0].source));
        // Whole, keyed by the geometry's own `Arc`, the same each time.
        let again = ShownErrors::new([whole]);
        assert!(Weak::ptr_eq(&a[0].source, &again.parts()[0].source));
        // Drawn without its curves, it has none.
        assert!(std::ptr::eq(b[0].lines, &*NO_LINES));
    }
}
