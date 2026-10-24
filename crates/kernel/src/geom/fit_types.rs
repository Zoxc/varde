//! The records of a fitted surface: its [`Fit`] (the [`Intent`] it
//! approximates and the certified deviation from it) and its
//! [`ExactRows`], the tile rows equal to a space curve bit for bit.
//!
//! **Fits.** A surface without a fit is its exact form. A fitted surface
//! (a free form, or a ruled one with a fitted rail) is authoritative once
//! stored: topology and decisions read its tiles, never its intent. The
//! intent says what the surface stands for (an offset, an extrusion, a
//! rolling ball, ...), for datums, measures, refits and export; `dev`
//! bounds the surface's distance from it, a certified bound either way:
//! re-certified from the stored surface and the intent alone where the
//! intent has a certified distance (`recheckable`), else the certificate
//! its construction gave.
//!
//! **Intents** are an open enum: later operations add variants, never
//! change one's shape. They name the body's own geometry by id: the
//! surfaces an offset or a rolling ball is built on, the space curves of
//! profiles, rails, spines, paths, sections and boundaries. An intent
//! names only surfaces added before its own, so intents never form a
//! cycle; what an intent names is kept with the surface (a root of the
//! body's geometry collection) and its ids follow the body's when it is
//! compacted.
//!
//! **Extension law** ([`Intent::extension`]). Every intent states how its
//! surface continues past its domain, deterministically, as part of the
//! operation's semantics: the defining form or curves continue as
//! themselves; a rolling ball's spine and a sweep's path continue by
//! their end tangents (a helix as itself), the ball on the supports'
//! extensions and the frame by its [`FrameLaw`]; a loft by the ruled
//! continuation of its end rows; an imported surface and a corner patch,
//! which have no law of their own, by the C¹ extrapolation of their
//! boundary rows, the deviation of the appended spans reported; an affine
//! image by its inner intent's law, mapped.
//!
//! **Exact rows.** A fitted surface may declare that a run of its tile
//! rows equals a space curve's chain bit for bit (a sweep's end profiles,
//! a loft's sections, a fillet's contact rows): span `k` of the curve is
//! the row on the tile line at cell `first + k`, affine control points
//! and weights. The body's check holds them to it (invariant 2).
//!
//! Nothing here is persisted: the records live in memory with their body.

use varde_decide::bezier::Dir;
use varde_decide::ids::{CurveId, SurfId};
use varde_decide::num::bounded::Bounded;

use super::form::FormKey;

/// A space curve of the body an intent names: its id in the body's
/// curves.
pub type CurveRef = CurveId;

/// A fitted surface's intent and certified deviation from it (module
/// docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    /// What the surface stands for.
    pub intent: Intent,
    /// A certified bound of the surface's distance from its intent.
    pub dev: Bounded<f64>,
    /// Whether `dev` can be certified again from the stored surface and
    /// the intent alone; else it is its construction's certificate.
    pub recheckable: bool,
}

/// What a fitted surface stands for (module docs). Open: later operations
/// add variants.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    /// A recognised analytic form (an imported surface).
    Form(FormKey),
    /// The offset of the surface `base` by `d` along its normal.
    Offset { base: SurfId, d: f64 },
    /// The extrusion of a profile along `dir`.
    Extrusion { profile: CurveRef, dir: [f64; 3] },
    /// The revolution of a profile about an axis.
    Revolution { profile: CurveRef, axis: Axis },
    /// The ruled surface between two rails, a rail fitted.
    Ruled { rails: [CurveRef; 2] },
    /// The wall of constant slope through a profile: `angle` degrees from
    /// the pull direction (a taper, a draft).
    ConstantSlope {
        profile: CurveRef,
        pull: [f64; 3],
        angle: f64,
    },
    /// The ball of the radius law rolling on two supports along a spine.
    RollingBall {
        supports: [SurfId; 2],
        radius: Law,
        spine: CurveRef,
    },
    /// A profile swept along a path, its frame by a law, scaled by a law.
    Sweep {
        profile: CurveRef,
        path: PathRef,
        frame: FrameLaw,
        scale: Law,
    },
    /// The loft through sections, guided by rails.
    Loft {
        sections: Vec<CurveRef>,
        rails: Vec<CurveRef>,
    },
    /// The n-sided corner patch between boundary curves.
    VertexBlend { boundaries: Vec<CurveRef> },
    /// An imported surface without a recognised form.
    Import { entity: ImportRef },
    /// The image of an intent under an affine map (per-axis scales).
    Affine { map: Affine3, inner: Box<Intent> },
}

/// A sweep's path: a space curve of the body, or an analytic helix kept
/// as itself (its fitted chain derived from it).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum PathRef {
    /// A space curve of the body.
    Curve(CurveRef),
    /// A helix about `axis` of `radius`, rising `pitch` per turn, over
    /// `turns` turns.
    Helix {
        axis: Axis,
        radius: f64,
        pitch: f64,
        turns: f64,
        handedness: Hand,
    },
}

/// How a sweep's profile frame moves along its path. Open: later
/// operations add laws.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameLaw {
    /// The profile frame translated only.
    Fixed,
    /// On a helix, the frame following the axis (constant relative to
    /// it); elsewhere the Frenet frame, where the curvature is certified
    /// positive.
    Frenet,
    /// The rotation-minimising frame; the profile starts in the plane
    /// perpendicular to the path.
    RotationMinimising,
}

/// A scalar along a spine or path (a radius, a scale). Open: later
/// operations add laws.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Law {
    /// The same value everywhere.
    Constant(f64),
}

/// A helix's turning sense about its axis direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hand {
    /// Counter-clockwise seen from the axis's head.
    Right,
    /// Clockwise seen from the axis's head.
    Left,
}

/// An axis: a point on it and its direction, as given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    /// A point on the axis.
    pub point: [f64; 3],
    /// Its direction.
    pub dir: [f64; 3],
}

/// An affine map `x ↦ linear·x + shift`, as given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine3 {
    /// The linear part, row-major.
    pub linear: [[f64; 3]; 3],
    /// The translation.
    pub shift: [f64; 3],
}

/// The entity of an imported file an intent came from: its instance
/// number there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImportRef(pub u64);

/// How a fitted surface continues past its domain (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtensionLaw {
    /// The defining form or curves continue as themselves: a conic or a
    /// helix as itself, other curves C¹ by their end tangents (an
    /// offset: of its base's extension).
    Defining,
    /// The spine continues C¹ by its end tangent, the ball rolling on
    /// the supports' extensions.
    Spine,
    /// The path continues C¹ by its end tangent (a helix as itself), the
    /// frame by its law.
    Path,
    /// Past the end sections, the C¹ ruled continuation of the end rows.
    EndRows,
    /// The C¹ extrapolation of the boundary rows, each continued by its
    /// end tangent; the deviation of the appended spans is reported.
    BoundaryRows,
}

/// An id an intent names, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IntentId {
    /// A surface of the body.
    Surface(SurfId),
    /// A space curve of the body.
    Curve(CurveId),
}

impl Intent {
    /// How the surface continues past its domain (module docs); an
    /// affine image by its inner intent's law, mapped.
    pub fn extension(&self) -> ExtensionLaw {
        match self {
            Intent::Form(_)
            | Intent::Offset { .. }
            | Intent::Extrusion { .. }
            | Intent::Revolution { .. }
            | Intent::Ruled { .. }
            | Intent::ConstantSlope { .. } => ExtensionLaw::Defining,
            Intent::RollingBall { .. } => ExtensionLaw::Spine,
            Intent::Sweep { .. } => ExtensionLaw::Path,
            Intent::Loft { .. } => ExtensionLaw::EndRows,
            Intent::VertexBlend { .. } | Intent::Import { .. } => ExtensionLaw::BoundaryRows,
            Intent::Affine { inner, .. } => inner.extension(),
        }
    }

    /// The ids it names, in the order of its fields, an affine image's
    /// inner intent's after its own.
    pub fn ids(&self) -> Vec<IntentId> {
        let mut out = Vec::new();
        let mut i = self;
        loop {
            let (s, c) = (IntentId::Surface, IntentId::Curve);
            match i {
                Intent::Form(_) | Intent::Import { .. } => {}
                Intent::Offset { base, .. } => out.push(s(*base)),
                Intent::Extrusion { profile, .. }
                | Intent::Revolution { profile, .. }
                | Intent::ConstantSlope { profile, .. } => out.push(c(*profile)),
                Intent::Ruled { rails } => out.extend(rails.map(c)),
                Intent::RollingBall {
                    supports, spine, ..
                } => {
                    out.extend(supports.map(s));
                    out.push(c(*spine));
                }
                Intent::Sweep { profile, path, .. } => {
                    out.push(c(*profile));
                    if let PathRef::Curve(p) = path {
                        out.push(c(*p));
                    }
                }
                Intent::Loft { sections, rails } => {
                    out.extend(sections.iter().chain(rails).copied().map(c));
                }
                Intent::VertexBlend { boundaries } => out.extend(boundaries.iter().copied().map(c)),
                Intent::Affine { inner, .. } => {
                    i = inner;
                    continue;
                }
            }
            return out;
        }
    }

    /// The intent with every id it names mapped, in [`ids`](Self::ids)'
    /// order; `None` where one maps to nothing.
    pub fn remap(
        &self,
        surface: &mut impl FnMut(SurfId) -> Option<SurfId>,
        curve: &mut impl FnMut(CurveId) -> Option<CurveId>,
    ) -> Option<Intent> {
        let curves = |v: &[CurveRef], curve: &mut dyn FnMut(CurveId) -> Option<CurveId>| {
            v.iter().map(|&x| curve(x)).collect::<Option<Vec<_>>>()
        };
        Some(match self {
            Intent::Form(_) | Intent::Import { .. } => self.clone(),
            Intent::Offset { base, d } => Intent::Offset {
                base: surface(*base)?,
                d: *d,
            },
            Intent::Extrusion { profile, dir } => Intent::Extrusion {
                profile: curve(*profile)?,
                dir: *dir,
            },
            Intent::Revolution { profile, axis } => Intent::Revolution {
                profile: curve(*profile)?,
                axis: *axis,
            },
            Intent::Ruled { rails } => Intent::Ruled {
                rails: [curve(rails[0])?, curve(rails[1])?],
            },
            Intent::ConstantSlope {
                profile,
                pull,
                angle,
            } => Intent::ConstantSlope {
                profile: curve(*profile)?,
                pull: *pull,
                angle: *angle,
            },
            Intent::RollingBall {
                supports,
                radius,
                spine,
            } => Intent::RollingBall {
                supports: [surface(supports[0])?, surface(supports[1])?],
                radius: *radius,
                spine: curve(*spine)?,
            },
            Intent::Sweep {
                profile,
                path,
                frame,
                scale,
            } => Intent::Sweep {
                profile: curve(*profile)?,
                path: match path {
                    PathRef::Curve(p) => PathRef::Curve(curve(*p)?),
                    h @ PathRef::Helix { .. } => h.clone(),
                },
                frame: *frame,
                scale: *scale,
            },
            Intent::Loft { sections, rails } => Intent::Loft {
                sections: curves(sections, curve)?,
                rails: curves(rails, curve)?,
            },
            Intent::VertexBlend { boundaries } => Intent::VertexBlend {
                boundaries: curves(boundaries, curve)?,
            },
            Intent::Affine { map, inner } => Intent::Affine {
                map: *map,
                inner: Box::new(inner.remap(surface, curve)?),
            },
        })
    }
}

impl ExtensionLaw {
    /// Whether the extension's deviation is reported: an extrapolation,
    /// not the intent's own continuation.
    pub fn reports_dev(self) -> bool {
        self == ExtensionLaw::BoundaryRows
    }
}

/// A run of a surface's tile rows equal to a space curve's chain bit for
/// bit (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExactRow {
    /// The direction the rows run in: [`Dir::U`] along the line `v = at`,
    /// [`Dir::V`] along `u = at`, as [`Surface::row`] reads them.
    ///
    /// [`Surface::row`]: super::surface::Surface::row
    pub along: Dir,
    /// The tile line, in the surface's global UV.
    pub at: i64,
    /// The cell of the curve's first span; span `k` is the row at cell
    /// `first + k`.
    pub first: i64,
    /// The curve, running the rows' way.
    pub curve: CurveId,
}

/// A surface's rows equal to a space curve bit for bit (module docs);
/// none for a surface that declares none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ExactRows {
    rows: Vec<ExactRow>,
}

impl ExactRows {
    /// The runs `rows`, in the order given.
    pub fn new(rows: Vec<ExactRow>) -> ExactRows {
        ExactRows { rows }
    }

    /// The runs.
    pub fn rows(&self) -> &[ExactRow] {
        &self.rows
    }

    /// The runs with their curves mapped; `None` where one maps to
    /// nothing.
    pub fn remap(&self, curve: &mut impl FnMut(CurveId) -> Option<CurveId>) -> Option<ExactRows> {
        let rows = self
            .rows
            .iter()
            .map(|r| {
                Some(ExactRow {
                    curve: curve(r.curve)?,
                    ..*r
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(ExactRows { rows })
    }
}

#[cfg(test)]
mod tests;
