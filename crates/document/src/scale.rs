//! The scale feature: bodies already made scaled about a point, by one
//! factor, a factor per world axis, or the factor that gives a picked
//! edge a typed length.

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::motion::check_bodies;
use crate::{
    AlignError, BodyId, Design, EdgeError, EdgeRef, Extent, FeatureId, MotionError, PointRef,
};

/// The largest factor a scale takes, and the inverse of the smallest:
/// `1e-3 ..= 1e3`, whether typed or worked out from an edge's length.
pub const MAX_SCALE_FACTOR: f64 = 1e3;

/// A scale: the bodies it scales, each about the point `about` by
/// `factor` along the world axes, `x ↦ c + S·(x − c)`. Each body keeps
/// its id and its faces their names, as a move's do. Positive factors
/// only: a negative one would be a mirror, which is its own feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scale {
    /// `1..=`[`MAX_FEATURE_BODIES`](crate::MAX_FEATURE_BODIES) bodies
    /// features before it make, sorted without repeats.
    pub bodies: Vec<BodyId>,
    /// The point that stays put: the origin, or a point on a body made
    /// before it (an align's references: a corner, an edge's middle, a
    /// rim's centre), found as the features before it leave its body. It
    /// may be on one of the scaled bodies.
    pub about: PointRef,
    pub factor: ScaleFactor,
}

/// How much a scale scales. New kinds are appended: a kind's place in
/// the list is how the workers' bytes store it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScaleFactor {
    /// The same factor along every axis ([`Scale::factor_ask`]).
    Uniform(Value),
    /// A factor along world X, Y and Z, each as a uniform one.
    PerAxis([Value; 3]),
    /// The factor that gives `edge` the length `length`, worked out at
    /// every regeneration from the edge's length as the features before
    /// the scale leave it (the whole chain's, between its two corners, or
    /// all round a closed one), so the edge keeps its typed length when
    /// an edit before it changes the edge. Uniform; with `axis_only`, along
    /// the world axis the edge runs along only, which it must then be
    /// (straight, within a sine of `1e-9` of the axis). The factor must
    /// come out within `1e-3 ..= 1e3`.
    EdgeLength {
        /// On one of the scaled bodies.
        edge: EdgeRef,
        /// A length as an extrude's distance ([`Extent::ask`]).
        length: Value,
        axis_only: bool,
    },
}

impl ScaleFactor {
    /// The edge it names, for an edge length.
    pub fn edge(&self) -> Option<&EdgeRef> {
        match self {
            ScaleFactor::EdgeLength { edge, .. } => Some(edge),
            ScaleFactor::Uniform(_) | ScaleFactor::PerAxis(_) => None,
        }
    }
}

impl Scale {
    /// What a factor is checked against in `design`: a number from
    /// `1 / `[`MAX_SCALE_FACTOR`] to [`MAX_SCALE_FACTOR`]
    /// ([`Ask::factor`]).
    pub fn factor_ask(design: &Design) -> Ask {
        Ask::factor(design.units, MAX_SCALE_FACTOR)
    }

    /// What an edge length is checked against in `design`: a length as
    /// an extrude's distance ([`Extent::ask`]).
    pub fn length_ask(design: &Design) -> Ask {
        Extent::ask(design)
    }

    /// Checks what needs only the scale and `design`: the body count and
    /// order, every factor or the edge length, the point's own parts,
    /// and an edge's own parts and its body among the scaled ones. What
    /// the bodies and references name is
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view.
    pub fn check_own(&self, design: &Design) -> Result<(), ScaleError> {
        check_bodies(&self.bodies).map_err(|why| match why {
            MotionError::Bodies(count) => ScaleError::Bodies(count),
            _ => ScaleError::BodyOrder,
        })?;
        self.about.check_own().map_err(ScaleError::About)?;
        let factor = Scale::factor_ask(design);
        match &self.factor {
            ScaleFactor::Uniform(value) => {
                value.check(&factor).map_err(|_| ScaleError::Factor)?;
            }
            ScaleFactor::PerAxis(values) => {
                for value in values {
                    value.check(&factor).map_err(|_| ScaleError::Factor)?;
                }
            }
            ScaleFactor::EdgeLength { edge, length, .. } => {
                (length.check(&Scale::length_ask(design))).map_err(|_| ScaleError::Length)?;
                edge.check_own().map_err(ScaleError::Edge)?;
                if self.bodies.binary_search(&edge.body).is_err() {
                    return Err(ScaleError::EdgeBody(edge.body));
                }
            }
        }
        Ok(())
    }

    /// Its typed values and what each is checked against in `design`:
    /// the factors, or the edge length.
    pub(crate) fn values_mut(&mut self, design: &Design) -> Vec<(&mut Value, Ask)> {
        let factor = Scale::factor_ask(design);
        match &mut self.factor {
            ScaleFactor::Uniform(value) => vec![(value, factor)],
            ScaleFactor::PerAxis(values) => values.iter_mut().map(|v| (v, factor)).collect(),
            ScaleFactor::EdgeLength { length, .. } => vec![(length, Scale::length_ask(design))],
        }
    }

    /// The features its point's and edge's keys name as their faces'
    /// makers, with the body each is on.
    pub(crate) fn named(&self) -> Vec<(BodyId, Vec<FeatureId>)> {
        let edge = (self.factor.edge()).map(|edge| (edge.body, edge.makers().to_vec()));
        let about = self.about.body().map(|body| (body, self.about.makers()));
        about.into_iter().chain(edge).collect()
    }
}

/// What's wrong with a scale, see
/// [`CheckError::Scale`](crate::CheckError::Scale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleError {
    /// It names this many bodies: none, or over
    /// [`MAX_FEATURE_BODIES`](crate::MAX_FEATURE_BODIES).
    Bodies(usize),
    /// Its bodies aren't sorted, or one is repeated.
    BodyOrder,
    /// It scales this body, which isn't there or which no feature before
    /// it makes.
    Body(BodyId),
    /// A factor's expression doesn't give its value, or the value isn't
    /// a factor [`Scale::factor_ask`] takes.
    Factor,
    /// Its edge length's expression doesn't give its value, or the value
    /// isn't a length [`Scale::length_ask`] takes.
    Length,
    /// Its point fails its own check ([`PointRef::check_own`]).
    About(AlignError),
    /// Its edge fails its own check ([`EdgeRef::check_own`]).
    Edge(EdgeError),
    /// Its edge is on this body, which isn't one it scales.
    EdgeBody(BodyId),
    /// Its point or edge is on this body, which is made by the feature
    /// or one after it, or isn't there and has an id a body made later
    /// could take.
    RefBody(BodyId),
    /// A key of its point's or edge's faces names this feature, which is
    /// the feature itself or comes after it, or isn't there and has an
    /// id a feature made later could take.
    RefMaker(FeatureId),
}

impl fmt::Display for ScaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScaleError::Bodies(count) => write!(
                f,
                "names {count} bodies, not 1 to {}",
                crate::MAX_FEATURE_BODIES
            ),
            ScaleError::BodyOrder => f.write_str("its bodies are out of order or repeated"),
            ScaleError::Body(body) => write!(
                f,
                "scales body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            ScaleError::Factor => write!(
                f,
                "a factor's expression doesn't give its value, or it isn't from {} to \
                 {MAX_SCALE_FACTOR}",
                1.0 / MAX_SCALE_FACTOR
            ),
            ScaleError::Length => {
                f.write_str("its edge length's expression doesn't give its value")
            }
            ScaleError::About(why) => write!(f, "its point: {why}"),
            ScaleError::Edge(why) => why.fmt(f),
            ScaleError::EdgeBody(body) => {
                write!(f, "its edge is on body {}, which it doesn't scale", body.0)
            }
            ScaleError::RefBody(body) => write!(
                f,
                "its point or edge is on body {}, which isn't made before it",
                body.0
            ),
            ScaleError::RefMaker(feature) => write!(
                f,
                "its point or edge is on a face made by feature {}, which doesn't come before it",
                feature.0
            ),
        }
    }
}

impl std::error::Error for ScaleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ScaleError::About(why) => Some(why),
            ScaleError::Edge(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
