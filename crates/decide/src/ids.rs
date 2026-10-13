//! The ids of a B-rep's entities: `u32` indices into the store's columns,
//! [`NONE`] (`u32::MAX`) for "no entity". Records name the entities they
//! are about by these ids ([`crate::records::keys`]); the kernel's store
//! hands them out. Ids order work and break exact ties only, never choose
//! an outcome.

/// The id that names no entity.
pub const NONE: u32 = u32::MAX;

macro_rules! ids {
    ($($(#[$doc:meta])* $name:ident;)*) => {$(
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
        }
    )*};
}

ids! {
    /// A vertex.
    VertId;
    /// An edge.
    EdgeId;
    /// A coedge.
    CoedgeId;
    /// A loop.
    LoopId;
    /// A face.
    FaceId;
    /// A surface.
    SurfId;
    /// A space curve.
    CurveId;
}
