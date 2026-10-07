//! The thumbnail each save writes with the design, for the welcome
//! screen: the bodies and visible sketches' curves of the last model the committed document
//! regenerated to (see [`MeshFeed::committed`]), with the curves of the sketch being edited,
//! which that model leaves out, from where Home looks,
//! framed to fit and cropped, on nothing, in each theme's colours. Only the viewport has the GPU,
//! so it's rendered there on the next frame (see
//! [`varde_view::ThumbnailRequest`]) and comes back as a message, and a
//! save waits for it as it waits for the edits waiting on the solver (see
//! [`Saves::waiting`]). One that fails, or takes longer than
//! [`PATIENCE`], as when the window isn't drawn, is saved without: it
//! never holds a save back for long.
//!
//! [`MeshFeed::committed`]: super::feed::MeshFeed::committed
//! [`Saves::waiting`]: super::save::Saves::waiting

use std::sync::Arc;
use std::time::Duration;

use iced::futures::channel::oneshot;
use iced::time::Instant;
use varde_document::{FeatureId, FeatureKind, Generation};
use varde_io::thumbnail::Thumbnail;
use varde_kernel::{RenderLines, RenderMesh};
use varde_render::Camera;
use varde_view::{ThumbnailImages, ThumbnailRequest};

use super::Doc;
use crate::{Files, Next};

/// How long a save waits for its thumbnail before it goes without: a
/// frame is all it takes, unless the window isn't drawn.
const PATIENCE: Duration = Duration::from_secs(2);

/// The document's thumbnails: the one being rendered, if one is, and the
/// last one rendered.
#[derive(Default)]
pub(crate) struct Thumbnails {
    rendering: Option<Rendering>,
    rendered: Option<Rendered>,
    /// Tags the next thumbnail asked for.
    next: u64,
    /// The answer to the thumbnail asked for last, until the app takes it
    /// to wait for, see [`Doc::take_thumbnail`].
    answer: Option<(u64, oneshot::Receiver<Option<ThumbnailImages>>)>,
}

/// A thumbnail being rendered.
struct Rendering {
    tag: u64,
    of: Of,
    request: Arc<ThumbnailRequest>,
    since: Instant,
}

/// A thumbnail rendered, or given up on.
struct Rendered {
    of: Of,
    image: Option<Thumbnail>,
}

/// What a thumbnail shows: a mesh and sketch curves, its parts as opaque as `opacity` says
/// and in the colours `tints` gives them.
#[derive(Clone)]
struct Of {
    mesh: Arc<RenderMesh>,
    /// The model's sketch curves, and with them the edited sketch's.
    model_sketches: Arc<RenderLines>,
    /// The sketch being edited, if its curves are added, and the
    /// generation of the document they're of.
    edited: Option<(FeatureId, Generation)>,
    sketches: Arc<RenderLines>,
    opacity: Arc<[f32]>,
    tints: Arc<[Option<varde_render::BodyTint>]>,
}

impl Of {
    /// Whether it shows what `other` does. Meshes are compared as the
    /// renderer does, by their `Arc`s: the regeneration lane hands an
    /// unchanged model back as the same one, natively.
    fn is(&self, other: &Of) -> bool {
        Arc::ptr_eq(&self.mesh, &other.mesh)
            && Arc::ptr_eq(&self.model_sketches, &other.model_sketches)
            && self.edited == other.edited
            && self.opacity == other.opacity
            && self.tints == other.tints
    }
}

impl Doc {
    /// What the thumbnail shows now: the committed model's mesh and
    /// sketch curves, those of the sketch being edited added (if it's
    /// visible), its
    /// parts as opaque and in the colours their bodies are. `None` before
    /// there's a model.
    fn thumbnail_of(&self) -> Option<Of> {
        let super::feed::Shown {
            mesh,
            sketches,
            parts,
        } = self.feed.committed()?;
        let document = self.editor.document();
        let opacity = parts
            .iter()
            .map(|&body| document.body(body).map_or(1.0, |body| body.opacity.alpha()));
        let tints =
            (parts.iter()).map(|&body| (document.body(body)?.color).map(varde_view::body_tint));
        let edited = self.sketch.as_ref().and_then(|session| {
            let feature = document.feature(session.feature)?;
            let FeatureKind::Sketch { sketch, .. } = &feature.kind else {
                return None;
            };
            feature
                .visible
                .then_some((session.feature, sketch, session.placement))
        });
        let mut lines = sketches.clone();
        let edited = edited.and_then(|(feature, sketch, placement)| {
            let mut with = RenderLines::clone(&lines);
            // Too many points to draw: the model's alone.
            varde_regen::push_sketch_lines(&mut with, sketch, placement).ok()?;
            lines = Arc::new(with);
            Some((feature, self.editor.generation()))
        });
        Some(Of {
            mesh: mesh.clone(),
            model_sketches: sketches.clone(),
            edited,
            sketches: lines,
            opacity: opacity.collect(),
            tints: tints.collect(),
        })
    }

    /// Whether a save must wait for the thumbnail, see the module docs:
    /// while one is rendered, or once one is asked for now, unless the one
    /// rendered last shows what one would now, or there's nothing to show.
    pub(super) fn thumbnail_waits(&mut self) -> bool {
        if self.thumbnails.rendering.is_some() {
            return true;
        }
        let Some(of) = self.thumbnail_of() else {
            self.thumbnails.rendered = None;
            return false;
        };
        if (self.thumbnails.rendered.as_ref()).is_some_and(|rendered| rendered.of.is(&of)) {
            return false;
        }
        let tag = self.thumbnails.next;
        self.thumbnails.next += 1;
        let (send, answer) = oneshot::channel();
        let request = ThumbnailRequest::new(
            of.mesh.clone(),
            of.sketches.clone(),
            of.opacity.clone(),
            of.tints.clone(),
            &Camera::default(),
            move |image| {
                // Unless the document went meanwhile.
                let _ = send.send(image);
            },
        );
        let Some(request) = request else {
            // Nothing to frame: there's no thumbnail to wait for.
            self.thumbnails.rendered = Some(Rendered { of, image: None });
            return false;
        };
        self.thumbnails.rendering = Some(Rendering {
            tag,
            of,
            request,
            since: Instant::now(),
        });
        self.thumbnails.answer = Some((tag, answer));
        true
    }

    /// The thumbnail the next save writes: the one rendered last, if it
    /// rendered, once [`Doc::thumbnail_waits`] doesn't wait.
    pub(super) fn thumbnail(&self) -> Option<Thumbnail> {
        (self.thumbnails.rendered.as_ref())?.image.clone()
    }

    /// The thumbnail for the viewport to render, while one is waited for.
    pub(crate) fn thumbnail_request(&self) -> Option<&Arc<ThumbnailRequest>> {
        self.thumbnails
            .rendering
            .as_ref()
            .map(|rendering| &rendering.request)
    }

    /// The tag of the thumbnail being rendered, if one is.
    #[cfg(test)]
    pub(crate) fn thumbnail_tag(&self) -> Option<u64> {
        self.thumbnails
            .rendering
            .as_ref()
            .map(|rendering| rendering.tag)
    }

    /// Whether a thumbnail is being rendered: frames are drawn meanwhile,
    /// which render it and, on the web, map it to be read, and the timer
    /// ticks, to give up on it.
    pub(crate) fn rendering_thumbnail(&self) -> bool {
        self.thumbnails.rendering.is_some()
    }

    /// The answer to the thumbnail asked for last, tagged, for the app to
    /// wait for and hand back to [`Doc::thumbnail_rendered`]. Once.
    pub(crate) fn take_thumbnail(
        &mut self,
    ) -> Option<(u64, oneshot::Receiver<Option<ThumbnailImages>>)> {
        self.thumbnails.answer.take()
    }

    /// The thumbnail tagged `tag` came back as `image`, or `None` if it
    /// couldn't be rendered: the saves waiting for it go, unless they wait
    /// for the solver too, or the model changed meanwhile, which asks for
    /// another.
    pub(crate) fn thumbnail_rendered(
        &mut self,
        cx: &mut Files,
        tag: u64,
        image: Option<Thumbnail>,
    ) -> Next {
        match self
            .thumbnails
            .rendering
            .take_if(|rendering| rendering.tag == tag)
        {
            Some(rendering) => self.thumbnail_settled(cx, rendering.of, image),
            None => Next::Stay,
        }
    }

    /// Gives up on the thumbnail being rendered if it's been longer than
    /// [`PATIENCE`] by `now`: the saves waiting for it go without.
    pub(crate) fn thumbnail_overdue(&mut self, cx: &mut Files, now: Instant) -> Next {
        let overdue = |rendering: &mut Rendering| now.duration_since(rendering.since) >= PATIENCE;
        match self.thumbnails.rendering.take_if(overdue) {
            Some(rendering) => {
                log::warn!("Saving without a thumbnail: it took too long to draw");
                self.thumbnail_settled(cx, rendering.of, None)
            }
            None => Next::Stay,
        }
    }

    /// The thumbnail of `of` is `image`, or none: the saves waiting go.
    /// One that failed isn't kept, so the next save tries again.
    fn thumbnail_settled(&mut self, cx: &mut Files, of: Of, image: Option<Thumbnail>) -> Next {
        let failed = image.is_none();
        self.thumbnails.rendered = Some(Rendered { of, image });
        let next = self.proposals_settled(cx);
        if failed {
            self.thumbnails.rendered = None;
        }
        next
    }
}
