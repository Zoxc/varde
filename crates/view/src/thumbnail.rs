//! A design's thumbnail, rendered as it's saved for the welcome screen's
//! cards, once in each theme's colours, so a card shows the one of the
//! theme it's in: the viewport draws them on its next frame, offscreen,
//! with the renderer it draws the model with (see
//! `varde_render::render_preview`), since only it has the GPU. The MCP
//! server's `render` draws its picture the same way ([`PreviewRequest`]).

use std::fmt;
use std::sync::{Arc, Mutex};

use varde_kernel::{RenderLines, RenderMesh};
use varde_render::{BodyTint, Camera, Colors, Highlights, PreviewImage, PreviewShot};

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

/// A picture for the viewport to render offscreen, taken by its first
/// frame drawn (see `varde_render::render_preview`): a design's thumbnail
/// ([`PreviewRequest::thumbnail`]) or the MCP server's `render`
/// ([`PreviewRequest::render`]). `mesh` and the finished `sketches`'
/// curves, its parts as opaque as `opacity` says (0 for those left out,
/// see `varde_render::frame`) and in the colours `tints` gives
/// them, with `selected_faces` and `highlights` drawn selected, as `shot`
/// frames it. What it's handed to is called with the pixels read back,
/// or with `None` should that fail, and dropped uncalled should it not be
/// drawn at all.
pub struct PreviewRequest {
    pub mesh: Arc<RenderMesh>,
    /// The visible sketches' curves, drawn with the model, so a design of
    /// sketches alone has a thumbnail too.
    pub sketches: Arc<RenderLines>,
    pub opacity: Arc<[f32]>,
    /// The colour of each of its parts, see `varde_render::Frame::tints`.
    pub tints: Arc<[Option<BodyTint>]>,
    pub shot: PreviewShot,
    /// The faces drawn selected, by their ids in the mesh.
    pub selected_faces: Vec<u32>,
    /// The edges and vertices drawn selected.
    pub highlights: Arc<Highlights>,
    /// Whether it's drawn in each of [`PreviewRequest::THEMES`], or once
    /// in the colours the viewport draws in now.
    pub(crate) themed: bool,
    /// Physical pixels to a logical one.
    pub(crate) scale: f32,
    done: Mutex<Option<Done>>,
}

/// A thumbnail's pixels as read back, in each theme's colours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailImages {
    pub light: PreviewImage,
    pub dark: PreviewImage,
}

/// What takes a picture's pixels, an image for each colouring drawn.
type Done = Box<dyn FnOnce(Option<Vec<PreviewImage>>) + Send>;

/// Physical pixels to a logical one in an MCP render: lines as wide as a
/// thumbnail's.
pub const RENDER_SCALE: f32 = THUMBNAIL_SCALE as f32;

impl PreviewRequest {
    /// The thumbnail of `mesh` and `sketches`, its parts as opaque as `opacity` says and
    /// in the colours `tints` gives them,
    /// looking from where the home camera does, framed to fit
    /// [`THUMBNAIL_ROOM`] at [`THUMBNAIL_SCALE`] and cropped to the model,
    /// in each theme's colours, handing its pixels to `done`. `None` with
    /// nothing to frame.
    pub fn thumbnail(
        mesh: Arc<RenderMesh>,
        sketches: Arc<RenderLines>,
        opacity: Arc<[f32]>,
        tints: Arc<[Option<BodyTint>]>,
        home: &Camera,
        done: impl FnOnce(Option<ThumbnailImages>) + Send + 'static,
    ) -> Option<Arc<PreviewRequest>> {
        let shot = thumbnail_shot(&mesh, &sketches, home)?;
        Some(Arc::new(PreviewRequest {
            mesh,
            sketches,
            opacity,
            tints,
            shot,
            selected_faces: Vec::new(),
            highlights: Arc::default(),
            themed: true,
            scale: THUMBNAIL_SCALE as f32,
            done: Mutex::new(Some(Box::new(move |images: Option<Vec<PreviewImage>>| {
                let images = images.and_then(|images| {
                    let [light, dark] = <[PreviewImage; 2]>::try_from(images).ok()?;
                    Some(ThumbnailImages { light, dark })
                });
                done(images);
            }))),
        }))
    }

    /// The MCP server's `render` of `mesh` as `shot` frames it, without
    /// sketches, once in the theme the viewport draws in, with
    /// `selected_faces` and `highlights` drawn selected, handing its
    /// pixels to `done`.
    pub fn render(
        mesh: Arc<RenderMesh>,
        opacity: Arc<[f32]>,
        tints: Arc<[Option<BodyTint>]>,
        shot: PreviewShot,
        selected_faces: Vec<u32>,
        highlights: Arc<Highlights>,
        done: impl FnOnce(Option<PreviewImage>) + Send + 'static,
    ) -> Arc<PreviewRequest> {
        Arc::new(PreviewRequest {
            mesh,
            sketches: Arc::default(),
            opacity,
            tints,
            shot,
            selected_faces,
            highlights,
            themed: false,
            scale: RENDER_SCALE,
            done: Mutex::new(Some(Box::new(move |images: Option<Vec<PreviewImage>>| {
                done(images.and_then(|images| images.into_iter().next()));
            }))),
        })
    }

    /// The colours a thumbnail is rendered in, one image each: the light
    /// theme's model, then the dark's.
    pub const THEMES: [Colors; 2] = [Mode::Light.palette().scene, Mode::Dark.palette().scene];

    /// What takes its pixels, the first time it's asked for: it's
    /// rendered once.
    pub(crate) fn take(&self) -> Option<Done> {
        self.done.lock().ok()?.take()
    }

    /// Hands `images` to what takes its pixels, as the viewport would:
    /// for tests, which have no GPU to draw with. Once, as drawing it is.
    #[cfg(any(test, feature = "probe"))]
    pub fn answer_for_tests(&self, images: Option<Vec<PreviewImage>>) {
        if let Some(done) = self.take() {
            done(images);
        }
    }
}

/// How a thumbnail frames `mesh` and `sketches`, looking from where `home`
/// does: to fit [`THUMBNAIL_ROOM`] at [`THUMBNAIL_SCALE`], cropped to them.
/// `None` with nothing to frame.
pub fn thumbnail_shot(
    mesh: &RenderMesh,
    sketches: &RenderLines,
    home: &Camera,
) -> Option<PreviewShot> {
    let room = THUMBNAIL_ROOM.map(|side| side * THUMBNAIL_SCALE);
    varde_render::frame(mesh, sketches, None, home, room, MARGIN)
}

impl fmt::Debug for PreviewRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewRequest")
            .field("shot", &self.shot)
            .finish_non_exhaustive()
    }
}
