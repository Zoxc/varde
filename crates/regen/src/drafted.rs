//! A feature being set up, not committed ([`Draft`]), and what
//! regenerating with it found ([`Drafted`] and the `*Found` types).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use varde_document::{BodyId, FeatureId, FeatureKind};

use crate::{ErrorGeometry, Evaluation, bounds};

/// A feature being set up, new or edited, that isn't committed (an
/// extrude, a revolve): a request answers with it applied, as
/// [`Command::AddFeature`](varde_document::Command::AddFeature) (with [`Operation::NewBody`] holding
/// [`BodyId::NEW`]) or [`Command::SetFeature`](varde_document::Command::SetFeature) would apply it, for a
/// preview.
///
/// [`Operation::NewBody`]: varde_document::Operation::NewBody
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    /// Counted up by the app with every change to the draft, so the
    /// newest answer can be told apart from older ones of the same
    /// generation.
    pub revision: u64,
    /// The feature being edited, or `None` for a new one.
    pub feature: Option<FeatureId>,
    /// What it is to be. A sketch kind is refused, as the commands
    /// refuse it.
    pub kind: FeatureKind,
}

/// How a [`Draft`] went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drafted {
    /// The draft's [`Draft::revision`].
    pub revision: u64,
    /// Why the draft gives no solid, or can't be applied: the model
    /// answered is then the committed one, without it.
    pub error: Option<String>,
    /// What to draw of where the draft fails, beside `error`, as a failed
    /// feature's ([`FeatureFailure::geometry`](crate::FeatureFailure::geometry)), its operand faces those
    /// of the committed model answered. Not serialized: the web worker's
    /// reply carries it apart and checks it against the model (see
    /// `src/wire.rs`).
    #[serde(skip)]
    pub geometry: Option<Arc<ErrorGeometry>>,
    /// For a join, cut or intersect, the bodies its solid touches, less
    /// those taken out of it, in the order they were made: those the
    /// panel lists to take out, with those taken out to put back. `None`
    /// where the touch test didn't run: for a new body, where the draft
    /// failed before its solid was made, or where the document refused
    /// it; `Some` of an empty list where it ran and touched nothing. See
    /// [`Evaluation::touched`](crate::Evaluation::touched).
    pub touched: Option<Vec<BodyId>>,
    /// For a cut that works, the bodies it cuts that it takes nothing
    /// from, which it only touches (face to face, say), in the order they
    /// were made: see [`Evaluation::uncut`](crate::Evaluation::uncut). Empty otherwise.
    pub uncut: Vec<BodyId>,
    /// For a move turning about an axis, a mirror or a pattern, where its
    /// axis or plane is, if it was found: a point on it and its direction (a
    /// mirror's normal), not unit, as [`Evaluation::references`](crate::Evaluation::references) has it,
    /// whether or not the draft goes on to work. Checked on the wire:
    /// finite, within four times the coordinate limit, the direction not
    /// zero.
    pub reference: Option<Box<[[f64; 3]; 2]>>,
    /// For an align, the points and directions found on each side, as
    /// [`Evaluation::aligned`](crate::Evaluation::aligned) has them, whether or not the draft goes on
    /// to work. Checked on the wire as [`Drafted::reference`] is
    /// ([`AlignDatums::fits`]).
    pub datums: Option<Box<AlignDatums>>,
    /// For a scale, what it found, as [`Evaluation::scaled`](crate::Evaluation::scaled) has it,
    /// whether or not the draft goes on to work. Checked on the wire
    /// ([`ScaleFound::fits`]).
    pub scale: Option<Box<ScaleFound>>,
    /// For a sweep, where its handles stand, as [`Evaluation::swept`](crate::Evaluation::swept)
    /// has it, whether or not the draft goes on to work. Checked on the
    /// wire ([`SweepFound::fits`]).
    pub sweep: Option<Box<SweepFound>>,
}

impl Drafted {
    /// The draft of `revision` refused before it was regenerated, for
    /// `error`.
    pub(crate) fn refused(revision: u64, error: String) -> Drafted {
        Drafted {
            error: Some(error),
            ..Drafted::empty(revision)
        }
    }

    /// What `evaluation` found of the draft, `feature`: what it touched
    /// and its references, an align's datums, a scale's and a sweep's,
    /// each where it has one. Its error and uncut bodies are the
    /// caller's to fill in.
    pub(crate) fn found(revision: u64, evaluation: &Evaluation, feature: FeatureId) -> Drafted {
        Drafted {
            touched: of_feature(&evaluation.touched, feature),
            reference: of_feature(&evaluation.references, feature)
                .map(|[point, along]| Box::new([point.to_array(), along.to_array()])),
            datums: of_feature(&evaluation.aligned, feature).map(Box::new),
            scale: of_feature(&evaluation.scaled, feature).map(Box::new),
            sweep: of_feature(&evaluation.swept, feature).map(Box::new),
            ..Drafted::empty(revision)
        }
    }

    /// The draft of `revision`, nothing found.
    fn empty(revision: u64) -> Drafted {
        Drafted {
            revision,
            error: None,
            geometry: None,
            touched: None,
            uncut: Vec::new(),
            reference: None,
            datums: None,
            scale: None,
            sweep: None,
        }
    }

    /// Whether what the draft found is fit to draw, as the wire checks
    /// it: its axis or plane (`reference_fits`), an align's datums
    /// ([`AlignDatums::fits`]), a scale's ([`ScaleFound::fits`]) and a
    /// sweep's ([`SweepFound::fits`]), each where it has one.
    pub fn fits(&self) -> bool {
        self.reference.as_deref().is_none_or(reference_fits)
            && self.datums.as_deref().is_none_or(AlignDatums::fits)
            && self.scale.as_deref().is_none_or(ScaleFound::fits)
            && self.sweep.as_deref().is_none_or(SweepFound::fits)
    }
}

/// What `list`, one of an [`Evaluation`]'s tables by feature, holds of
/// `feature`.
pub(crate) fn of_feature<T: Clone>(list: &[(FeatureId, T)], feature: FeatureId) -> Option<T> {
    list.iter()
        .find(|(id, _)| *id == feature)
        .map(|(_, found)| found.clone())
}

/// How near 1 the lengths of a [`SweepFound`]'s directions are: looser
/// than the answer's tables' [`Picking::UNIT`](crate::Picking::UNIT), as
/// they're worked out from the path as built.
const SWEEP_UNIT: f64 = 1e-6;

/// Where a sweep's handles stand, found from its path as built, before
/// the kernel is asked: for its twist, the end of an open chain; for a
/// helix's pitch and turns, its axis and the profile's middle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SweepFound {
    /// An open chain's end: where it ends, its unit tangent there, and
    /// the profile's x carried along to it (unit, square to the tangent),
    /// where no twist leaves it: by rotation-minimizing frames as the
    /// path turns, or kept and taken square to the tangent for Keep
    /// orientation.
    End {
        at: [f64; 3],
        tangent: [f64; 3],
        zero: [f64; 3],
    },
    /// A helix's start: the middle of the profile's box, its foot on
    /// the axis, and the unit axis the way the helix climbs.
    Helix {
        middle: [f64; 3],
        foot: [f64; 3],
        axis: [f64; 3],
    },
}

impl SweepFound {
    /// Whether the workers' wire takes it: its points within
    /// [`MAX_REFERENCE`] of zero, finite, its directions unit.
    pub fn fits(&self) -> bool {
        let unit = |v: &[f64; 3]| bounds::unit_within(*v, SWEEP_UNIT);
        match self {
            SweepFound::End { at, tangent, zero } => {
                within_reference(at) && unit(tangent) && unit(zero)
            }
            SweepFound::Helix { middle, foot, axis } => {
                within_reference(middle) && within_reference(foot) && unit(axis)
            }
        }
    }
}

/// What a scale found, on its bodies as the features before it leave
/// them: the point it scales about, its edge length's edge's length
/// (before the scale, in millimetres), the factors along X, Y and Z
/// it scales by, each `None` where it wasn't found (the factors also
/// where they'd be out of range), and how many faces of the scaled bodies
/// are fitted (stand for a surface they're only within the fit tolerance
/// of): a scale up stretches their distance from it by its largest
/// factor, which the panel says.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScaleFound {
    pub centre: Option<[f64; 3]>,
    pub length: Option<f64>,
    pub factors: Option<[f64; 3]>,
    pub fitted: u32,
}

impl ScaleFound {
    /// Whether the workers' wire takes it: at least one of its point,
    /// length and factors there, its point within [`MAX_REFERENCE`] of
    /// zero and finite, its length finite and not negative, its factors
    /// finite and above zero.
    pub fn fits(&self) -> bool {
        (self.centre.is_some() || self.length.is_some() || self.factors.is_some())
            && self.centre.is_none_or(ScaleFound::centre_fits)
            && self.length.is_none_or(ScaleFound::length_fits)
            && self.factors.is_none_or(ScaleFound::factors_fit)
    }

    /// Whether the wire takes `centre` as a scale's point.
    pub(crate) fn centre_fits(centre: [f64; 3]) -> bool {
        within_reference(&centre)
    }

    /// Whether the wire takes `length` as a scale's edge's length.
    pub(crate) fn length_fits(length: f64) -> bool {
        length.is_finite() && length >= 0.0
    }

    /// Whether the wire takes `factors` as a scale's.
    pub(crate) fn factors_fit(factors: [f64; 3]) -> bool {
        factors.iter().all(|f| f.is_finite() && *f > 0.0)
    }
}

/// One side of an align as regenerating found it, on the bodies as the
/// features before the align leave them: its point, and its directions
/// (not unit; a flat face's normal out of its body, the others signed as
/// the document's `DirRef` says).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AlignFound {
    pub point: [f64; 3],
    pub primary: Option<[f64; 3]>,
    pub secondary: Option<[f64; 3]>,
}

/// What an align found: each side whose references were all found, and
/// whether the moved side's primary goes against the target's (the
/// default for two flat faces' normals or rims' axes, turned round by
/// the align's flip). A side that wasn't, or whose numbers the wire
/// wouldn't take ([`AlignDatums::fits`]: a nearly straight arc's centre
/// far out), is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AlignDatums {
    pub moved: Option<AlignFound>,
    pub target: Option<AlignFound>,
    pub opposed: bool,
}

impl AlignFound {
    /// Whether the workers' wire takes it: its point and directions
    /// within [`MAX_REFERENCE`] of zero, finite, no direction zero.
    pub fn fits(&self) -> bool {
        let direction = |v: &[f64; 3]| within_reference(v) && bounds::nonzero(*v);
        within_reference(&self.point) && self.primary.iter().chain(&self.secondary).all(direction)
    }
}

impl AlignDatums {
    /// Whether the workers' wire takes it: each side there
    /// [`AlignFound::fits`], and at least one there.
    pub fn fits(&self) -> bool {
        let sides = [self.moved, self.target];
        sides.iter().any(Option::is_some) && sides.iter().flatten().all(AlignFound::fits)
    }
}

/// How far from zero a coordinate of a draft's axis or plane
/// ([`Drafted::reference`]) may be: its point is on a body within the
/// coordinate limit, its direction at most the difference of two such
/// points.
pub const MAX_REFERENCE: f64 = 4.0 * varde_document::MAX_COORD as f64;

/// Whether `[point, along]` is an axis or plane a draft's reply can
/// carry ([`Drafted::reference`]): finite, every coordinate within
/// [`MAX_REFERENCE`] of zero, `along` not zero. Regenerating keeps only
/// those (one far out, a big arc's centre, isn't drawn), so a reply it
/// sends is never refused on the wire for its reference.
pub(crate) fn reference_fits([point, along]: &[[f64; 3]; 2]) -> bool {
    within_reference(point) && within_reference(along) && bounds::nonzero(*along)
}

/// Whether every coordinate of `v` is finite and within
/// [`MAX_REFERENCE`] of zero, as the wire takes a reference's.
fn within_reference(v: &[f64; 3]) -> bool {
    bounds::within(*v, MAX_REFERENCE)
}
