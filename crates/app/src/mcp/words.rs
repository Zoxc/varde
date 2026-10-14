//! The tools' word enums and their document or panel twins, each pair
//! declared once ([`mirror!`]): the setters take a word to the document's
//! value, the `feature` query the document's back to the word, so what
//! `feature` reads `set_feature` takes back.

use varde_document::{Axis3, BodyOp, Keep, LoftMode, OriginPlane, Side};
use varde_mcp::tools::{
    BodyOp as BodyOpWord, ExtrudeExtent, LoftMode as LoftModeWord, Operation as OperationWord,
    PatternMode as PatternModeWord, RevolveExtent, ScaleMode as ScaleModeWord,
    ShellDirection as ShellDirectionWord, SplitKeep, SplitSide, WorldAxis, WorldPlane,
};
use varde_view::{ExtentKind, OperationKind, PatternMode, ScaleMode, ShellDirection, TurnKind};

mirror! {
    pub(super) fn origin_plane(WorldPlane) -> OriginPlane / pub(super) fn plane_word {
        Xy = XY, Xz = XZ, Yz = YZ,
    }
    pub(super) fn world_axis(WorldAxis) -> Axis3 / pub(super) fn axis_word {
        X = X, Y = Y, Z = Z,
    }
    pub(super) fn operation(OperationWord) -> OperationKind / pub(super) fn operation_word {
        NewBody = NewBody, Join = Join, Cut = Cut, Intersect = Intersect,
    }
    pub(super) fn body_op(BodyOpWord) -> BodyOp / pub(super) fn body_op_word {
        Union = Union, Subtract = Subtract, Intersect = Intersect,
    }
    pub(super) fn pattern_mode(PatternModeWord) -> PatternMode / pub(super) fn pattern_mode_word {
        Spacing = Spacing, Total = Total, Full = Full,
    }
    pub(super) fn split_side(SplitSide) -> Side / pub(super) fn split_side_word {
        Front = Front, Back = Back,
    }
    pub(super) fn split_keep(SplitKeep) -> Keep / pub(super) fn split_keep_word {
        Both = Both, Front = Front, Back = Back,
    }
    pub(super) fn loft_mode(LoftModeWord) -> LoftMode / pub(super) fn loft_mode_word {
        Smooth = Smooth, Ruled = Ruled,
    }
    pub(super) fn extrude_extent(ExtrudeExtent) -> ExtentKind / pub(super) fn extrude_extent_word {
        OneSide = OneSide, Symmetric = Symmetric, TwoSides = TwoSides, ThroughAll = ThroughAll,
    }
    pub(super) fn revolve_extent(RevolveExtent) -> TurnKind / pub(super) fn revolve_extent_word {
        Full = Full, OneSide = OneSide, Symmetric = Symmetric, TwoSides = TwoSides,
    }
    pub(super) fn shell_direction(ShellDirectionWord) -> ShellDirection
        / pub(super) fn shell_direction_word {
        Inward = Inward, Outward = Outward,
    }
    pub(super) fn scale_mode(ScaleModeWord) -> ScaleMode / pub(super) fn scale_mode_word {
        Uniform = Uniform, PerAxis = PerAxis, EdgeLength = EdgeLength,
    }
}
