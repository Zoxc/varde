//! Features: the steps a design is built from, sketches, extrudes,
//! revolves, combines, moves, mirrors, patterns, aligns, scales, splits,
//! chamfers and shells.

use serde::{Deserialize, Serialize};
use varde_sketch::Sketch;

use crate::{
    Align, BodyId, Chamfer, Combine, Extrude, Mirror, Move, Operation, Pattern, Plane, Revolve,
    Scale, Shell, Split,
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
        }
    }

    /// The features it builds on, which come before it: an extrude's or a
    /// revolve's sketch, or the sketch a split takes regions or a line
    /// from. Removing one of them removes this too. Sorted,
    /// without repeats. The makers of the bodies it names
    /// ([`FeatureKind::bodies`]) aren't listed, as finding them takes the
    /// document, but [`Document::removal`](crate::Document::removal)
    /// follows them too. Not the maker of the body under a sketch's face
    /// plane ([`Plane::Face`]): removing it leaves the sketch, which then
    /// doesn't regenerate until it's put on another plane.
    pub fn uses(&self) -> Vec<FeatureId> {
        match self.sketch() {
            Some(sketch) => vec![sketch],
            None => Vec::new(),
        }
    }

    /// The bodies it names, which features before it make, and which it
    /// depends on: a combine's target and tools, the bodies a move, a
    /// mirror or a pattern moves or copies, the body an align moves, the
    /// bodies a scale scales (its edge length's edge among them), the
    /// body a split splits, its tool body and its face tool's body, the
    /// body a chamfer's edges are on, the body a shell hollows.
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
            FeatureKind::Sketch { .. } | FeatureKind::Extrude(_) | FeatureKind::Revolve(_) => {
                Vec::new()
            }
        }
    }

    /// The sketch whose regions it takes: an extrude's or a revolve's,
    /// or a split's regions or line, which adding it hides.
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
            | FeatureKind::Shell(_) => None,
            FeatureKind::Extrude(extrude) => Some(extrude.sketch),
            FeatureKind::Revolve(revolve) => Some(revolve.sketch),
            FeatureKind::Split(split) => split.tool.sketch(),
        }
    }

    /// What it does with the solid it makes: an extrude's or a revolve's
    /// operation. A combine has none: it makes no solid of its own.
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
            | FeatureKind::Shell(_) => None,
            FeatureKind::Extrude(extrude) => Some(&extrude.operation),
            FeatureKind::Revolve(revolve) => Some(&revolve.operation),
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
            | FeatureKind::Shell(_) => None,
            FeatureKind::Extrude(extrude) => Some(&mut extrude.operation),
            FeatureKind::Revolve(revolve) => Some(&mut revolve.operation),
        }
    }

    /// The body it makes: an extrude's or a revolve's
    /// [`Operation::NewBody`], or a split's new body.
    pub fn new_body(&self) -> Option<BodyId> {
        match self {
            FeatureKind::Split(split) => split.new_body,
            _ => self.operation().and_then(Operation::new_body),
        }
    }

    /// The same, to change.
    pub(crate) fn new_body_mut(&mut self) -> Option<&mut BodyId> {
        match self {
            FeatureKind::Split(split) => split.new_body.as_mut(),
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
