//! The failures' geometry the viewport draws: which ones, picked by the
//! app (see its `Doc::shown_errors`), and handed to the renderer as
//! [`ErrorParts`].

use std::any::Any;
use std::sync::{Arc, Weak};

use varde_regen::ErrorGeometry;
use varde_render::ErrorParts;

/// The geometry of the failures shown, each with the `Weak` the renderer
/// keys its upload by: made once when what's shown changes, so a frame
/// only borrows its parts and the renderer uploads nothing again while
/// the same geometry shows.
#[derive(Debug, Default)]
pub struct ShownErrors {
    errors: Vec<(Arc<ErrorGeometry>, Weak<dyn Any + Send + Sync>)>,
}

impl ShownErrors {
    /// Shows `geometry`, in order. Each source is the live `Arc`'s
    /// `Weak`, never a dangling one: those compare equal whatever they
    /// stood for, so a change wouldn't be uploaded.
    pub fn new(geometry: impl IntoIterator<Item = Arc<ErrorGeometry>>) -> Self {
        let errors = (geometry.into_iter())
            .map(|geometry| {
                let erased: Arc<dyn Any + Send + Sync> = geometry.clone();
                let source = Arc::downgrade(&erased);
                (geometry, source)
            })
            .collect();
        Self { errors }
    }

    /// The geometry shown, in order.
    pub fn geometry(&self) -> impl Iterator<Item = &Arc<ErrorGeometry>> {
        self.errors.iter().map(|(geometry, _)| geometry)
    }

    /// Whether it shows nothing.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Whether it shows the very `Arc`s of `geometry`, in order.
    pub fn shows(&self, geometry: &[Arc<ErrorGeometry>]) -> bool {
        self.errors.len() == geometry.len()
            && (self.geometry().zip(geometry)).all(|(shown, other)| Arc::ptr_eq(shown, other))
    }

    /// What the renderer draws of it, see `Frame::errors`.
    pub(crate) fn parts(&self) -> Vec<ErrorParts<'_>> {
        (self.errors.iter())
            .map(|(geometry, source)| ErrorParts {
                mesh: geometry.mesh(),
                lines: geometry.lines(),
                points: geometry.points(),
                source: source.clone(),
            })
            .collect()
    }
}
