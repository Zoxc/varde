//! UI code: iced widgets and layout.
//!
//! The view is a pure function of application state. It produces
//! [`Message`]s but never mutates anything itself; that is the job of the
//! `app` crate.

// Proving wgpu types `Send + Sync` for the shader pipeline exceeds the default
// limit on recent nightlies.
#![recursion_limit = "256"]

mod anchors;
mod chrome;
mod combine;
mod constrain;
mod context_menu;
mod controls;
pub mod dimension;
mod divider;
mod document;
mod errors;
mod escape;
mod extrude;
mod feature_text;
mod file_menu;
mod handle_snap;
mod hit;
mod icons;
mod mcp;
mod mcp_config;
mod measure;
mod menu;
mod message;
mod motion;
mod mouse;
mod mouse_only;
mod names;
mod naming;
mod operation_panel;
mod options;
mod overflow_tip;
mod overlaps;
mod panels;
mod params;
mod pick;
mod plane_pick;
#[cfg(any(test, feature = "probe"))]
pub mod probe;
mod projection;
mod rail;
mod regenerating;
mod revolve;
mod rollback;
mod select;
mod settings;
mod shortcut;
mod snap;
pub mod spline;
mod status;
mod suggest;
#[cfg(test)]
mod testing;
mod theme;
mod thumbnail;
mod toast;
mod tool;
mod toolbar;
pub use file_menu::tolerance_label;
pub mod typed;
mod view_choices;
mod view_cube;
mod viewport;
mod welcome;
mod words;
mod wrap;

pub use combine::{CombineBody, CombineLook, CombinePick, CombineState};
pub use constrain::{ConstraintKind, ConstraintSet};
pub use document::{
    ActiveTool, CURVED_FACE, Damage, DamagedFile, DeletePrompt, DocumentState, FileShown, LinkRow,
    MeshStatus, NamePrompt, OperationState, Overlay, RecoveredChanges, Refusal, RefusedEdit,
    Rollback, SketchState, ValueField, ValueTarget, body_tint, document, part_opacity, part_tints,
};
pub use errors::{ShownError, ShownErrors};
pub use extrude::{Distance, ExtentKind, ExtrudeLook, ExtrudeState, Handle};
pub use feature_text::{
    align_info, chamfer_info, draft_info, fillet_info, loft_info, offset_info, scale_info,
    shell_info, split_info, sweep_info,
};
pub use handle_snap::snap_step;
pub use icons::LOGO_SVG;
pub use mcp::{
    MCP_PANEL_WIDTH, McpAsk, McpCall, McpCallState, McpEdit, McpPanel, McpRevert, McpRevertAsk,
    McpRow, McpShown, McpTool,
};
pub use mcp_config::{mcp_client_lines, mcp_port_of, mcp_stdio_lines};
pub use measure::{
    MeasureLook, MeasureSlot, MeasureState, MeasureValue, MeasuredPick, MeasuredPickKind, Outcome,
    between_values, edge_kind, face_kind, values as measure_values,
};
pub use message::{
    Edit, File, Location, Look, Message, RowMenu, SavePlace, Unsaved, ViewSubmenu, Welcome,
};
pub use motion::{
    AlignMark, AlignRole, AlignSide, AlignSlot, AlignView, BlendEdge, BlendEdges, BlendFace,
    ChamferType, ChamferView, DraftView, FilletView, KnobPath, KnobRadius, KnobScale, KnobSnap,
    KnobTone, LoftSection, LoftShape, LoftView, MotionField, MotionKind, MotionLook, MotionParts,
    MotionPick, MotionState, OffsetFaceView, OpKnob, PatternMode, PickedFace, PickedFaces,
    ScaleMode, ScaleView, ShellDirection, ShellView, SketchLines, SplitMode, SplitPiece, SplitView,
    SweepPart, SweepPath, SweepView, axis_name, direction_name, pattern_copies, plane_name,
    point_name,
};
pub use mouse::{DragAction, MouseOptions, PivotClick, Rotation, RotationPoint};
pub use naming::NamingPicks;
pub use operation_panel::{
    BodyTarget, Candidate, Framing, NO_REGIONS, OperationKind, PANEL_BODY, PanelHover, Side, Sides,
    TypedField, Verdict,
};
pub use options::{
    AUTO_SAVE_MINUTES, Edges, INTERFACE_SCALE_MAX, INTERFACE_SCALE_MIN, INTERFACE_SCALE_STEP,
    MCP_PORT_DEFAULT, MCP_PORT_MAX, MCP_PORT_MIN, RECENT_DESIGNS, ViewOptions,
    interface_scale_offered, mcp_port_offered,
};
pub use overlaps::{OverlapItem, OverlapItems, OverlapNote, OverlapTick, Overlaps};
pub use panels::{GeometryGroup, ObjectGroup, ObjectRow, OriginObject, Panel, TIMELINE_LIST};
pub use params::{ParamDraft, ParamEdit, ParamField, ParamsLook, ParamsState, param_name_id};
pub use pick::{
    EDGE_REACH, ModelHighlight, Pick, PickIndex, Picked, Picks, SNAP_REACH, Snapped, VERTEX_REACH,
};
pub use plane_pick::{PlanePick, face_name, plane_note};
pub use rail::{RAIL_LIST, RailLook, RailOpen, rail_rows, rail_sets};
pub use revolve::{
    Angle, EDGE_NOT_STRAIGHT, EDGE_OFF_PLANE, RevolveLook, RevolvePick, RevolveState, TurnKind,
    axis_edge,
};
pub use select::{Selected, Selection, SelectionMode, SketchItem};
pub use settings::{
    KeptIn, McpClient, McpState, McpStatus, Setting, SettingChange, SettingsPage, SettingsState,
    SettingsTab, changed_settings,
};
pub use shortcut::{
    Binding, DocumentKeys, Held, KeysInput, Operating, OperationKeys, claimed, document_bindings,
    escapes, pressed, settings_binding, welcome_bindings,
};
pub use snap::{Inference, Level, Own, SNAP_TOLERANCE, Snap, Target, closing};
pub use status::{STATUS_BAR_HEIGHT, STATUS_BAR_ROOM};
pub use suggest::{MAX_SUGGESTIONS, ParamsIn, Suggestion, suggest, take as take_suggestion};
pub use theme::{Mode, SIDE_PANEL_WIDTH, ThemeChoice, theme as iced_theme};
pub use thumbnail::{
    PreviewRequest, RENDER_SCALE, THUMBNAIL_ROOM, THUMBNAIL_SCALE, ThumbnailImages, thumbnail_shot,
};
pub use toast::toast;
pub use tool::{Tool, ToolClick};
pub use varde_regen::{Naming, Shown, Unnamed};
pub use viewport::ModelPicking;
pub use viewport::sketch_grid;
pub use viewport::warm_up;
pub use welcome::{
    CardKey, DamagedPrompt, DeleteFromBrowserPrompt, DesignCard, Downloads, PanicNote, RecentCard,
    SampleCard, StorageNote, WelcomeState, welcome,
};

/// The text field a dimension's value is typed in, placing it or editing
/// it in place: there's one at a time, focused as it opens.
pub const VALUE_FIELD: iced::widget::Id = iced::widget::Id::new("dimension-value");

/// The text field of the Save As dialog's name, on the web, focused as it
/// opens.
pub const NAME_FIELD: iced::widget::Id = iced::widget::Id::new("design-name");

/// The text field a feature, sketch or body is renamed in, in its row of
/// the side panel: there's one at a time, focused as it opens.
pub const RENAME_FIELD: iced::widget::Id = iced::widget::Id::new("rename");

/// What shows in the name's place of a design with no name, never saved.
pub const NOT_SAVED: &str = "Not saved";
