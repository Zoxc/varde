//! A shell in the move's session ([`MotionKind::Shell`]): its faces to
//! remove picked as a face session's (see `faces`), its thickness and
//! which way the walls grow (the model mock's Direction tiles: Inward,
//! Outward).

use varde_document::{Design, FeatureKind, Shell};
use varde_render::Camera;
use varde_view::{MotionField, MotionKind, ShellDirection, ShellView};

use super::refs::Refs;
use super::{BLEND_SHARE, Doc, MotionSession, length_field};
use crate::doc::camera::fitting_length;
use crate::doc::regions::TypedText;

/// The shell's thickness field as a new one opens it, seen by `camera`:
/// [`BLEND_SHARE`] of the view's height made nice ([`fitting_length`]),
/// in the design's units with their symbol.
pub(super) fn thickness_field(design: &Design, camera: &Camera) -> TypedText {
    let length = fitting_length(camera, design.units, BLEND_SHARE);
    length_field(length, &Shell::thickness_ask(design), design)
}

impl MotionSession {
    /// The shell as set up, if it's whole: its body, the faces to remove
    /// (none for a closed one), and its thickness as it last read.
    pub(super) fn shell(&self) -> Option<Shell> {
        let &[body] = self.bodies.as_slice() else {
            return None;
        };
        Some(Shell {
            body,
            open: self.faces.refs.clone(),
            thickness: self.field(MotionField::Thickness).value.clone()?,
            outward: self.direction.outward(),
        })
    }

    /// Opens the shell `shell` in this session: its body, faces,
    /// thickness and direction.
    pub(super) fn open_shell(&mut self, shell: &Shell) {
        let ask = Shell::thickness_ask(&self.design);
        self.fields[MotionField::Thickness.index()] = TypedText::of(&shell.thickness, &ask);
        self.direction = ShellDirection::of(shell.outward);
        self.faces = Refs::of(&shell.open);
        self.bodies = vec![shell.body];
        self.faces_body();
    }

    /// The panel's warning for a shell with no faces to remove, the UI
    /// mock's, once nothing else is still to do or gone.
    pub(super) fn shell_warning(&self) -> Option<String> {
        (self.kind == MotionKind::Shell
            && self.faces.refs.is_empty()
            && self.need().is_none()
            && self.gone().is_none())
        .then(|| "No faces removed: the body becomes closed and hollow".to_owned())
    }
}

impl Doc {
    /// What the panel shows of the shell being set up.
    pub(super) fn shell_view(&self, session: &MotionSession) -> ShellView {
        let units = self.editor.document().units();
        ShellView {
            faces: self.picked_faces(session),
            direction: session.direction,
            info: (session.shell())
                .filter(|_| session.kind == MotionKind::Shell)
                .map(|shell| varde_view::shell_info(&shell, units)),
        }
    }
}

/// The feature a shell session makes.
pub(super) fn shell_kind(session: &MotionSession) -> Option<FeatureKind> {
    session.shell().map(FeatureKind::Shell)
}
