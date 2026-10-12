//! How finely the model is drawn for the view: [`Detail`] and [`Sight`].

use serde::{Deserialize, Serialize};
use varde_document::BodyId;

/// How finely a model is drawn for the view: its chords within
/// `2^level` mm of the curves, as the app picks from the camera's zoom
/// ([`Display::for_view`](varde_kernel_compat::Display::for_view)). Levels apart so a camera moving a little
/// doesn't ask for the model again; past [`Detail::MIN`] and
/// [`Detail::MAX`] they're taken as those.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Detail(pub i8);

impl Detail {
    /// The finest level: chords within about a micrometre, finer than the
    /// fit tolerance can be anyway.
    pub const MIN: i8 = -20;
    /// The coarsest level: chords within a metre, where the turn per
    /// segment decides alone.
    pub const MAX: i8 = 10;

    /// The chord error asked for, in mm.
    pub fn chord(self) -> f64 {
        2f64.powi(i32::from(self.0.clamp(Self::MIN, Self::MAX)))
    }
}

/// How finely each visible body is drawn for the view: a [`Detail`]
/// level per body the app has seen, from how near the eye its box is and
/// whether it's in view at all, and one for the rest. The app picks the
/// levels from the camera and the boxes of the model shown, and asks
/// again only once one changes ([`Sight::revision`] tells them apart).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sight {
    /// Tells this sight from the app's others: an answer carries it back
    /// ([`Response::sight`](crate::Response::sight)).
    pub revision: u32,
    /// The level of a body not in `bodies`, one the app hasn't seen: from
    /// the view's height at the camera's target.
    pub view: Detail,
    /// Each body's level, sorted by body: `None` for one out of view,
    /// drawn within a share of its size. A body listed twice, or the
    /// list unsorted, only draws at another listed level.
    pub bodies: Vec<(BodyId, Option<Detail>)>,
}

impl Sight {
    /// The level `body` is drawn at, `None` for within a share of its
    /// size.
    pub fn level(&self, body: BodyId) -> Option<Detail> {
        match (self.bodies).binary_search_by_key(&body, |&(body, _)| body) {
            Ok(at) => self.bodies[at].1,
            Err(_) => Some(self.view),
        }
    }

    /// The sight with every level `steps` coarser, no coarser than
    /// [`Detail::MAX`]; `None` once they all are, as a sight that asks
    /// for nothing finer than its bodies' shares would.
    pub fn coarser(&self, steps: i8) -> Option<Sight> {
        let coarser = |Detail(level): Detail| {
            Detail(
                level
                    .clamp(Detail::MIN, Detail::MAX)
                    .saturating_add(steps)
                    .min(Detail::MAX),
            )
        };
        let finest = (self.bodies.iter().filter_map(|&(_, level)| level))
            .chain([self.view])
            .map(|Detail(level)| level)
            .min()?;
        (finest < Detail::MAX).then(|| Sight {
            revision: self.revision,
            view: coarser(self.view),
            bodies: (self.bodies.iter())
                .map(|&(body, level)| (body, level.map(coarser)))
                .collect(),
        })
    }
}

/// How many levels coarser a sight is drawn at each time its meshes
/// pass [`RenderMesh`](varde_kernel_compat::RenderMesh)'s limits: a factor of eight in chord, about
/// three in triangles along a curve.
pub(crate) const COARSER_STEPS: i8 = 3;
