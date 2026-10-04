//! A design's thumbnail, rendered as it's saved for the welcome screen's
//! cards, once in each theme's colours, so a card shows the one of the
//! theme it's in: the viewport draws them on its next frame, offscreen,
//! with the renderer it draws the model with (see
//! `varde_render::render_preview`), since only it has the GPU.

use std::fmt;
use std::sync::{Arc, Mutex};

use varde_kernel::RenderMesh;
use varde_render::{Camera, Colors, PreviewImage, PreviewShot};

use crate::theme::Mode;

/// How large a thumbnail is shown at most, in logical pixels: what a
/// recent file's card has inside its padding at its widest.
pub const THUMBNAIL_ROOM: [u32; 2] = [194, 126];

/// Physical pixels to a logical one in a thumbnail: rendered at twice
/// the size it's shown at, for screens that scale by two.
pub const THUMBNAIL_SCALE: u32 = 2;

/// The margin left around the model in a thumbnail, in its pixels: room
/// for the outline's edges, drawn across the model's silhouette.
const MARGIN: u32 = 6;

/// A thumbnail for the viewport to render, taken by its first frame
/// drawn: `mesh`, its parts as opaque as `opacity` says, as `shot`
/// frames it, in each theme's colours. What it's handed to is called with
/// the pixels read back, or with `None` should that fail, and dropped
/// uncalled should it not be drawn at all.
pub struct ThumbnailRequest {
    pub mesh: Arc<RenderMesh>,
    pub opacity: Arc<[f32]>,
    pub shot: PreviewShot,
    done: Mutex<Option<Done>>,
}

/// A thumbnail's pixels as read back, in each theme's colours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailImages {
    pub light: PreviewImage,
    pub dark: PreviewImage,
}

/// What takes a thumbnail's pixels.
type Done = Box<dyn FnOnce(Option<ThumbnailImages>) + Send>;

impl ThumbnailRequest {
    /// The thumbnail of `mesh`, its parts as opaque as `opacity` says,
    /// looking from where the home camera does, framed to fit
    /// [`THUMBNAIL_ROOM`] at [`THUMBNAIL_SCALE`] and cropped to the model,
    /// handing its pixels to `done`. `None` for a mesh with nothing to
    /// frame.
    pub fn new(
        mesh: Arc<RenderMesh>,
        opacity: Arc<[f32]>,
        home: &Camera,
        done: impl FnOnce(Option<ThumbnailImages>) + Send + 'static,
    ) -> Option<Arc<ThumbnailRequest>> {
        let room = THUMBNAIL_ROOM.map(|side| side * THUMBNAIL_SCALE);
        let shot = varde_render::frame(&mesh, home, room, MARGIN)?;
        Some(Arc::new(ThumbnailRequest {
            mesh,
            opacity,
            shot,
            done: Mutex::new(Some(Box::new(done))),
        }))
    }

    /// The colours it's rendered in, one image each: the light theme's
    /// model, then the dark's.
    pub(crate) const COLORS: [Colors; 2] =
        [Mode::Light.palette().scene, Mode::Dark.palette().scene];

    /// What takes its pixels, one image for each of [`Self::COLORS`], the
    /// first time it's asked for: it's rendered once.
    pub(crate) fn take(&self) -> Option<impl FnOnce(Option<Vec<PreviewImage>>) + Send + 'static> {
        let done = self.done.lock().ok()?.take()?;
        Some(move |images: Option<Vec<PreviewImage>>| {
            let images = images.and_then(|images| {
                let [light, dark] = <[PreviewImage; 2]>::try_from(images).ok()?;
                Some(ThumbnailImages { light, dark })
            });
            done(images);
        })
    }
}

impl fmt::Debug for ThumbnailRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThumbnailRequest")
            .field("shot", &self.shot)
            .finish_non_exhaustive()
    }
}
