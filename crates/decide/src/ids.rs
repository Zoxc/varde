//! The ids of a B-rep's entities: `u32` indices into the store's columns,
//! [`NONE`] (`u32::MAX`) for "no entity". Records name the entities they
//! are about by these ids ([`crate::records::keys`]); the kernel's store
//! hands them out. Ids order work and break exact ties only, never choose
//! an outcome.
//!
//! `NONE` is the store's "no entity" and never names one: a record key
//! holding it is refused before anything is decided for it
//! ([`crate::records::table`]), and the store hands out ids below it.

/// The id that names no entity.
pub const NONE: u32 = u32::MAX;

macro_rules! ids {
    ($($(#[$doc:meta])* $name:ident => $kind:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl $name {
            /// No entity.
            pub const NONE: $name = $name(NONE);

            /// The id as an index into its kind's columns.
            #[inline]
            pub fn index(self) -> usize {
                self.0 as usize
            }

            /// Whether this is [`NONE`], naming no entity.
            #[inline]
            pub fn is_none(self) -> bool {
                self.0 == NONE
            }
        }

        impl From<$name> for Entity {
            fn from(id: $name) -> Entity {
                Entity::$kind(id)
            }
        }
    )*};
}

ids! {
    /// A vertex.
    VertId => Vert;
    /// An edge.
    EdgeId => Edge;
    /// A coedge.
    CoedgeId => Coedge;
    /// A loop.
    LoopId => Loop;
    /// A face.
    FaceId => Face;
    /// A surface.
    SurfId => Surf;
    /// A space curve.
    CurveId => Curve;
}

/// An entity of one body by its id, of any kind: what evidence names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Entity {
    /// A vertex.
    Vert(VertId),
    /// An edge.
    Edge(EdgeId),
    /// A coedge.
    Coedge(CoedgeId),
    /// A loop.
    Loop(LoopId),
    /// A face.
    Face(FaceId),
    /// A surface.
    Surf(SurfId),
    /// A space curve.
    Curve(CurveId),
}

impl Entity {
    /// Whether the id is [`NONE`], naming no entity.
    pub fn is_none(self) -> bool {
        match self {
            Entity::Vert(id) => id.is_none(),
            Entity::Edge(id) => id.is_none(),
            Entity::Coedge(id) => id.is_none(),
            Entity::Loop(id) => id.is_none(),
            Entity::Face(id) => id.is_none(),
            Entity::Surf(id) => id.is_none(),
            Entity::Curve(id) => id.is_none(),
        }
    }
}
