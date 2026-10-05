//! The split feature: a body already made cut in two by a tool, a plane,
//! a face's surface continued past the body, another body, a sketch's
//! regions or an open chain of its curves. One piece keeps the body's
//! id, the other becomes a new body the split makes, or only one side is
//! kept (a trim).

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_sketch::{Id, RegionRef, RegionRefError, Sketch};

use crate::{BodyId, FaceRef, FeatureId, MAX_COORD, MAX_EXTRUDE_REGIONS, PlaneError, PlaneRef};

/// The most curves a split's open chain may hold.
pub const MAX_SPLIT_CURVES: usize = 256;

/// A split: `body` cut by `tool` into its **front**, the part inside
/// the tool (on the side a plane's normal points to, inside the closed
/// solid a face's surface bounds, inside the tool body or the sketch's
/// regions, left of the chain), and its **back**, the rest.
///
/// With both kept, the piece `original` names keeps the body's id (and
/// so every later feature naming the body gets it), and the other is
/// `new_body`, which the split makes. Keeping one side is a trim: that
/// side keeps the id, whatever `original` says, and there's no new body;
/// but a split that made one keeps its id in `new_body`, for when it
/// keeps both again. A side of several pieces is one body of several
/// shells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Split {
    /// A body a feature before it makes.
    pub body: BodyId,
    pub tool: SplitTool,
    /// Which piece keeps the body's id when both are kept.
    #[serde(default)]
    pub original: Side,
    #[serde(default)]
    pub keep: Keep,
    /// The body the piece `original` doesn't name becomes, made by the
    /// split while both sides are kept, and there then. Keeping one side
    /// it's the id the body had, if the split made one before, which the
    /// document holds for it (no other body or feature gets it), so that
    /// keeping both again brings the body back with it, and what was
    /// named on it (a sketch on its face) finds it again; or none, for a
    /// split that never kept both (and every split written before ids
    /// were held). The commands fill it in ([`BodyId::NEW`] stands for a
    /// new body until then), whatever it holds: a new id when adding,
    /// the one held when setting.
    #[serde(default)]
    pub new_body: Option<BodyId>,
}

/// What a split cuts with. New kinds are appended: a kind's place in the
/// list is how the workers' bytes store it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SplitTool {
    /// An origin plane or a flat face's plane: the front is on the side
    /// its normal points to (out of a face's body).
    Plane(PlaneRef),
    /// A face's surface, flat or curved, continued past the body: the
    /// front is inside the closed solid it bounds (a plane's or an open
    /// wall's on the side its normal points to). Found on its body as
    /// the features before the split leave it; may be on the split body.
    Face(FaceRef),
    /// Another body's solid, kept: the front is inside it.
    Body(BodyId),
    /// Regions of a sketch before the split, extruded through the body
    /// both ways: the front is inside them.
    Regions {
        sketch: FeatureId,
        /// `1..=`[`MAX_EXTRUDE_REGIONS`], as an extrude's.
        regions: Vec<RegionRef>,
    },
    /// An open chain of a sketch's curves, continued along its end
    /// tangents past the body and extruded through it both ways: the
    /// front is on its left, the chain running as its first curve (by
    /// id) does.
    Chain {
        sketch: FeatureId,
        /// `1..=`[`MAX_SPLIT_CURVES`], sorted without repeats; joined
        /// end to end into one open chain by regeneration.
        curves: Vec<Id>,
    },
}

/// A piece of a split body.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    /// Inside the tool.
    #[default]
    Front,
    /// The rest.
    Back,
}

impl Side {
    /// The other piece.
    pub fn other(self) -> Side {
        match self {
            Side::Front => Side::Back,
            Side::Back => Side::Front,
        }
    }

    /// The piece as the user sees it: "Front".
    pub fn name(self) -> &'static str {
        match self {
            Side::Front => "Front",
            Side::Back => "Back",
        }
    }
}

/// Which pieces of a split body stay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Keep {
    /// Both: one keeps the body's id, the other is a new body.
    #[default]
    Both,
    /// Only the front, which keeps the body's id.
    Front,
    /// Only the back, which keeps the body's id.
    Back,
}

impl Keep {
    /// The piece that keeps the body's id: the one kept, or with both
    /// kept, `original`.
    pub fn kept(self, original: Side) -> Side {
        match self {
            Keep::Both => original,
            Keep::Front => Side::Front,
            Keep::Back => Side::Back,
        }
    }
}

impl SplitTool {
    /// The sketch it takes regions or curves from.
    pub fn sketch(&self) -> Option<FeatureId> {
        match self {
            SplitTool::Regions { sketch, .. } | SplitTool::Chain { sketch, .. } => Some(*sketch),
            SplitTool::Plane(_) | SplitTool::Face(_) | SplitTool::Body(_) => None,
        }
    }
}

impl Split {
    /// The piece that keeps the body's id: the one kept, or with both
    /// kept, `original`.
    pub fn kept(&self) -> Side {
        self.keep.kept(self.original)
    }

    /// Whether it keeps both pieces, and so makes a new body.
    pub fn keeps_both(&self) -> bool {
        self.keep == Keep::Both
    }

    /// The body it makes: its new body while it keeps both pieces.
    pub fn made_body(&self) -> Option<BodyId> {
        self.new_body.filter(|_| self.keeps_both())
    }

    /// The id it holds for its new body while it keeps one side, see
    /// [`Split::new_body`].
    pub fn held_body(&self) -> Option<BodyId> {
        self.new_body.filter(|_| !self.keeps_both())
    }

    /// The bodies it names, which it depends on: its body, a tool body,
    /// and a face tool's body; sorted without repeats. Not a plane face's
    /// body (as a mirror's plane: the split stays, and fails until given
    /// another).
    pub fn bodies(&self) -> Vec<BodyId> {
        let tool = match &self.tool {
            SplitTool::Body(body) => Some(*body),
            SplitTool::Face(face) => Some(face.body),
            SplitTool::Plane(_) | SplitTool::Regions { .. } | SplitTool::Chain { .. } => None,
        };
        let mut bodies: Vec<BodyId> = [Some(self.body), tool].into_iter().flatten().collect();
        bodies.sort_unstable();
        bodies.dedup();
        bodies
    }

    /// Checks what needs only the split: its new body there when both
    /// sides are kept, a tool body not the body itself, a face's
    /// point in bounds, the regions' count and each region, the curves'
    /// count and order. What the bodies, faces and sketch name is
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view.
    pub fn check_own(&self) -> Result<(), SplitError> {
        if self.keeps_both() && self.new_body.is_none() {
            return Err(SplitError::NoNewBody);
        }
        match &self.tool {
            SplitTool::Plane(PlaneRef::Origin(_)) => {}
            SplitTool::Plane(PlaneRef::Face(face)) | SplitTool::Face(face) => {
                face.check_own().map_err(SplitError::Face)?;
            }
            SplitTool::Body(tool) => {
                if *tool == self.body {
                    return Err(SplitError::ToolIsBody);
                }
            }
            SplitTool::Regions { regions, .. } => {
                let count = regions.len();
                if !(1..=MAX_EXTRUDE_REGIONS).contains(&count) {
                    return Err(SplitError::Regions(count));
                }
                for region in regions {
                    (region.check(f64::from(MAX_COORD))).map_err(SplitError::Region)?;
                }
            }
            SplitTool::Chain { curves, .. } => {
                let count = curves.len();
                if !(1..=MAX_SPLIT_CURVES).contains(&count) {
                    return Err(SplitError::Curves(count));
                }
                if !curves.windows(2).all(|pair| pair[0] < pair[1]) {
                    return Err(SplitError::CurveOrder);
                }
            }
        }
        Ok(())
    }

    /// Checks what's required of a split when it's added or edited, but
    /// not of one in a document (a later edit of its sketch may break it,
    /// which regeneration reports): every curve of a chain is a curve of
    /// `sketch`, its sketch.
    pub fn check_curves(&self, sketch: &Sketch) -> Result<(), SplitError> {
        if let SplitTool::Chain { curves, .. } = &self.tool
            && let Some(&missing) = curves.iter().find(|&&id| sketch.curve(id).is_none())
        {
            return Err(SplitError::Curve(missing));
        }
        Ok(())
    }
}

/// What's wrong with a split, see
/// [`CheckError::Split`](crate::CheckError::Split).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitError {
    /// It splits this body, which isn't there or which no feature before
    /// it makes.
    Body(BodyId),
    /// It keeps both sides but has no new body.
    NoNewBody,
    /// Its new body, this one, isn't a body it makes.
    NewBody(BodyId),
    /// Its tool body is the body it splits.
    ToolIsBody,
    /// Its tool body, this one, isn't there or no feature before it
    /// makes it.
    ToolBody(BodyId),
    /// Its face's point isn't finite or within the coordinate limit.
    Face(PlaneError),
    /// Its face tool is on this body, which isn't there or which no
    /// feature before it makes.
    FaceBody(BodyId),
    /// Its plane face is on this body, which is made by the split or a
    /// feature after it, or isn't there and has an id a body made later
    /// could take.
    RefBody(BodyId),
    /// Its face's key names this feature, which is the split itself or
    /// comes after it, or isn't there and has an id a feature made later
    /// could take.
    RefMaker(FeatureId),
    /// Its sketch, this feature, isn't a sketch feature before it.
    Sketch(FeatureId),
    /// It takes this many regions: none, or over [`MAX_EXTRUDE_REGIONS`].
    Regions(usize),
    /// A region reference fails its check.
    Region(RegionRefError),
    /// Its chain has this many curves: none, or over
    /// [`MAX_SPLIT_CURVES`].
    Curves(usize),
    /// Its chain's curves aren't sorted, or one is repeated.
    CurveOrder,
    /// Its chain names this curve, which its sketch doesn't have.
    Curve(Id),
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SplitError::Body(body) => write!(
                f,
                "splits body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            SplitError::NoNewBody => f.write_str("keeps both sides but makes no new body"),
            SplitError::NewBody(body) => {
                write!(
                    f,
                    "names body {} as its new body, which it doesn't make",
                    body.0
                )
            }
            SplitError::ToolIsBody => f.write_str("splits a body with itself"),
            SplitError::ToolBody(body) => write!(
                f,
                "splits with body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            SplitError::Face(why) => why.fmt(f),
            SplitError::FaceBody(body) => write!(
                f,
                "splits with a face of body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            SplitError::RefBody(body) => write!(
                f,
                "its plane is a face on body {}, which isn't made before it",
                body.0
            ),
            SplitError::RefMaker(feature) => write!(
                f,
                "its face is made by feature {}, which doesn't come before it",
                feature.0
            ),
            SplitError::Sketch(sketch) => write!(
                f,
                "its sketch, feature {}, isn't a sketch before it",
                sketch.0
            ),
            SplitError::Regions(count) => {
                write!(f, "takes {count} regions, not 1 to {MAX_EXTRUDE_REGIONS}")
            }
            SplitError::Region(why) => why.fmt(f),
            SplitError::Curves(count) => {
                write!(
                    f,
                    "its line has {count} curves, not 1 to {MAX_SPLIT_CURVES}"
                )
            }
            SplitError::CurveOrder => f.write_str("its line's curves are out of order or repeated"),
            SplitError::Curve(id) => {
                write!(
                    f,
                    "its line names curve {}, which its sketch doesn't have",
                    id.get()
                )
            }
        }
    }
}

impl std::error::Error for SplitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SplitError::Face(why) => Some(why),
            SplitError::Region(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
