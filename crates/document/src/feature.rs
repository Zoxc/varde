//! Features: the steps a design is built from, sketches, extrudes,
//! revolves, combines, moves, mirrors, patterns, aligns, scales, splits,
//! chamfers, shells, fillets, offset faces, drafts, sweeps and lofts.

use serde::{Deserialize, Serialize};
use varde_sketch::Sketch;

use crate::{
    Align, BodyId, Chamfer, Combine, Extrude, FaceDraft, Fillet, LinkSource, Loft, Mirror, Move,
    OffsetFace, Operation, Pattern, Plane, Revolve, Scale, Shell, Split, Sweep,
};

/// A feature's handle in one document. It's opaque: ids come from the
/// document's features, not from literals. Features and bodies take their
/// ids from the same counter, so no feature has a body's number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FeatureId(pub(crate) u64);

impl FeatureId {
    /// Its number, unique in its document among features and bodies: for
    /// naming what the feature makes outside the document, such as the
    /// faces of an extrude's solid.
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    /// Whether it's drawn with the model: a sketch's curves, when it isn't
    /// being edited. An extrude's or revolve's solid is drawn by its
    /// body's visibility.
    pub visible: bool,
    pub kind: FeatureKind,
}

/// What a feature is. Files store a kind by its variant name, so new kinds
/// can go anywhere; a name is never renamed or reused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    Sketch {
        plane: Plane,
        sketch: Sketch,
        /// What each of the sketch's links comes from, in the order of
        /// its links, one each (see [`LinkSource`]). Defaulted: a sketch
        /// from before links has none.
        #[serde(default)]
        sources: Vec<LinkSource>,
    },
    Extrude(Extrude),
    Revolve(Revolve),
    Combine(Combine),
    Move(Move),
    Mirror(Mirror),
    Pattern(Pattern),
    /// Boxed: its two sides' references make it the largest kind by far.
    Align(Box<Align>),
    Scale(Scale),
    Split(Split),
    Chamfer(Chamfer),
    Shell(Shell),
    Fillet(Fillet),
    OffsetFace(OffsetFace),
    FaceDraft(FaceDraft),
    Sweep(Sweep),
    Loft(Loft),
}

impl FeatureKind {
    /// What a feature of this kind is called, as in its default name,
    /// "Revolve 2".
    pub fn noun(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude(_) => "Extrude",
            FeatureKind::Revolve(_) => "Revolve",
            FeatureKind::Combine(_) => "Combine",
            FeatureKind::Move(_) => "Move",
            FeatureKind::Mirror(_) => "Mirror",
            FeatureKind::Pattern(_) => "Pattern",
            FeatureKind::Align(_) => "Align",
            FeatureKind::Scale(_) => "Scale",
            FeatureKind::Split(_) => "Split",
            FeatureKind::Chamfer(_) => "Chamfer",
            FeatureKind::Shell(_) => "Shell",
            FeatureKind::Fillet(_) => "Fillet",
            FeatureKind::OffsetFace(_) => "Offset face",
            FeatureKind::FaceDraft(_) => "Draft",
            FeatureKind::Sweep(_) => "Sweep",
            FeatureKind::Loft(_) => "Loft",
        }
    }

    /// The features it builds on, which come before it: an extrude's or a
    /// revolve's sketch, the sketch a split takes regions or a line
    /// from, a sweep's profile's sketch and its path's sketches, or the
    /// sketches of a loft's sections and rails. Removing one of them
    /// removes this too. Sorted,
    /// without repeats. The makers of the bodies it names
    /// ([`FeatureKind::bodies`]) aren't listed, as finding them takes the
    /// document, but [`Document::removal`](crate::Document::removal)
    /// follows them too. Not the maker of the body under a sketch's face
    /// plane ([`Plane::Face`]): removing it leaves the sketch, which then
    /// doesn't regenerate until it's put on another plane.
    pub fn uses(&self) -> Vec<FeatureId> {
        if let FeatureKind::Loft(loft) = self {
            return loft.sketches();
        }
        let mut uses: Vec<FeatureId> = self.sketch().into_iter().collect();
        if let FeatureKind::Sweep(sweep) = self {
            uses.extend(sweep.path_sketches());
            uses.sort_unstable();
            uses.dedup();
        }
        uses
    }

    /// The sketches whose regions (or points) it takes, which adding it
    /// hides: [`FeatureKind::sketch`]'s, or a loft's sections' sketches;
    /// sorted without repeats. Not a loft's rails' sketches, which stay
    /// as they were.
    pub fn profile_sketches(&self) -> Vec<FeatureId> {
        match self {
            FeatureKind::Loft(loft) => loft.section_sketches(),
            _ => self.sketch().into_iter().collect(),
        }
    }

    /// The bodies it names, which features before it make, and which it
    /// depends on: a combine's target and tools, the bodies a move, a
    /// mirror or a pattern moves or copies, the body an align moves, the
    /// bodies a scale scales (its edge length's edge among them), the
    /// body a split splits, its tool body and its face tool's body, the
    /// body a chamfer's or a fillet's edges are on, the body a shell
    /// hollows, the body an offset face's faces are on, the bodies of a
    /// draft's faces and of its neutral plane's face, the bodies a
    /// sweep's path's edges are on and its helix's axis's body.
    /// Removing one of them, or its maker, removes this too. Not the
    /// bodies an extrude or revolve takes out of its targets, which are
    /// dropped from its list instead, nor the body under a sketch's face
    /// plane, a revolve's axis edge, a move's or pattern's axis, a
    /// mirror's or a split's plane, an align's target or a scale's point
    /// (the feature stays, and fails until it's given another).
    pub fn bodies(&self) -> Vec<BodyId> {
        match self {
            FeatureKind::Combine(combine) => combine.bodies().collect(),
            FeatureKind::Move(moved) => moved.bodies.clone(),
            FeatureKind::Mirror(mirror) => mirror.bodies.clone(),
            FeatureKind::Pattern(pattern) => pattern.bodies.clone(),
            FeatureKind::Align(align) => vec![align.body],
            FeatureKind::Scale(scale) => scale.bodies.clone(),
            FeatureKind::Split(split) => split.bodies(),
            FeatureKind::Chamfer(chamfer) => chamfer.bodies(),
            FeatureKind::Shell(shell) => shell.bodies(),
            FeatureKind::Fillet(fillet) => fillet.bodies(),
            FeatureKind::OffsetFace(offset) => offset.bodies(),
            FeatureKind::FaceDraft(draft) => draft.bodies(),
            FeatureKind::Sweep(sweep) => sweep.bodies(),
            FeatureKind::Sketch { .. }
            | FeatureKind::Extrude(_)
            | FeatureKind::Revolve(_)
            | FeatureKind::Loft(_) => Vec::new(),
        }
    }

    /// The sketch whose regions it takes: an extrude's, a revolve's or a
    /// sweep's (its profile's, not its path's), or a split's regions or
    /// line, which adding it hides. None for a loft, whose sections may be of several
    /// ([`FeatureKind::profile_sketches`]).
    pub fn sketch(&self) -> Option<FeatureId> {
        match self {
            FeatureKind::Sketch { .. }
            | FeatureKind::Combine(_)
            | FeatureKind::Move(_)
            | FeatureKind::Mirror(_)
            | FeatureKind::Pattern(_)
            | FeatureKind::Align(_)
            | FeatureKind::Scale(_)
            | FeatureKind::Chamfer(_)
            | FeatureKind::Shell(_)
            | FeatureKind::Fillet(_)
            | FeatureKind::OffsetFace(_)
            | FeatureKind::FaceDraft(_)
            | FeatureKind::Loft(_) => None,
            FeatureKind::Extrude(extrude) => Some(extrude.sketch),
            FeatureKind::Revolve(revolve) => Some(revolve.sketch),
            FeatureKind::Sweep(sweep) => Some(sweep.sketch),
            FeatureKind::Split(split) => split.tool.sketch(),
        }
    }

    /// What it does with the solid it makes: an extrude's, a revolve's or
    /// a sweep's or a loft's operation. A combine has none: it makes no
    /// solid of its own.
    pub fn operation(&self) -> Option<&Operation> {
        match self {
            FeatureKind::Sketch { .. }
            | FeatureKind::Combine(_)
            | FeatureKind::Move(_)
            | FeatureKind::Mirror(_)
            | FeatureKind::Pattern(_)
            | FeatureKind::Align(_)
            | FeatureKind::Scale(_)
            | FeatureKind::Split(_)
            | FeatureKind::Chamfer(_)
            | FeatureKind::Shell(_)
            | FeatureKind::Fillet(_)
            | FeatureKind::OffsetFace(_)
            | FeatureKind::FaceDraft(_) => None,
            FeatureKind::Extrude(extrude) => Some(&extrude.operation),
            FeatureKind::Revolve(revolve) => Some(&revolve.operation),
            FeatureKind::Sweep(sweep) => Some(&sweep.operation),
            FeatureKind::Loft(loft) => Some(&loft.operation),
        }
    }

    /// The same, to change.
    pub(crate) fn operation_mut(&mut self) -> Option<&mut Operation> {
        match self {
            FeatureKind::Sketch { .. }
            | FeatureKind::Combine(_)
            | FeatureKind::Move(_)
            | FeatureKind::Mirror(_)
            | FeatureKind::Pattern(_)
            | FeatureKind::Align(_)
            | FeatureKind::Scale(_)
            | FeatureKind::Split(_)
            | FeatureKind::Chamfer(_)
            | FeatureKind::Shell(_)
            | FeatureKind::Fillet(_)
            | FeatureKind::OffsetFace(_)
            | FeatureKind::FaceDraft(_) => None,
            FeatureKind::Extrude(extrude) => Some(&mut extrude.operation),
            FeatureKind::Revolve(revolve) => Some(&mut revolve.operation),
            FeatureKind::Sweep(sweep) => Some(&mut sweep.operation),
            FeatureKind::Loft(loft) => Some(&mut loft.operation),
        }
    }

    /// The body it makes: an extrude's, a revolve's, a sweep's or a loft's
    /// [`Operation::NewBody`], or a split's new body.
    pub fn new_body(&self) -> Option<BodyId> {
        match self {
            FeatureKind::Split(split) => split.made_body(),
            _ => self.operation().and_then(Operation::new_body),
        }
    }

    /// The id it holds for the body it made before but doesn't make now,
    /// which no other body or feature gets: a split's new body while it
    /// keeps one side ([`Split::held_body`]), or that of a join, cut or
    /// intersect that was a new body ([`Operation::held_body`]). Making
    /// the body again brings it back with this id.
    ///
    /// [`Split::held_body`]: crate::Split::held_body
    pub fn held_body(&self) -> Option<BodyId> {
        match self {
            FeatureKind::Split(split) => split.held_body(),
            _ => self.operation().and_then(Operation::held_body),
        }
    }

    /// The same, to change.
    pub(crate) fn new_body_mut(&mut self) -> Option<&mut BodyId> {
        match self {
            FeatureKind::Split(split) if split.keeps_both() => split.new_body.as_mut(),
            FeatureKind::Split(_) => None,
            _ => match self.operation_mut()? {
                Operation::NewBody(body) => Some(body),
                _ => None,
            },
        }
    }
}

impl From<Extrude> for FeatureKind {
    fn from(extrude: Extrude) -> Self {
        FeatureKind::Extrude(extrude)
    }
}

impl From<Revolve> for FeatureKind {
    fn from(revolve: Revolve) -> Self {
        FeatureKind::Revolve(revolve)
    }
}

impl From<Move> for FeatureKind {
    fn from(moved: Move) -> Self {
        FeatureKind::Move(moved)
    }
}

impl From<Mirror> for FeatureKind {
    fn from(mirror: Mirror) -> Self {
        FeatureKind::Mirror(mirror)
    }
}

impl From<Pattern> for FeatureKind {
    fn from(pattern: Pattern) -> Self {
        FeatureKind::Pattern(pattern)
    }
}

impl From<Combine> for FeatureKind {
    fn from(combine: Combine) -> Self {
        FeatureKind::Combine(combine)
    }
}

impl From<Align> for FeatureKind {
    fn from(align: Align) -> Self {
        FeatureKind::Align(Box::new(align))
    }
}

impl From<Scale> for FeatureKind {
    fn from(scale: Scale) -> Self {
        FeatureKind::Scale(scale)
    }
}

impl From<Split> for FeatureKind {
    fn from(split: Split) -> Self {
        FeatureKind::Split(split)
    }
}

impl From<Chamfer> for FeatureKind {
    fn from(chamfer: Chamfer) -> Self {
        FeatureKind::Chamfer(chamfer)
    }
}

impl From<Shell> for FeatureKind {
    fn from(shell: Shell) -> Self {
        FeatureKind::Shell(shell)
    }
}

impl From<Fillet> for FeatureKind {
    fn from(fillet: Fillet) -> Self {
        FeatureKind::Fillet(fillet)
    }
}

impl From<OffsetFace> for FeatureKind {
    fn from(offset: OffsetFace) -> Self {
        FeatureKind::OffsetFace(offset)
    }
}

impl From<FaceDraft> for FeatureKind {
    fn from(draft: FaceDraft) -> Self {
        FeatureKind::FaceDraft(draft)
    }
}

impl From<Sweep> for FeatureKind {
    fn from(sweep: Sweep) -> Self {
        FeatureKind::Sweep(sweep)
    }
}

impl From<Loft> for FeatureKind {
    fn from(loft: Loft) -> Self {
        FeatureKind::Loft(loft)
    }
}
