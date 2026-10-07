//! The keyboard shortcuts and held modifiers: what the app matches key
//! presses against and the labels the view shows for them, so the two
//! can't disagree, and which message each shortcut sends on which screen.

use std::borrow::Cow;

use iced::keyboard::{Key as KeyPress, Modifiers, key::Named};
use varde_document::{FeatureId, OriginPlane};

use crate::{
    CombineState, ConstraintKind, ConstraintSet, Edit, ExtrudeState, File, Look, Message,
    MotionKind, MotionState, RevolveState, SketchState, Tool, Welcome,
};

/// A key pressed on its own, or with the platform's command modifier
/// (`Ctrl`, or `Cmd` on macOS) and maybe Shift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    key: Key,
    shift: bool,
    command: bool,
}

/// The key of a [`Shortcut`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    /// An ASCII lower case letter, or a digit.
    Letter(char),
    Enter,
    /// `Delete`, or `Backspace`, which macOS calls delete.
    Delete,
    Escape,
    Space,
    Tab,
    /// The arrow keys up and down.
    Up,
    Down,
    /// `F2`.
    F2,
    /// `F10`.
    F10,
    /// The context menu key.
    Menu,
    /// No key: a tool the UI mock gives none, reached from the toolbar
    /// and the rail. Never pressed, and shown as nothing.
    None,
}

impl Shortcut {
    pub const NEW: Self = Self::plain('n');
    pub const OPEN: Self = Self::plain('o');
    pub const SAVE: Self = Self::command('s');
    pub const SAVE_AS: Self = Self {
        shift: true,
        ..Self::SAVE
    };
    pub const UNDO: Self = Self::command('z');
    pub const REDO: Self = Self {
        shift: true,
        ..Self::UNDO
    };
    /// Redo as Windows and Linux apps also have it.
    pub const REDO_Y: Self = Self::command('y');
    /// Starts a new sketch.
    pub const SKETCH: Self = Self::plain('s');
    /// Starts a new extrude: outside sketches, where `X` turns geometry
    /// into construction. The UI mock's key: `E` opens the model rail's
    /// third set.
    pub const EXTRUDE: Self = Self::plain('x');
    /// Starts a new revolve: outside sketches, where `O` takes up the
    /// Offset tool.
    pub const REVOLVE: Self = Self::plain('o');
    /// Starts the measure tool, or leaves it: outside sketches, where
    /// `I` is Coincident's.
    pub const MEASURE: Self = Self::plain('i');
    /// Starts a new combine: outside sketches, where `B` takes up the
    /// Rectangle tool.
    pub const COMBINE: Self = Self::plain('b');
    /// Starts a new move: outside sketches, the UI mock's key.
    pub const MOVE: Self = Self::plain('m');
    /// Starts a new linear pattern: outside sketches, where `P` takes up
    /// the Point tool. The UI mock's key (its circular pattern has none).
    pub const PATTERN: Self = Self::plain('p');
    /// Starts a new chamfer: outside sketches, where `C` takes up the
    /// Circle tool. The UI mock's key.
    pub const CHAMFER: Self = Self::plain('c');
    /// Starts a new fillet: outside sketches, where `F` takes up the
    /// Fillet tool. The UI mock's key.
    pub const FILLET: Self = Self::plain('f');
    /// No key: what a tool the UI mock gives none is bound to, never
    /// pressed (the mirror's).
    /// Picking an origin plane while picking the plane for a sketch: `1`
    /// XY, `2` XZ, `3` YZ, as the toolbar orders them.
    pub const PLANES: [Self; 3] = [Self::plain('1'), Self::plain('2'), Self::plain('3')];
    pub const NONE: Self = Self::named(Key::None);
    pub const ENTER: Self = Self::named(Key::Enter);
    pub const DELETE: Self = Self::named(Key::Delete);
    /// Renames the feature, sketch or body selected.
    pub const RENAME: Self = Self::named(Key::F2);
    /// Opens the context menu of what's selected.
    pub const MENU: Self = Self::named(Key::Menu);
    /// [`Shortcut::MENU`]'s other key, for keyboards without one.
    pub const MENU_F10: Self = Self {
        shift: true,
        ..Self::named(Key::F10)
    };
    /// Only labels the key: the app matches it itself, with any
    /// modifiers, since what it does depends on what's open (see
    /// `escape_key` there).
    pub const ESCAPE: Self = Self::named(Key::Escape);
    /// Clears the selection.
    pub const SPACE: Self = Self::named(Key::Space);
    /// Moves up and down the rail's open list.
    pub const UP: Self = Self::named(Key::Up);
    pub const DOWN: Self = Self::named(Key::Down);
    /// Turns geometry between normal and construction.
    pub const CONSTRUCTION: Self = Self::plain('x');
    /// Takes up the Constrain tool.
    pub const CONSTRAIN: Self = Self::plain('k');
    /// Turns the dimensions selected between driving and reference: with
    /// Shift, as `D` takes up the Dimension tool.
    pub const REFERENCE: Self = Self::shifted('d');
    /// Switches the Dimension tool between a circle's or an arc's radius
    /// and its diameter.
    pub const SWITCH_ROUND: Self = Self::named(Key::Tab);
    /// Moves to a drawing tool's next field. The key switching the
    /// Dimension tool's measure, which draws nothing.
    pub const NEXT_FIELD: Self = Self::SWITCH_ROUND;
    /// Switches the Rectangle tool between drawing from a corner and from
    /// the centre: a letter no tool or constraint has, by the left hand,
    /// under the rail's sets' `Q` to `R`.
    pub const CENTERED: Self = Self::plain('z');
    /// Switches the Spline tool, or the splines selected, between through
    /// fit points and by control points: the Rectangle tool's switch, as
    /// the two never go together.
    pub const SPLINE_KIND: Self = Self::CENTERED;
    /// Gives the fit points selected handles, or takes them away: with
    /// Shift, as `H` is Horizontal's.
    pub const HANDLES: Self = Self::shifted('h');
    /// Shows the curvature comb of the splines selected, or hides it: a
    /// letter no tool or constraint has (cUrvature).
    pub const COMB: Self = Self::plain('u');
    /// Closes the shape the Spline or Line tool is drawing, or the curves
    /// selected, or opens them: with Shift, as `O` is Offset's (clOse).
    pub const CLOSE: Self = Self::shifted('o');

    const fn plain(key: char) -> Self {
        assert!(key.is_ascii_lowercase() || key.is_ascii_digit());
        Self::named(Key::Letter(key))
    }

    const fn named(key: Key) -> Self {
        Self {
            key,
            shift: false,
            command: false,
        }
    }

    /// A letter on its own, `key` in lower case: picks a tool from the
    /// rail's open set.
    pub(crate) fn letter(key: char) -> Self {
        Self::plain(key)
    }

    /// Its letter, with or without Shift, if it's a letter without the
    /// command modifier.
    pub(crate) fn letter_key(self) -> Option<char> {
        match self.key {
            Key::Letter(letter) if !self.command => Some(letter),
            _ => None,
        }
    }

    /// Whether it's no key ([`Shortcut::NONE`]): nothing to show.
    pub(crate) fn is_none(self) -> bool {
        self.key == Key::None
    }

    /// Whether it's a key pressed on its own, with no modifier.
    pub(crate) fn is_plain(self) -> bool {
        !self.shift && !self.command
    }

    /// The letter with Shift held, where the letter alone is a tool's.
    const fn shifted(key: char) -> Self {
        Self {
            shift: true,
            ..Self::plain(key)
        }
    }

    const fn command(key: char) -> Self {
        Self {
            command: true,
            ..Self::plain(key)
        }
    }

    /// The shortcut as shown on a key chip or in a menu: `Ctrl Shift S`,
    /// or `Cmd Shift S` on macOS, matching [`Modifiers::command`].
    pub fn label(self) -> String {
        let command = if cfg!(target_os = "macos") {
            "Cmd "
        } else {
            "Ctrl "
        };
        let command = if self.command { command } else { "" };
        let shift = if self.shift { "Shift " } else { "" };
        let key: Cow<'_, str> = match self.key {
            Key::Letter(c) => c.to_ascii_uppercase().to_string().into(),
            Key::Enter => "Enter".into(),
            Key::Delete => "Del".into(),
            Key::Escape => "Esc".into(),
            Key::Space => "Space".into(),
            Key::Tab => "Tab".into(),
            Key::Up => "↑".into(),
            Key::Down => "↓".into(),
            Key::F2 => "F2".into(),
            Key::F10 => "F10".into(),
            Key::Menu => "Menu".into(),
            Key::None => "".into(),
        };
        format!("{command}{shift}{key}")
    }

    /// Whether pressing `key` with `modifiers` is this shortcut. Alt, and
    /// any modifier the shortcut doesn't ask for, rule it out.
    pub fn matches(self, key: &KeyPress, modifiers: Modifiers) -> bool {
        let command = if self.command {
            modifiers.command()
        } else {
            !modifiers.control() && !modifiers.logo()
        };
        let is_key = match (self.key, key) {
            (Key::Letter(letter), KeyPress::Character(c)) => {
                let mut chars = c.chars();
                chars
                    .next()
                    .is_some_and(|c| c.eq_ignore_ascii_case(&letter))
                    && chars.next().is_none()
            }
            (Key::Enter, KeyPress::Named(Named::Enter))
            | (Key::Delete, KeyPress::Named(Named::Delete | Named::Backspace))
            | (Key::Escape, KeyPress::Named(Named::Escape))
            | (Key::Space, KeyPress::Named(Named::Space))
            | (Key::Tab, KeyPress::Named(Named::Tab))
            | (Key::Up, KeyPress::Named(Named::ArrowUp))
            | (Key::Down, KeyPress::Named(Named::ArrowDown))
            | (Key::F2, KeyPress::Named(Named::F2))
            | (Key::F10, KeyPress::Named(Named::F10))
            | (Key::Menu, KeyPress::Named(Named::ContextMenu)) => true,
            (Key::Space, KeyPress::Character(c)) => c == " ",
            _ => false,
        };
        is_key && command && modifiers.shift() == self.shift && !modifiers.alt()
    }
}

/// A shortcut and the message it sends. The view shows the shortcut on the
/// control sending the message, and the app matches key presses against
/// it, so the key does what the control does: nothing while the control
/// is disabled.
#[derive(Debug, Clone)]
pub struct Binding {
    pub(crate) shortcut: Shortcut,
    message: Message,
    enabled: bool,
    /// Whether it keeps its key from the bindings after it even while
    /// disabled: the rail's open set's letters do, so a letter of a tool
    /// that can't be used there does nothing rather than what it does
    /// elsewhere.
    claims: bool,
}

impl Binding {
    pub(crate) fn new(shortcut: Shortcut, message: Message, enabled: bool) -> Self {
        Self {
            shortcut,
            message,
            enabled,
            claims: false,
        }
    }

    /// The same binding on `shortcut`, keeping its key even while
    /// disabled, see [`Binding::claims`].
    pub(crate) fn claiming(self, shortcut: Shortcut) -> Self {
        Self {
            shortcut,
            claims: true,
            ..self
        }
    }

    /// The message it sends, unless it's disabled.
    pub fn sends(&self) -> Option<Message> {
        self.enabled.then(|| self.message.clone())
    }
}

/// The welcome screen's shortcuts: New design and Open, in that order.
pub fn welcome_bindings() -> [Binding; 2] {
    [
        Binding::new(Shortcut::NEW, Message::Welcome(Welcome::NewDesign), true),
        Binding::new(Shortcut::OPEN, Message::Welcome(Welcome::Open), true),
    ]
}

/// The file's shortcuts: Save and Save As, in that order. Save is
/// disabled unless the document is `editable` and `edited` since it was
/// last saved.
pub fn file_bindings(editable: bool, edited: bool) -> [Binding; 2] {
    [
        Binding::new(
            Shortcut::SAVE,
            Message::File(File::Save),
            editable && edited,
        ),
        Binding::new(Shortcut::SAVE_AS, Message::File(File::SaveAs), true),
    ]
}

impl Tool {
    /// The key taking up the tool.
    pub fn shortcut(self) -> Shortcut {
        match self {
            Tool::Line => Shortcut::plain('l'),
            Tool::Circle => Shortcut::plain('c'),
            Tool::Arc => Shortcut::plain('a'),
            Tool::Point => Shortcut::plain('p'),
            // A box: `R` opens the rail's fourth set.
            Tool::Rectangle => Shortcut::plain('b'),
            // Polygon's `G`: its `P` is the Point tool's.
            Tool::Polygon => Shortcut::plain('g'),
            // A letter of its name: `S` is New sketch's, `P` the Point
            // tool's, `L` the Line tool's.
            Tool::Spline => Shortcut::plain('n'),
            Tool::Dimension => Shortcut::plain('d'),
            Tool::Trim => Shortcut::plain('t'),
            // A free letter: `E` opens a set of the rail's, `M` is Midpoint's.
            Tool::Extend => Shortcut::plain('j'),
            Tool::Offset => Shortcut::plain('o'),
            // `W` opens the rail's second set, and `M` is Midpoint's.
            Tool::Mirror => Shortcut::shifted('m'),
            Tool::Fillet => Shortcut::plain('f'),
            // Bevel, with Shift: `C` is the Circle tool's, `B` the
            // Rectangle tool's.
            Tool::Chamfer => Shortcut::shifted('b'),
            // No free letter of theirs: `P` is the Point tool's, `I`
            // Coincident's, and `Shift P` Parallel's.
            Tool::Project | Tool::Intersect => Shortcut::NONE,
        }
    }
}

impl ConstraintKind {
    /// The key applying the constraint to the selection: its initial, or
    /// a letter of its name, or with Shift where a tool or the rail's sets
    /// have the letter (Trim `T`, Fillet `F`, Point `P`, Circle `C`, New
    /// sketch `S`, the sets `E` and `R`).
    /// Recorded in `agents/sketch.md` and `README.md`. None for equal
    /// offsets, which only the Offset tool makes.
    pub fn shortcut(self) -> Option<Shortcut> {
        Some(match self {
            ConstraintKind::Coincident => Shortcut::plain('i'),
            ConstraintKind::Horizontal => Shortcut::plain('h'),
            ConstraintKind::Vertical => Shortcut::plain('v'),
            ConstraintKind::Parallel => Shortcut::shifted('p'),
            // A right angle.
            ConstraintKind::Perpendicular => Shortcut::shifted('r'),
            ConstraintKind::Tangent => Shortcut::shifted('t'),
            // `S` is New sketch's.
            ConstraintKind::Smooth => Shortcut::shifted('s'),
            // `E` opens the rail's third set.
            ConstraintKind::Equal => Shortcut::shifted('e'),
            ConstraintKind::Concentric => Shortcut::shifted('c'),
            ConstraintKind::Midpoint => Shortcut::plain('m'),
            ConstraintKind::Symmetric => Shortcut::plain('y'),
            ConstraintKind::Fix => Shortcut::shifted('f'),
            ConstraintKind::Offset => return None,
        })
    }
}

/// What the document screen's shortcuts depend on, besides the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DocumentKeys {
    /// Whether the document can be changed.
    pub editable: bool,
    /// Whether a sketch is being edited.
    pub sketching: bool,
    /// The feature selected in the Timeline, if any.
    pub selected: Option<FeatureId>,
    /// What `F2` renames: the feature selected in the Timeline, else the
    /// one sketch selected in Objects or the one body selected, outside
    /// sketches and operations.
    pub rename: Option<varde_document::Named>,
    /// The row whose context menu the context menu key opens: the
    /// feature selected in the Timeline, else the sketch or body selected
    /// in Objects or the body of what's selected in the model, or in a
    /// sketch the point or curve selected.
    pub menu: Option<crate::RowMenu>,
    /// Whether anything is selected in the sketch being edited.
    pub geometry_selected: bool,
    /// Whether a tool drawing shapes is in use in the sketch being edited.
    pub drawing: bool,
    /// Whether any tool is in use in the sketch being edited, the
    /// Constrain tool included: `Space` puts it down.
    pub tool: bool,
    /// Whether any dimensions are selected in the sketch being edited.
    pub dimensions_selected: bool,
    /// Whether the Dimension tool is placing a dimension of a circle or an
    /// arc, which can be its radius or its diameter.
    pub round_picked: bool,
    /// Whether the drawing tool's shape has fields to type values in, see
    /// [`ActiveTool::fields`](crate::ActiveTool::fields).
    pub fields: bool,
    /// Whether the Rectangle tool is in use.
    pub rectangle: bool,
    /// Whether the Spline tool is in use.
    pub spline: bool,
    /// Whether the Spline tool has placed enough points to end its spline.
    pub spline_ends: bool,
    /// Whether any splines are selected in the sketch being edited, and no
    /// drawing tool is in use.
    pub splines_selected: bool,
    /// Whether handles can be given to, or taken from, what's selected in
    /// the sketch being edited (see [`crate::spline::handles`]).
    pub handles: bool,
    /// Whether the drawing tool can close its shape (see
    /// [`ActiveTool::can_close`](crate::ActiveTool::can_close)).
    pub tool_closes: bool,
    /// Whether the curves selected in the sketch being edited can be
    /// closed (`Some(true)`) or opened (`Some(false)`), see
    /// [`crate::spline::closings`], with no drawing tool in use.
    pub closing: Option<bool>,
    /// Whether the Mirror tool has picked what it mirrors and can go on
    /// to the line to mirror about.
    pub mirror_picked: bool,
    /// The constraints that fit what's selected in the sketch being
    /// edited.
    pub constraints: ConstraintSet,
    /// Whether a face and nothing else is selected in the model, outside
    /// sketches, operations and picking a plane: a new sketch goes on it
    /// (if it's flat).
    pub face_selected: bool,
    /// Whether sketches are selected in Objects, and no feature in the
    /// Timeline: `Delete` removes them.
    pub objects_deletable: bool,
    /// Whether the plane for a sketch is being picked.
    pub picking_plane: bool,
    /// Whether an extrude is being set up.
    pub extruding: bool,
    /// Whether the extrude being set up can be committed.
    pub extrude_ready: bool,
    /// Whether a revolve is being set up.
    pub revolving: bool,
    /// Whether the revolve being set up can be committed.
    pub revolve_ready: bool,
    /// Whether the document has two bodies or more, to combine.
    pub combinable: bool,
    /// Whether a combine is being set up.
    pub combining: bool,
    /// Whether the combine being set up can be committed.
    pub combine_ready: bool,
    /// Whether the document has a body, to move or mirror.
    pub bodies: bool,
    /// The move, mirror or pattern being set up, if one is.
    pub motion: Option<MotionKind>,
    /// Whether the move or mirror being set up can be committed.
    pub motion_ready: bool,
    /// Whether the measure tool is in use.
    pub measuring: bool,
    /// Whether the document has changes not saved.
    pub edited: bool,
    /// Whether undo can take anything back.
    pub undo: bool,
    /// Whether redo can bring anything back.
    pub redo: bool,
    /// The rail's tool set whose list is open, if one is, and its row the
    /// keys are on: its letters, the arrows and `Enter` pick its tools,
    /// before any other key.
    pub rail: Option<crate::RailOpen>,
}

impl DocumentKeys {
    /// The keys of a document, which can be changed if `editable`, with
    /// `selected` selected in the Timeline and `sketch` being edited, if
    /// they are.
    pub fn new(
        editable: bool,
        selected: Option<FeatureId>,
        sketch: Option<SketchState<'_>>,
    ) -> Self {
        Self {
            editable,
            sketching: sketch.is_some(),
            selected,
            rename: None,
            menu: None,
            picking_plane: false,
            geometry_selected: sketch.is_some_and(|sketch| !sketch.selection.is_empty()),
            drawing: sketch.is_some_and(|sketch| sketch.tool.is_some_and(|tool| tool.tool.draws())),
            tool: sketch.is_some_and(|sketch| sketch.tool.is_some() || sketch.constraining),
            dimensions_selected: sketch.is_some_and(|sketch| {
                let mut selected = sketch.selection.iter();
                selected.any(|&id| {
                    id.item()
                        .is_some_and(|id| sketch.sketch.dimension(id).is_some())
                })
            }),
            round_picked: sketch.is_some_and(|sketch| {
                sketch.tool.is_some_and(|tool| {
                    tool.tool == Tool::Dimension
                        && crate::dimension::round(sketch.sketch, tool.picked)
                })
            }),
            fields: sketch
                .is_some_and(|sketch| sketch.tool.is_some_and(|tool| !tool.fields().is_empty())),
            rectangle: sketch
                .is_some_and(|sketch| sketch.tool.is_some_and(|tool| tool.tool == Tool::Rectangle)),
            spline: sketch
                .is_some_and(|sketch| sketch.tool.is_some_and(|tool| tool.tool == Tool::Spline)),
            spline_ends: sketch
                .is_some_and(|sketch| sketch.tool.is_some_and(|tool| tool.spline_ends())),
            splines_selected: sketch.is_some_and(|sketch| {
                !sketch.tool.is_some_and(|tool| tool.tool.draws())
                    && crate::spline::any_selected(sketch.sketch, sketch.selection)
            }),
            tool_closes: sketch
                .is_some_and(|sketch| sketch.tool.is_some_and(|tool| tool.can_close())),
            closing: sketch.and_then(|sketch| {
                if sketch.tool.is_some_and(|tool| tool.tool.draws()) {
                    return None;
                }
                crate::spline::closings(sketch.sketch, sketch.selection).map(|(close, _)| close)
            }),
            handles: sketch.is_some_and(|sketch| {
                crate::spline::handles(sketch.sketch, sketch.selection).is_some()
            }),
            mirror_picked: sketch.is_some_and(|sketch| {
                sketch.tool.is_some_and(|tool| {
                    tool.tool == Tool::Mirror && !tool.about && !tool.picked.is_empty()
                })
            }),
            constraints: sketch.map_or_else(ConstraintSet::default, |sketch| {
                ConstraintKind::fitting(sketch.sketch, sketch.selection)
                    .into_iter()
                    .collect()
            }),
            face_selected: false,
            objects_deletable: false,
            extruding: false,
            extrude_ready: false,
            revolving: false,
            revolve_ready: false,
            combinable: false,
            combining: false,
            combine_ready: false,
            bodies: false,
            motion: None,
            motion_ready: false,
            measuring: false,
            edited: false,
            undo: false,
            redo: false,
            rail: None,
        }
    }

    /// The same keys where a face and nothing else is selected in the
    /// model if `face_selected`.
    pub fn with_objects_deletable(self, objects_deletable: bool) -> Self {
        Self {
            objects_deletable,
            ..self
        }
    }

    pub fn with_face_selected(self, face_selected: bool) -> Self {
        Self {
            face_selected,
            ..self
        }
    }

    /// The same keys with the rail's set `rail` open, if one is.
    pub fn with_rail(self, rail: Option<crate::RailOpen>) -> Self {
        Self { rail, ..self }
    }

    /// The same keys where the document has changes not saved if
    /// `edited`.
    /// The same keys with `F2` renaming `rename`, if anything.
    pub fn with_rename(self, rename: Option<varde_document::Named>) -> Self {
        Self { rename, ..self }
    }

    /// The same keys with the context menu key opening `menu`'s, if
    /// anything's.
    pub fn with_menu(self, menu: Option<crate::RowMenu>) -> Self {
        Self { menu, ..self }
    }

    pub fn with_edited(self, edited: bool) -> Self {
        Self { edited, ..self }
    }

    /// The same keys where undo can take something back if `undo`, and
    /// redo bring something back if `redo`.
    pub fn with_history(self, undo: bool, redo: bool) -> Self {
        Self { undo, redo, ..self }
    }

    /// The same keys with `extrude` being set up, if one is.
    pub fn with_extrude(self, extrude: Option<&ExtrudeState<'_>>) -> Self {
        Self {
            extruding: extrude.is_some(),
            extrude_ready: extrude.is_some_and(|extrude| extrude.ready),
            ..self
        }
    }
}

impl DocumentKeys {
    /// The same keys with `revolve` being set up, if one is.
    pub fn with_revolve(self, revolve: Option<&RevolveState<'_>>) -> Self {
        Self {
            revolving: revolve.is_some(),
            revolve_ready: revolve.is_some_and(|revolve| revolve.ready),
            ..self
        }
    }

    /// The same keys where there are bodies to combine if `combinable`,
    /// with `combine` being set up, if one is.
    pub fn with_combine(self, combinable: bool, combine: Option<&CombineState<'_>>) -> Self {
        Self {
            combinable,
            combining: combine.is_some(),
            combine_ready: combine.is_some_and(|combine| combine.ready),
            ..self
        }
    }

    /// The same keys where the document has a body if `bodies`, with
    /// `motion`, a move, mirror or pattern, being set up, if one is.
    pub fn with_motion(self, bodies: bool, motion: Option<&MotionState<'_>>) -> Self {
        Self {
            bodies,
            motion: motion.map(|motion| motion.kind),
            motion_ready: motion.is_some_and(|motion| motion.ready),
            ..self
        }
    }

    /// The same keys with the plane for a sketch being picked if `picking`.
    pub fn with_picking_plane(self, picking_plane: bool) -> Self {
        Self {
            picking_plane,
            ..self
        }
    }

    /// The same keys with the measure tool in use if `measuring`.
    pub fn with_measure(self, measuring: bool) -> Self {
        Self { measuring, ..self }
    }

    /// Whether an operation is being set up: an extrude, a revolve, a
    /// combine, a move or a mirror.
    pub fn operating(&self) -> bool {
        self.extruding || self.revolving || self.combining || self.motion.is_some()
    }
}

/// Undo and Redo, in that order, then Redo's other key: while there's
/// something to take back or bring back, in a document that can be
/// changed.
pub fn history_bindings(keys: DocumentKeys) -> [Binding; 3] {
    let redo = |shortcut| {
        Binding::new(
            shortcut,
            Message::Edit(Edit::Redo),
            keys.editable && keys.redo,
        )
    };
    [
        Binding::new(
            Shortcut::UNDO,
            Message::Edit(Edit::Undo),
            keys.editable && keys.undo,
        ),
        redo(Shortcut::REDO),
        redo(Shortcut::REDO_Y),
    ]
}

/// Starting a new sketch: on the face selected in the model if a face
/// alone is, else asking for its plane first (or backing out of that).
/// Disabled in a sketch, and unless the document can be changed; an
/// operation being set up is dropped for it.
pub fn sketch_binding(keys: DocumentKeys) -> Binding {
    let message = if keys.face_selected {
        Message::Edit(Edit::SketchOnSelection)
    } else {
        Message::Look(Look::PickPlane)
    };
    Binding::new(Shortcut::SKETCH, message, keys.editable && !keys.sketching)
}

/// Picking the origin `plane` for a sketch, while one is picked: its key of
/// [`Shortcut::PLANES`], in a document that can be changed.
pub fn plane_binding(plane: OriginPlane, keys: DocumentKeys) -> Binding {
    let index = OriginPlane::ALL
        .iter()
        .position(|&p| p == plane)
        .unwrap_or(0);
    Binding::new(
        Shortcut::PLANES[index],
        Message::Edit(Edit::PlanePicked(plane)),
        keys.editable,
    )
}

/// Starting a new extrude, or backing out of the one being set up:
/// outside a sketch, in a document that can be changed. Another operation
/// being set up is dropped for it, and with no sketch yet the panel waits
/// for one.
pub fn extrude_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::EXTRUDE,
        Message::Look(Look::StartExtrude),
        keys.editable && !keys.sketching,
    )
}

/// Starting a new revolve, or backing out of the one being set up, as
/// [`extrude_binding`] does an extrude.
pub fn revolve_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::REVOLVE,
        Message::Look(Look::StartRevolve),
        keys.editable && !keys.sketching,
    )
}

/// Starting a new combine, or backing out of the one being set up:
/// outside a sketch and the other operations being set up, while the
/// document has two bodies or more, in a document that can be changed.
pub fn combine_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::COMBINE,
        Message::Look(Look::StartCombine),
        keys.editable
            && !keys.sketching
            && !keys.extruding
            && !keys.revolving
            && keys.motion.is_none()
            && (keys.combinable || keys.combining),
    )
}

/// Starting a new move, or backing out of the one being set up: outside
/// a sketch, while the document has a body, in a document that can be
/// changed. Another operation being set up is dropped for it.
pub fn move_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::MOVE,
        Message::Look(Look::StartMove),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new mirror, or backing out of the one being set up, as
/// [`move_binding`] does a move: with no key, as the UI mock has it.
pub fn mirror_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartMirror),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new linear pattern, or backing out of the one being set
/// up, as [`move_binding`] does a move: `P`, the UI mock's key.
pub fn pattern_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::PATTERN,
        Message::Look(Look::StartPattern),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new circular pattern, or backing out of the one being set
/// up, as [`move_binding`] does a move: with no key, as the UI mock has
/// it.
pub fn circular_pattern_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartCircularPattern),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new align, or backing out of the one being set up, as
/// [`move_binding`] does a move: with no key, as the UI mock has it.
pub fn align_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartAlign),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new scale, or backing out of the one being set up, as
/// [`move_binding`] does a move: with no key, as the UI mock has it.
pub fn scale_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartScale),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new split, or backing out of the one being set up, as
/// [`move_binding`] does a move: with no key, as the icon mock has it.
pub fn split_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartSplit),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new chamfer, or backing out of the one being set up, as
/// [`move_binding`] does a move: `C`, the UI mock's key.
pub fn chamfer_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::CHAMFER,
        Message::Look(Look::StartChamfer),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new fillet, or backing out of the one being set up, as
/// [`move_binding`] does a move: `F`, the UI mock's key.
pub fn fillet_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::FILLET,
        Message::Look(Look::StartFillet),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new shell, or backing out of the one being set up, as
/// [`move_binding`] does a move: with no key, as the UI mock has it.
pub fn shell_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartShell),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new offset face, or backing out of the one being set up,
/// as [`move_binding`] does a move: with no key, as the UI mock has it.
pub fn offset_face_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartOffsetFace),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new draft, or backing out of the one being set up, as
/// [`move_binding`] does a move: with no key, as the icon mock has it.
pub fn draft_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartDraft),
        keys.editable && !keys.sketching && (keys.bodies || keys.motion.is_some()),
    )
}

/// Starting a new sweep, or backing out of the one being set up, as
/// [`revolve_binding`] does a revolve: with no key, as the UI mock has
/// it (its Create group lists Sweep without one).
pub fn sweep_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartSweep),
        keys.editable && !keys.sketching,
    )
}

/// Starting a new loft, or backing out of the one being set up, as
/// [`sweep_binding`] does a sweep: with no key, as the UI mock has it
/// (its Create group lists Loft without one).
pub fn loft_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::NONE,
        Message::Look(Look::StartLoft),
        keys.editable && !keys.sketching,
    )
}

/// Starting the measure tool, or leaving it: outside a sketch and the
/// operations being set up. Measuring changes nothing, so a read-only
/// document is measured too.
pub fn measure_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::MEASURE,
        Message::Look(Look::StartMeasure),
        !keys.sketching && !keys.operating(),
    )
}

/// Taking up `tool`, or putting it down: in a sketch that can be changed.
pub fn tool_binding(tool: Tool, keys: DocumentKeys) -> Binding {
    Binding::new(
        tool.shortcut(),
        Message::Look(Look::SelectTool(tool)),
        keys.editable && keys.sketching,
    )
}

/// Taking up the Constrain tool, or putting it down: in a sketch that can
/// be changed.
pub fn constrain_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::CONSTRAIN,
        Message::Look(Look::ToggleConstrain),
        keys.editable && keys.sketching,
    )
}

/// Applying the constraint `kind` to the selection, or taking it off
/// what all has it already ([`Edit::ToggleConstraint`]): while it fits
/// it, in a sketch that can be changed. None for a kind with no key.
pub fn constraint_binding(kind: ConstraintKind, keys: DocumentKeys) -> Option<Binding> {
    let edit = Edit::ToggleConstraint(kind);
    Some(Binding::new(
        kind.shortcut()?,
        Message::Edit(edit),
        keys.editable && keys.sketching && keys.constraints.contains(kind),
    ))
}

/// Turning geometry between normal and construction: what's selected, or
/// the tool's next shapes, in a sketch that can be changed.
pub fn construction_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::CONSTRUCTION,
        Message::Edit(Edit::ToggleConstruction),
        keys.editable && keys.sketching && (keys.geometry_selected || keys.drawing),
    )
}

/// Turning the dimensions selected between driving and reference: while
/// any are, in a sketch that can be changed.
pub fn reference_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::REFERENCE,
        Message::Edit(Edit::ToggleReference),
        keys.editable && keys.sketching && keys.dimensions_selected,
    )
}

/// `Tab`: moving to the drawing tool's next field while its shape has
/// fields, switching the Dimension tool between radius and diameter while
/// it places one of a circle or an arc. The two never go together, as the
/// Dimension tool draws nothing.
fn tab_binding(keys: DocumentKeys) -> Binding {
    let message = if keys.fields {
        Look::NextField
    } else {
        Look::SwitchRound
    };
    Binding::new(
        Shortcut::NEXT_FIELD,
        Message::Look(message),
        keys.editable && (keys.fields || keys.round_picked),
    )
}

/// `Enter` in a sketch: placing the drawing tool's shape while it has
/// fields, or ending the Spline tool's spline, going on to the line to
/// mirror about once the Mirror tool has picked what it mirrors. They
/// never go together, as the Mirror tool draws nothing and the Spline
/// tool has no fields.
fn enter_binding(keys: DocumentKeys) -> Binding {
    let message = if keys.mirror_picked {
        Message::Look(Look::MirrorAbout)
    } else {
        Message::Edit(Edit::PlaceShape)
    };
    Binding::new(
        Shortcut::ENTER,
        message,
        keys.editable && (keys.fields || keys.mirror_picked || keys.spline_ends),
    )
}

/// `Z`: switching the Rectangle tool between from a corner and from the
/// centre, the Spline tool between through fit points and by control
/// points, or without a drawing tool the splines selected, converting
/// them. Only one of them at a time.
pub fn switch_binding(keys: DocumentKeys) -> Binding {
    let message = if keys.rectangle {
        Message::Look(Look::ToggleCentered)
    } else if keys.spline {
        Message::Look(Look::ToggleSplineKind)
    } else {
        Message::Edit(Edit::ConvertSplines)
    };
    Binding::new(
        Shortcut::CENTERED,
        message,
        keys.editable && (keys.rectangle || keys.spline || keys.splines_selected),
    )
}

/// Giving the fit points selected handles, or taking them away: while
/// they can be, in a sketch that can be changed, and no drawing tool is
/// in use, as converting splines waits for one.
pub fn handles_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::HANDLES,
        Message::Edit(Edit::ToggleHandles),
        keys.editable && keys.sketching && keys.handles && !keys.drawing,
    )
}

/// Closing the shape the tool draws, or closing or opening the curves
/// selected: while either can be, in a sketch that can be changed.
pub fn close_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::CLOSE,
        Message::Edit(Edit::ToggleClosed),
        keys.editable
            && keys.sketching
            && (keys.tool_closes || (keys.closing.is_some() && !keys.drawing)),
    )
}

/// Showing the splines' curvature comb, or hiding it: in any sketch, as
/// it changes nothing.
pub fn comb_binding(keys: DocumentKeys) -> Binding {
    Binding::new(
        Shortcut::COMB,
        Message::Look(Look::ToggleComb),
        keys.sketching,
    )
}

/// The document screen's shortcuts: the file's (Save, Save As), undo and
/// redo, starting a
/// sketch, clearing the selection, outside a sketch editing and deleting
/// the feature selected in the Timeline, if there is one, renaming what
/// `F2` renames ([`DocumentKeys::rename`]), opening the context menu of
/// what's selected ([`DocumentKeys::menu`]) on the context menu key or
/// `Shift F10`, and in a sketch
/// its tools, deleting what's selected, construction, the Constrain tool,
/// the constraints that fit the selection, turning dimensions between
/// driving and reference, `Tab` (see `tab_binding`), placing the shape
/// being drawn, going on to the Mirror tool's line, switching the
/// Rectangle tool between from a corner and from the centre (and the
/// Spline tool and splines between their kinds), handles and the
/// curvature comb. The rail's open set's keys come before them all, and
/// its sets' keys after (see the rail's module).
pub fn document_bindings(keys: DocumentKeys) -> Vec<Binding> {
    let feature = keys
        .selected
        .filter(|_| !keys.sketching && !keys.operating());
    let feature = feature.into_iter().flat_map(|id| {
        [
            Binding::new(Shortcut::ENTER, Message::Look(Look::EditFeature(id)), true),
            Binding::new(
                Shortcut::DELETE,
                Message::Edit(Edit::RemoveFeature(id)),
                keys.editable,
            ),
        ]
    });
    let objects =
        (keys.objects_deletable && keys.selected.is_none() && !keys.sketching).then(|| {
            Binding::new(
                Shortcut::DELETE,
                Message::Edit(Edit::RemoveObjects),
                keys.editable && !keys.operating(),
            )
        });
    let rename = (keys.rename)
        .filter(|_| !keys.sketching && !keys.operating())
        .map(|target| {
            Binding::new(
                Shortcut::RENAME,
                Message::Look(Look::StartRename(target)),
                keys.editable,
            )
        });
    let menu = (keys.menu)
        .filter(|_| !keys.operating())
        .into_iter()
        .flat_map(|target| {
            [Shortcut::MENU, Shortcut::MENU_F10]
                .map(|shortcut| Binding::new(shortcut, Message::Look(Look::KeyMenu(target)), true))
        });
    let feature = feature.chain(objects).chain(rename).chain(menu);
    let sketch = keys.sketching.then(|| {
        Tool::ALL
            .map(|tool| tool_binding(tool, keys))
            .into_iter()
            .chain([
                Binding::new(
                    Shortcut::DELETE,
                    Message::Edit(Edit::DeleteSelection),
                    keys.editable && keys.geometry_selected,
                ),
                construction_binding(keys),
                constrain_binding(keys),
                reference_binding(keys),
                tab_binding(keys),
                enter_binding(keys),
                switch_binding(keys),
                handles_binding(keys),
                close_binding(keys),
                comb_binding(keys),
            ])
            .chain(
                ConstraintKind::ALL
                    .into_iter()
                    .filter_map(move |kind| constraint_binding(kind, keys)),
            )
    });
    // Outside a sketch, where `X` is construction's, `O` Offset's, `B`
    // the Rectangle's, `F` the Fillet tool's, `C` the Circle's and `I`
    // Coincident's.
    let extrude = (!keys.sketching).then(|| {
        [
            extrude_binding(keys),
            revolve_binding(keys),
            combine_binding(keys),
            move_binding(keys),
            pattern_binding(keys),
            fillet_binding(keys),
            chamfer_binding(keys),
            measure_binding(keys),
        ]
    });
    let commit = keys.extruding.then(|| {
        Binding::new(
            Shortcut::ENTER,
            Message::Edit(Edit::CommitExtrude),
            keys.editable && keys.extrude_ready,
        )
    });
    let commit_revolve = keys.revolving.then(|| {
        Binding::new(
            Shortcut::ENTER,
            Message::Edit(Edit::CommitRevolve),
            keys.editable && keys.revolve_ready,
        )
    });
    let commit_combine = keys.combining.then(|| {
        Binding::new(
            Shortcut::ENTER,
            Message::Edit(Edit::CommitCombine),
            keys.editable && keys.combine_ready,
        )
    });
    let commit_motion = keys.motion.is_some().then(|| {
        Binding::new(
            Shortcut::ENTER,
            Message::Edit(Edit::CommitMotion),
            keys.editable && keys.motion_ready,
        )
    });
    let planes = keys
        .picking_plane
        .then(|| OriginPlane::ALL.map(|plane| plane_binding(plane, keys)));
    planes
        .into_iter()
        .flatten()
        .chain(crate::rail::letter_bindings(keys))
        .chain(file_bindings(keys.editable, keys.edited))
        .chain(history_bindings(keys))
        .chain([sketch_binding(keys), space_binding(keys)])
        .chain(extrude.into_iter().flatten())
        .chain(commit)
        .chain(commit_revolve)
        .chain(commit_combine)
        .chain(commit_motion)
        .chain(feature)
        .chain(sketch.into_iter().flatten())
        .chain(crate::rail::set_bindings(keys.sketching))
        .collect()
}

/// `Space`: puts down the tool in use in the sketch being edited, if one
/// is, else clears the selection. A value field with the focus takes
/// `Space` itself, so it only gets here with none.
fn space_binding(keys: DocumentKeys) -> Binding {
    let message = if keys.sketching && keys.tool {
        Look::PutDownTool
    } else {
        Look::ClearSelection
    };
    Binding::new(Shortcut::SPACE, Message::Look(message), true)
}

/// Whether pressing `key` with `modifiers` backs out as `Esc` does: `Esc`
/// with any modifiers, or `Tab` alone, which the app leaves to bindings
/// taking it where there are any (see [`claimed`]).
pub fn escapes(key: &KeyPress, modifiers: Modifiers) -> bool {
    match key {
        KeyPress::Named(Named::Escape) => true,
        KeyPress::Named(Named::Tab) => modifiers.is_empty(),
        _ => false,
    }
}

/// Whether a binding of `bindings` takes pressing `key` with `modifiers`,
/// enabled or claiming it, see [`pressed`].
pub fn claimed(
    bindings: impl IntoIterator<Item = Binding>,
    key: &KeyPress,
    modifiers: Modifiers,
) -> bool {
    bindings.into_iter().any(|binding| {
        (binding.enabled || binding.claims) && binding.shortcut.matches(key, modifiers)
    })
}

/// The message of the first enabled binding of `bindings` that pressing
/// `key` with `modifiers` is, if any, unless a disabled one that
/// claims the key comes before it (the rail's open set's letters do).
pub fn pressed(
    bindings: impl IntoIterator<Item = Binding>,
    key: &KeyPress,
    modifiers: Modifiers,
) -> Option<Message> {
    bindings
        .into_iter()
        .find(|binding| {
            (binding.enabled || binding.claims) && binding.shortcut.matches(key, modifiers)
        })
        .and_then(|binding| binding.enabled.then_some(binding.message))
}

/// A modifier held down, rather than a key pressed.
#[derive(Debug, Clone, Copy)]
pub struct Held {
    is_held: fn(Modifiers) -> bool,
    label: &'static str,
    /// The label on macOS.
    mac_label: &'static str,
}

impl Held {
    /// Held to peek at the other side panel tab.
    pub const PEEK: Self = Self {
        is_held: Modifiers::alt,
        label: "Alt",
        mac_label: "Option",
    };

    /// Held while placing a dimension with the Dimension tool to place it
    /// as a reference. The peek key: in the Dimension tool it doesn't peek.
    pub const REFERENCE: Self = Self::PEEK;

    /// Held while clicking the model to add what's clicked to the
    /// selection or take it out: `Shift`, or `Ctrl` (`Cmd` on macOS) as
    /// in a sketch.
    pub const TOGGLE: Self = Self {
        is_held: |modifiers| modifiers.shift() || modifiers.command(),
        label: "Shift",
        mac_label: "Shift",
    };

    /// Held to orbit with the right mouse button, which pans otherwise.
    pub const ORBIT: Self = Self {
        is_held: Modifiers::shift,
        label: "Shift",
        mac_label: "Shift",
    };

    /// Held while clicking with a drawing tool to place the point where
    /// the cursor is, snapping to nothing. The orbit key, which orbits
    /// only with the right button.
    pub const FREE: Self = Self::ORBIT;

    /// Whether it's held among `modifiers`.
    pub fn is_held(self, modifiers: Modifiers) -> bool {
        (self.is_held)(modifiers)
    }

    /// The modifier as shown on a key chip: `Alt`, or `Option` on macOS.
    pub fn label(self) -> &'static str {
        if cfg!(target_os = "macos") {
            self.mac_label
        } else {
            self.label
        }
    }
}

/// A key as the view shows it, on a key chip or in a menu: a shortcut or a
/// held modifier, so the view can only show keys the app matches.
#[derive(Debug, Clone, Copy)]
pub enum KeyName {
    Press(Shortcut),
    Held(Held),
}

impl From<Shortcut> for KeyName {
    fn from(shortcut: Shortcut) -> Self {
        KeyName::Press(shortcut)
    }
}

impl From<Held> for KeyName {
    fn from(held: Held) -> Self {
        KeyName::Held(held)
    }
}

impl KeyName {
    pub fn label(self) -> Cow<'static, str> {
        match self {
            KeyName::Press(shortcut) => shortcut.label().into(),
            KeyName::Held(held) => held.label().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: &str) -> KeyPress {
        KeyPress::Character(c.into())
    }

    #[test]
    fn labels() {
        assert_eq!(Shortcut::NEW.label(), "N");
        let command = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };
        assert_eq!(Shortcut::SAVE.label(), format!("{command} S"));
        assert_eq!(Shortcut::SAVE_AS.label(), format!("{command} Shift S"));
        assert_eq!(Shortcut::ENTER.label(), "Enter");
        assert_eq!(Shortcut::DELETE.label(), "Del");
        assert_eq!(Shortcut::ESCAPE.label(), "Esc");
        assert_eq!(Shortcut::SPACE.label(), "Space");
        assert_eq!(Tool::Line.shortcut().label(), "L");
    }

    #[test]
    fn matches() {
        let none = Modifiers::empty();
        assert!(Shortcut::NEW.matches(&key("n"), none));
        assert!(!Shortcut::NEW.matches(&key("n"), Modifiers::CTRL));
        assert!(!Shortcut::NEW.matches(&key("N"), Modifiers::SHIFT));
        assert!(!Shortcut::NEW.matches(&key("o"), none));
        assert!(!Shortcut::NEW.matches(&key("nn"), none));
        assert!(!Shortcut::NEW.matches(&key(""), none));

        let command = Modifiers::COMMAND;
        assert!(Shortcut::SAVE.matches(&key("s"), command));
        assert!(!Shortcut::SAVE.matches(&key("s"), none));
        assert!(!Shortcut::SAVE.matches(&key("s"), command | Modifiers::ALT));
        assert!(!Shortcut::SAVE.matches(&key("S"), command | Modifiers::SHIFT));
        assert!(Shortcut::SAVE_AS.matches(&key("S"), command | Modifiers::SHIFT));
        assert!(!Shortcut::SAVE_AS.matches(&key("s"), command));
    }

    #[test]
    fn named_keys_match() {
        let none = Modifiers::empty();
        let named = KeyPress::Named;
        assert!(Shortcut::ENTER.matches(&named(Named::Enter), none));
        assert!(!Shortcut::ENTER.matches(&named(Named::Enter), Modifiers::CTRL));
        assert!(Shortcut::DELETE.matches(&named(Named::Delete), none));
        assert!(Shortcut::DELETE.matches(&named(Named::Backspace), none));
        assert!(Shortcut::ESCAPE.matches(&named(Named::Escape), none));
        assert!(!Shortcut::ESCAPE.matches(&named(Named::Enter), none));
        assert!(!Shortcut::ENTER.matches(&key("e"), none));
        assert!(!Shortcut::NEW.matches(&named(Named::Enter), none));
        assert!(Shortcut::SPACE.matches(&named(Named::Space), none));
        assert!(Shortcut::SPACE.matches(&key(" "), none));
        assert!(!Shortcut::SPACE.matches(&named(Named::Space), Modifiers::CTRL));
    }

    #[test]
    fn peek() {
        assert!(Held::PEEK.is_held(Modifiers::ALT));
        assert!(Held::PEEK.is_held(Modifiers::ALT | Modifiers::SHIFT));
        assert!(!Held::PEEK.is_held(Modifiers::CTRL));
    }

    fn keys(editable: bool) -> DocumentKeys {
        DocumentKeys {
            editable,
            ..DocumentKeys::default()
        }
    }

    #[test]
    fn undo_and_redo_have_keys_while_there_s_something_to_do() {
        let command = Modifiers::COMMAND;
        let shift = command | Modifiers::SHIFT;
        let history = |undo, redo| keys(true).with_history(undo, redo);
        let undo = |keys| pressed(document_bindings(keys), &key("z"), command);
        assert!(matches!(
            undo(history(true, false)),
            Some(Message::Edit(Edit::Undo))
        ));
        assert!(undo(history(false, true)).is_none());
        assert!(undo(keys(false).with_history(true, true)).is_none());
        for (letter, modifiers) in [("Z", shift), ("y", command)] {
            let redo = |keys| pressed(document_bindings(keys), &key(letter), modifiers);
            assert!(matches!(
                redo(history(false, true)),
                Some(Message::Edit(Edit::Redo))
            ));
            assert!(redo(history(true, false)).is_none());
        }
        // `Z` alone is the Rectangle tool's switch, not undo.
        let plain = pressed(
            document_bindings(history(true, true)),
            &key("z"),
            Modifiers::empty(),
        );
        assert!(!matches!(plain, Some(Message::Edit(Edit::Undo))));
    }

    #[test]
    fn a_disabled_binding_is_a_dead_key() {
        let command = Modifiers::COMMAND;
        let shift = command | Modifiers::SHIFT;
        let save = |editable, edited| {
            let keys = keys(editable).with_edited(edited);
            pressed(document_bindings(keys), &key("s"), command)
        };
        assert!(matches!(save(true, true), Some(Message::File(File::Save))));
        assert!(save(false, true).is_none());
        // Nothing to save.
        assert!(save(true, false).is_none());
        let save_as = pressed(document_bindings(keys(false)), &key("S"), shift);
        assert!(matches!(save_as, Some(Message::File(File::SaveAs))));
    }

    #[test]
    fn a_sketch_is_started_outside_sketches_only() {
        let sketch = |keys| pressed(document_bindings(keys), &key("s"), Modifiers::empty());
        assert!(matches!(
            sketch(keys(true)),
            Some(Message::Look(Look::PickPlane))
        ));
        assert!(sketch(keys(false)).is_none());
        // On the face selected, if one alone is.
        let face = DocumentKeys {
            face_selected: true,
            ..keys(true)
        };
        assert!(matches!(
            sketch(face),
            Some(Message::Edit(Edit::SketchOnSelection))
        ));
        let sketching = DocumentKeys {
            sketching: true,
            ..keys(true)
        };
        assert!(sketch(sketching).is_none());
    }

    #[test]
    fn picking_a_plane_takes_the_origin_planes_by_number() {
        let none = Modifiers::empty();
        let two = KeyPress::Character("2".into());
        let picking = DocumentKeys {
            picking_plane: true,
            ..keys(true)
        };
        assert!(matches!(
            pressed(document_bindings(picking), &two, none),
            Some(Message::Edit(Edit::PlanePicked(OriginPlane::XZ)))
        ));
        assert!(pressed(document_bindings(keys(true)), &two, none).is_none());
    }

    #[test]
    fn the_selected_feature_is_edited_and_deleted() {
        let id = {
            let mut editor = varde_document::Editor::new(Default::default());
            let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
            editor.apply(editor.document().add_sketch(plane)).unwrap();
            editor.document().features()[0].id
        };
        let none = Modifiers::empty();
        let enter = KeyPress::Named(Named::Enter);
        let delete = KeyPress::Named(Named::Delete);
        let selected = DocumentKeys {
            selected: Some(id),
            ..keys(true)
        };
        assert!(matches!(
            pressed(document_bindings(selected), &enter, none),
            Some(Message::Look(Look::EditFeature(edited))) if edited == id
        ));
        assert!(matches!(
            pressed(document_bindings(selected), &delete, none),
            Some(Message::Edit(Edit::RemoveFeature(removed))) if removed == id
        ));
        // Read-only, it can be looked into but not deleted.
        let read_only = DocumentKeys {
            editable: false,
            ..selected
        };
        assert!(pressed(document_bindings(read_only), &enter, none).is_some());
        assert!(pressed(document_bindings(read_only), &delete, none).is_none());
        // Nothing selected, or in a sketch, neither does anything.
        assert!(pressed(document_bindings(keys(true)), &enter, none).is_none());
        let sketching = DocumentKeys {
            sketching: true,
            ..selected
        };
        assert!(pressed(document_bindings(sketching), &enter, none).is_none());
        assert!(pressed(document_bindings(sketching), &delete, none).is_none());
    }

    #[test]
    fn tools_are_taken_up_in_a_sketch_that_can_be_changed() {
        let sketching = DocumentKeys {
            sketching: true,
            ..keys(true)
        };
        for tool in Tool::ALL
            .into_iter()
            .filter(|tool| !tool.shortcut().is_none())
        {
            let label = tool.shortcut().label().to_lowercase();
            // Mirror and Chamfer take Shift.
            let (letter, held) = match label.strip_prefix("shift ") {
                Some(letter) => (letter.to_uppercase(), Modifiers::SHIFT),
                None => (label, Modifiers::empty()),
            };
            assert!(matches!(
                pressed(document_bindings(sketching), &key(&letter), held),
                Some(Message::Look(Look::SelectTool(taken))) if taken == tool
            ));
            let read_only = DocumentKeys {
                editable: false,
                ..sketching
            };
            assert!(pressed(document_bindings(read_only), &key(&letter), held).is_none());
            // Outside a sketch, a letter Extrude or Revolve shares is
            // theirs.
            assert!(!matches!(
                pressed(document_bindings(keys(true)), &key(&letter), held),
                Some(Message::Look(Look::SelectTool(_)))
            ));
        }
    }

    #[test]
    fn geometry_is_deleted_and_made_construction_once_selected() {
        let none = Modifiers::empty();
        let delete = KeyPress::Named(Named::Delete);
        let sketching = DocumentKeys {
            sketching: true,
            ..keys(true)
        };
        assert!(pressed(document_bindings(sketching), &delete, none).is_none());
        assert!(pressed(document_bindings(sketching), &key("x"), none).is_none());
        let selected = DocumentKeys {
            geometry_selected: true,
            ..sketching
        };
        assert!(matches!(
            pressed(document_bindings(selected), &delete, none),
            Some(Message::Edit(Edit::DeleteSelection))
        ));
        assert!(matches!(
            pressed(document_bindings(selected), &key("x"), none),
            Some(Message::Edit(Edit::ToggleConstruction))
        ));
        // A tool takes X for its next shapes.
        let drawing = DocumentKeys {
            drawing: true,
            ..sketching
        };
        assert!(pressed(document_bindings(drawing), &key("x"), none).is_some());
        let read_only = DocumentKeys {
            editable: false,
            ..selected
        };
        assert!(pressed(document_bindings(read_only), &delete, none).is_none());
        assert!(pressed(document_bindings(read_only), &key("x"), none).is_none());
    }

    #[test]
    fn constraints_are_applied_by_their_keys_while_they_fit() {
        let sketching = DocumentKeys {
            sketching: true,
            ..keys(true)
        };
        let shift = Modifiers::SHIFT;
        let tangent = |keys| pressed(document_bindings(keys), &key("T"), shift);
        assert!(tangent(sketching).is_none());
        let fitting = DocumentKeys {
            constraints: [ConstraintKind::Tangent].into_iter().collect(),
            ..sketching
        };
        assert!(matches!(
            tangent(fitting),
            Some(Message::Edit(Edit::ToggleConstraint(
                ConstraintKind::Tangent
            )))
        ));
        let read_only = DocumentKeys {
            editable: false,
            ..fitting
        };
        assert!(tangent(read_only).is_none());
        // K takes up the Constrain tool.
        assert!(matches!(
            pressed(document_bindings(sketching), &key("k"), Modifiers::empty()),
            Some(Message::Look(Look::ToggleConstrain))
        ));
    }

    #[test]
    fn no_two_of_a_sketch_s_keys_are_the_same() {
        let all = DocumentKeys {
            sketching: true,
            geometry_selected: true,
            dimensions_selected: true,
            round_picked: true,
            fields: true,
            rectangle: true,
            spline: true,
            spline_ends: true,
            splines_selected: true,
            handles: true,
            tool_closes: true,
            closing: Some(true),
            constraints: ConstraintKind::ALL.into_iter().collect(),
            ..keys(true)
        };
        // Project and Intersect have no key.
        let shortcuts: Vec<_> = document_bindings(all)
            .into_iter()
            .map(|binding| binding.shortcut)
            .filter(|shortcut| !shortcut.is_none())
            .collect();
        for (i, shortcut) in shortcuts.iter().enumerate() {
            assert!(
                !shortcuts[..i].contains(shortcut),
                "{} twice",
                shortcut.label()
            );
        }
        assert_eq!(ConstraintKind::Fix.shortcut().unwrap().label(), "Shift F");
        assert_eq!(ConstraintKind::Offset.shortcut(), None);
    }

    #[test]
    fn dimensions_turn_to_references_and_rounds_switch_by_their_keys() {
        let sketching = DocumentKeys {
            sketching: true,
            ..keys(true)
        };
        let shift_d = |keys| pressed(document_bindings(keys), &key("D"), Modifiers::SHIFT);
        let tab = |keys| {
            let tab = KeyPress::Named(Named::Tab);
            pressed(document_bindings(keys), &tab, Modifiers::empty())
        };
        assert!(shift_d(sketching).is_none());
        assert!(tab(sketching).is_none());
        let selected = DocumentKeys {
            dimensions_selected: true,
            ..sketching
        };
        assert!(matches!(
            shift_d(selected),
            Some(Message::Edit(Edit::ToggleReference))
        ));
        let placing = DocumentKeys {
            round_picked: true,
            ..sketching
        };
        assert!(matches!(
            tab(placing),
            Some(Message::Look(Look::SwitchRound))
        ));
        // D alone takes up the Dimension tool.
        assert!(matches!(
            pressed(document_bindings(selected), &key("d"), Modifiers::empty()),
            Some(Message::Look(Look::SelectTool(Tool::Dimension)))
        ));
        // And T, J, O, Shift M, F and Shift B the shape tools.
        let none = Modifiers::empty();
        let shapes = [
            ("t", none, Tool::Trim),
            ("j", none, Tool::Extend),
            ("o", none, Tool::Offset),
            ("M", Modifiers::SHIFT, Tool::Mirror),
            ("f", none, Tool::Fillet),
            ("B", Modifiers::SHIFT, Tool::Chamfer),
        ];
        for (letter, held, tool) in shapes {
            let sent = pressed(document_bindings(selected), &key(letter), held);
            assert!(
                matches!(sent, Some(Message::Look(Look::SelectTool(t))) if t == tool),
                "{letter}: {sent:?}"
            );
        }
        let enter = |keys| {
            let enter = KeyPress::Named(Named::Enter);
            pressed(document_bindings(keys), &enter, Modifiers::empty())
        };
        assert!(enter(sketching).is_none());
        let mirroring = DocumentKeys {
            mirror_picked: true,
            ..sketching
        };
        assert!(matches!(
            enter(mirroring),
            Some(Message::Look(Look::MirrorAbout))
        ));
        let read_only = DocumentKeys {
            editable: false,
            ..selected
        };
        assert!(shift_d(read_only).is_none());
        assert_eq!(Shortcut::REFERENCE.label(), "Shift D");
        assert_eq!(Shortcut::SWITCH_ROUND.label(), "Tab");
    }

    #[test]
    fn a_drawing_tool_s_fields_take_tab_and_enter_places_its_shape() {
        let sketching = DocumentKeys {
            sketching: true,
            ..keys(true)
        };
        let none = Modifiers::empty();
        let tab = KeyPress::Named(Named::Tab);
        let enter = KeyPress::Named(Named::Enter);
        assert!(pressed(document_bindings(sketching), &tab, none).is_none());
        assert!(pressed(document_bindings(sketching), &enter, none).is_none());
        let fields = DocumentKeys {
            fields: true,
            ..sketching
        };
        assert!(matches!(
            pressed(document_bindings(fields), &tab, none),
            Some(Message::Look(Look::NextField))
        ));
        assert!(matches!(
            pressed(document_bindings(fields), &enter, none),
            Some(Message::Edit(Edit::PlaceShape))
        ));
        let read_only = DocumentKeys {
            editable: false,
            ..fields
        };
        assert!(pressed(document_bindings(read_only), &tab, none).is_none());
        assert!(pressed(document_bindings(read_only), &enter, none).is_none());
        // Z switches the Rectangle tool only.
        let q = key("z");
        assert!(pressed(document_bindings(sketching), &q, none).is_none());
        let rectangle = DocumentKeys {
            rectangle: true,
            ..sketching
        };
        assert!(matches!(
            pressed(document_bindings(rectangle), &q, none),
            Some(Message::Look(Look::ToggleCentered))
        ));
        assert_eq!(Tool::Rectangle.shortcut().label(), "B");
        assert_eq!(Tool::Polygon.shortcut().label(), "G");
    }

    #[test]
    fn splines_take_their_keys() {
        let sketching = DocumentKeys {
            sketching: true,
            ..keys(true)
        };
        let none = Modifiers::empty();
        let sent =
            |keys, key: &KeyPress, modifiers| pressed(document_bindings(keys), key, modifiers);
        // N takes up the Spline tool, Z then switches its kind.
        assert!(matches!(
            sent(sketching, &key("n"), none),
            Some(Message::Look(Look::SelectTool(Tool::Spline)))
        ));
        assert!(sent(sketching, &key("z"), none).is_none());
        let drawing = DocumentKeys {
            spline: true,
            drawing: true,
            ..sketching
        };
        assert!(matches!(
            sent(drawing, &key("z"), none),
            Some(Message::Look(Look::ToggleSplineKind))
        ));
        // Enter ends it once it has enough.
        let enter = KeyPress::Named(Named::Enter);
        assert!(sent(drawing, &enter, none).is_none());
        let ends = DocumentKeys {
            spline_ends: true,
            ..drawing
        };
        assert!(matches!(
            sent(ends, &enter, none),
            Some(Message::Edit(Edit::PlaceShape))
        ));
        // Splines selected: Z converts them, Shift H handles them, and U
        // shows their comb, which it does in any sketch.
        let selected = DocumentKeys {
            splines_selected: true,
            handles: true,
            ..sketching
        };
        assert!(matches!(
            sent(selected, &key("z"), none),
            Some(Message::Edit(Edit::ConvertSplines))
        ));
        assert!(matches!(
            sent(selected, &key("H"), Modifiers::SHIFT),
            Some(Message::Edit(Edit::ToggleHandles))
        ));
        assert!(sent(sketching, &key("H"), Modifiers::SHIFT).is_none());
        for keys in [sketching, selected] {
            assert!(matches!(
                sent(keys, &key("u"), none),
                Some(Message::Look(Look::ToggleComb))
            ));
        }
        assert!(sent(keys(true), &key("u"), none).is_none());
        let read_only = DocumentKeys {
            editable: false,
            ..selected
        };
        assert!(sent(read_only, &key("z"), none).is_none());
        assert!(sent(read_only, &key("H"), Modifiers::SHIFT).is_none());
        assert_eq!(Tool::Spline.shortcut().label(), "N");
        assert_eq!(Shortcut::HANDLES.label(), "Shift H");
    }

    #[test]
    fn handles_wait_for_the_drawing_tool() {
        // Fit points selected while the Spline tool (or any drawing
        // tool) is drawing: Shift H is the tool's, as Z is, not theirs.
        let drawing = DocumentKeys {
            sketching: true,
            drawing: true,
            spline: true,
            handles: true,
            ..keys(true)
        };
        let shift_h = |keys| pressed(document_bindings(keys), &key("H"), Modifiers::SHIFT);
        assert!(shift_h(drawing).is_none());
        assert!(matches!(
            shift_h(DocumentKeys {
                drawing: false,
                spline: false,
                ..drawing
            }),
            Some(Message::Edit(Edit::ToggleHandles))
        ));
    }

    #[test]
    fn x_starts_an_extrude_outside_sketches_and_enter_commits_it() {
        let none = Modifiers::empty();
        let e = |keys| pressed(document_bindings(keys), &key("x"), none);
        let enter = |keys| {
            let enter = KeyPress::Named(Named::Enter);
            pressed(document_bindings(keys), &enter, none)
        };
        // With no sketch yet too: the panel waits for one.
        let extrudable = keys(true);
        assert!(matches!(
            e(extrudable),
            Some(Message::Look(Look::StartExtrude))
        ));
        let read_only = DocumentKeys {
            editable: false,
            ..extrudable
        };
        assert!(e(read_only).is_none());
        // In a sketch, X turns geometry into construction.
        let sketching = DocumentKeys {
            sketching: true,
            ..extrudable
        };
        assert!(!matches!(
            e(sketching),
            Some(Message::Look(Look::StartExtrude))
        ));

        // While setting one up, Enter is OK once it's ready, not editing
        // the feature selected, and S drops it for a new sketch.
        let id = {
            let mut editor = varde_document::Editor::new(Default::default());
            let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
            editor.apply(editor.document().add_sketch(plane)).unwrap();
            editor.document().features()[0].id
        };
        let extruding = DocumentKeys {
            extruding: true,
            selected: Some(id),
            ..extrudable
        };
        assert!(enter(extruding).is_none());
        assert!(matches!(
            pressed(document_bindings(extruding), &key("s"), none),
            Some(Message::Look(Look::PickPlane))
        ));
        let ready = DocumentKeys {
            extrude_ready: true,
            ..extruding
        };
        assert!(matches!(
            enter(ready),
            Some(Message::Edit(Edit::CommitExtrude))
        ));
        // X again backs out.
        assert!(matches!(e(ready), Some(Message::Look(Look::StartExtrude))));
        assert_eq!(Shortcut::EXTRUDE.label(), "X");
    }

    #[test]
    fn o_starts_a_revolve_outside_sketches_and_enter_commits_it() {
        let none = Modifiers::empty();
        let press = |keys, k: &str| pressed(document_bindings(keys), &key(k), none);
        let enter = |keys| {
            let enter = KeyPress::Named(Named::Enter);
            pressed(document_bindings(keys), &enter, none)
        };
        // With no sketch yet too.
        let revolvable = keys(true);
        assert!(matches!(
            press(revolvable, "o"),
            Some(Message::Look(Look::StartRevolve))
        ));
        let read_only = DocumentKeys {
            editable: false,
            ..revolvable
        };
        assert!(press(read_only, "o").is_none());
        // In a sketch, O takes up the Offset tool.
        let sketching = DocumentKeys {
            sketching: true,
            ..revolvable
        };
        assert!(matches!(
            press(sketching, "o"),
            Some(Message::Look(Look::SelectTool(Tool::Offset)))
        ));
        // While an extrude is set up, O drops it for a revolve, and X a
        // revolve for an extrude.
        let extruding = DocumentKeys {
            extruding: true,
            ..revolvable
        };
        assert!(matches!(
            press(extruding, "o"),
            Some(Message::Look(Look::StartRevolve))
        ));
        let revolving = DocumentKeys {
            revolving: true,
            ..revolvable
        };
        assert!(matches!(
            press(revolving, "x"),
            Some(Message::Look(Look::StartExtrude))
        ));
        // Setting one up, Enter is OK once it's ready, S drops it for a
        // new sketch, and O again backs out.
        assert!(enter(revolving).is_none());
        assert!(matches!(
            press(revolving, "s"),
            Some(Message::Look(Look::PickPlane))
        ));
        let ready = DocumentKeys {
            revolve_ready: true,
            ..revolving
        };
        assert!(matches!(
            enter(ready),
            Some(Message::Edit(Edit::CommitRevolve))
        ));
        assert!(matches!(
            press(ready, "o"),
            Some(Message::Look(Look::StartRevolve))
        ));
        assert_eq!(Shortcut::REVOLVE.label(), "O");
    }

    #[test]
    fn b_starts_a_combine_outside_sketches_and_enter_commits_it() {
        let none = Modifiers::empty();
        let press = |keys, k: &str| pressed(document_bindings(keys), &key(k), none);
        let enter = |keys| {
            let enter = KeyPress::Named(Named::Enter);
            pressed(document_bindings(keys), &enter, none)
        };
        // One body or none: nothing to combine.
        assert!(press(keys(true), "b").is_none());
        let combinable = DocumentKeys {
            combinable: true,
            ..keys(true)
        };
        assert!(matches!(
            press(combinable, "b"),
            Some(Message::Look(Look::StartCombine))
        ));
        let read_only = DocumentKeys {
            editable: false,
            ..combinable
        };
        assert!(press(read_only, "b").is_none());
        // In a sketch, B takes up the Rectangle tool.
        let sketching = DocumentKeys {
            sketching: true,
            ..combinable
        };
        assert!(matches!(
            press(sketching, "b"),
            Some(Message::Look(Look::SelectTool(Tool::Rectangle)))
        ));
        // Not while another operation is set up, nor the measure tool
        // while it is; Sketch, Extrude and Revolve drop it.
        let extruding = DocumentKeys {
            extruding: true,
            ..combinable
        };
        assert!(press(extruding, "b").is_none());
        let combining = DocumentKeys {
            combining: true,
            ..combinable
        };
        assert!(press(combining, "i").is_none());
        for other in ["x", "o", "s"] {
            assert!(press(combining, other).is_some(), "{other}");
        }
        assert!(enter(combining).is_none());
        let ready = DocumentKeys {
            combine_ready: true,
            ..combining
        };
        assert!(matches!(
            enter(ready),
            Some(Message::Edit(Edit::CommitCombine))
        ));
        assert!(matches!(
            press(ready, "b"),
            Some(Message::Look(Look::StartCombine))
        ));
        assert_eq!(Shortcut::COMBINE.label(), "B");
    }

    #[test]
    fn space_puts_down_a_sketch_tool() {
        let space = KeyPress::Named(Named::Space);
        let tool = DocumentKeys {
            sketching: true,
            tool: true,
            ..keys(true)
        };
        assert!(matches!(
            pressed(document_bindings(tool), &space, Modifiers::empty()),
            Some(Message::Look(Look::PutDownTool))
        ));
    }

    #[test]
    fn space_clears_the_selection_everywhere() {
        let space = KeyPress::Named(Named::Space);
        let sketching = DocumentKeys {
            sketching: true,
            ..keys(false)
        };
        for keys in [keys(true), keys(false), sketching] {
            assert!(matches!(
                pressed(document_bindings(keys), &space, Modifiers::empty()),
                Some(Message::Look(Look::ClearSelection))
            ));
        }
    }
}
