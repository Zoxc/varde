//! Setting up a move, a mirror, a pattern, an align or a scale: its
//! session, started by `Look::StartMove` (`M`, the toolbar, the rail's
//! Transform set), `Look::StartMirror` (the toolbar, the rail),
//! `Look::StartPattern` (`P`, the toolbar, the rail),
//! `Look::StartCircularPattern` (the rail), `Look::StartAlign` (the rail;
//! an align's own parts are in `align`), `Look::StartScale` (the rail's
//! Modify set; a scale's own parts are in `scale`) or `Look::StartSplit`
//! (the rail's Modify set; a split's own parts are in `split`),
//! `Look::StartChamfer` (`C`, the toolbar, the rail's Modify set; a
//! chamfer's edges are picked as a blend's, in `blend`, its own parts in
//! `chamfer`), `Look::StartFillet` (`F`, the rail's Modify set; a
//! fillet's edges are picked as a chamfer's, its own parts in
//! `fillet`), `Look::StartShell` (the rail's Modify set;
//! a shell's faces are picked as a face session's, in `faces`, its own
//! parts in `shell`), or by editing one, picking its bodies as a
//! combine's (the body of what a click in the viewport is on, or a row
//! in Objects), a move's or pattern's axis or a mirror's plane (an origin
//! one from the toolbar, or a model edge or face clicked, named as of the
//! feature), a move's offsets and angle or a pattern's count and spread
//! typed, the preview through the regeneration lane's drafts, and
//! committing it as one undo step or cancelling it, which leaves no
//! trace.
//!
//! While the axis or plane is picked, the model shown is the history as
//! of the feature: a new one isn't previewed then, and an edited one is
//! previewed as a move that leaves its bodies where they are, so the
//! edges and faces clicked are where the feature finds them. A click is
//! taken only on the model answering what was asked last.

use std::borrow::Cow;
use std::sync::Arc;

use std::f64::consts::{PI, TAU};

use glam::DVec3;
use varde_document::{
    Axis3, AxisRef, BodyId, Copies, Design, Document, EdgeRef, FaceRef, FeatureId, FeatureKind,
    Generation, MAX_FEATURE_BODIES, MAX_PATTERN_COUNT, Mirror, Move, Pattern, PatternKind,
    PlaneRef, Scale,
};
use varde_expr::{AngleUnit, Ask, ErrorKind, Unit, Value};
use varde_kernel::Motion;
use varde_regen::Summary;
use varde_view::{
    AlignRole, AlignSide, AlignSlot, ChamferType, CombineBody, ModelHighlight, MotionField,
    MotionKind, MotionLook, MotionPick, MotionState, Naming, PanelHover, PatternMode, Pick, Picked,
    ShellDirection, SplitMode, Unnamed, axis_name, pattern_copies, plane_name,
};

use self::align::AlignSetup;
use self::blend::BlendSetup;
use self::refs::Refs;
use self::scale::ScaleSetup;
use self::split::SplitSetup;
use super::combine::pickable;
use super::feed::Merges;
use super::regions::TypedText;
use super::{Doc, Focus};

/// The move, mirror or pattern being set up, while one is:
/// [`Doc::motion`].
#[derive(Debug, Clone)]
pub(crate) struct MotionSession {
    pub(crate) kind: MotionKind,
    /// The feature edited, or `None` for a new one.
    pub(crate) feature: Option<FeatureId>,
    /// Sorted without repeats, as the document keeps them; at most
    /// [`MAX_FEATURE_BODIES`].
    pub(crate) bodies: Vec<BodyId>,
    /// What a click picks.
    pub(crate) picking: MotionPick,
    /// Its fields ([`MotionField::index`]): a move's offsets along X, Y
    /// and Z and its angle, a pattern's count and spread (its spacing or
    /// total, a length or a circular one's angle, as its mode reads it),
    /// an align's distance (and its angle, the move's), a scale's
    /// factors and length, a chamfer's distances and angle, a shell's
    /// thickness, a fillet's radius.
    pub(crate) fields: [TypedText; 17],
    /// A move's or pattern's axis: the Z axis to begin with (a linear
    /// pattern's X), as the UI mock's. A move stores it only with an
    /// angle other than zero.
    pub(crate) axis: Option<AxisRef>,
    /// A mirror's plane, once picked.
    pub(crate) plane: Option<PlaneRef>,
    /// The bodies picked that the document no longer holds, or that
    /// aren't made before the feature edited any more (an undo took them
    /// away), as of the last change to it: kept, said to be gone, until
    /// taken out or back.
    gone_bodies: Vec<BodyId>,
    /// The axis or plane, as picked, the document can't name any more
    /// (its body or the faces' maker taken away): kept, said to be gone,
    /// until another is picked or it's back.
    gone_reference: Option<Reference>,
    /// A mirror's Create copy: on to begin with, as the UI mock's.
    pub(crate) keep_original: bool,
    /// A linear pattern's Flip direction, stored as a negative spacing;
    /// an align's Flip.
    pub(crate) flip: bool,
    /// An align's references, as picked.
    pub(crate) align: AlignSetup,
    /// A scale's point, mode and edge.
    pub(crate) scale: ScaleSetup,
    /// A split's tools and what it keeps.
    pub(crate) split: SplitSetup,
    /// A chamfer's edges and Tangent chain.
    pub(crate) blend: BlendSetup,
    /// How a chamfer is sized: Equal to begin with, as the UI mock's.
    pub(crate) chamfer_type: ChamferType,
    /// A face session's faces: a shell's to remove.
    pub(crate) faces: Refs<FaceRef>,
    /// Which way a shell's walls grow: Inward to begin with, as the UI
    /// mock's.
    pub(crate) direction: ShellDirection,
    /// A pattern's Join to original: ticked to begin with, each body
    /// holding its copies, as the pattern stores by default (the UI
    /// mock's starts unticked; the user's decision is ticked); unticked,
    /// each copy is a body of its own ([`Copies::Separate`]).
    pub(crate) join: bool,
    /// An edited pattern's spread as it opened: see [`Opened`].
    opened: Option<Opened>,
    /// How a pattern's copies are spread: by the spacing to begin with
    /// (a circular one's round a full turn), as the UI mock's.
    pub(crate) mode: PatternMode,
    /// The panel's row the cursor is over, if any.
    pub(crate) hover: Option<PanelHover>,
    /// The bodies of the feature edited, which its preview leaves where
    /// they are while the axis or plane is picked with none picked.
    edited_bodies: Vec<BodyId>,
    /// The design as the fields' texts were last read, whose units bare
    /// lengths in them are in.
    design: Design,
    /// Built when what it's of changes, so the renderer uploads it only
    /// then.
    highlight: Arc<ModelHighlight>,
    /// What `highlight` was built of.
    built: Option<Built>,
    /// Where a move's handles stand, as a point of its bodies before the
    /// move, once the model shown has told: [`Pivot`].
    pivot: Option<Pivot>,
}

/// Where a move's handles stand: `at`, a point of `bodies` as they are
/// before the move, which the move takes where the handles show
/// ([`MotionSession::centre`]). Found once from the centre of their box
/// in the model shown, taken back through the move it shows, and kept
/// while the bodies and the document stay as they were: the box of
/// turned bodies has another centre, so the handles would jump as a
/// ring is let go of, and a second turn would be about another point.
#[derive(Debug, Clone, PartialEq)]
struct Pivot {
    bodies: Vec<BodyId>,
    generation: Generation,
    at: DVec3,
}

/// What a move's, mirror's or pattern's highlight is built of: the model, what's
/// hovered and whether its row in the panel is, what clicks pick, and
/// the bodies.
/// An align's lit picks, the moved side's and the target's
/// ([`Doc::align_lit`]), too; whether a split's tool body is picked,
/// which lights the body hovered whole; and whether a chamfer's edges
/// take in their tangent chains, which light with the edge hovered. A
/// shell's faces are lit as an align's moved side.
type Built = (
    u64,
    Option<(Picked, bool)>,
    MotionPick,
    Vec<BodyId>,
    [Vec<Picked>; 2],
    bool,
    bool,
);

/// An edited pattern's spacing or angle as stored, and the mode, Flip
/// and spread field the session opened it with: while those are as they
/// opened, the pattern keeps the stored value, text and all, so OK with
/// nothing changed writes nothing even where the session would write the
/// same value another way ("-15" read with Flip on would come back
/// "-(15)", "360" for Full 360° as "360°").
#[derive(Debug, Clone, PartialEq)]
struct Opened {
    mode: PatternMode,
    flip: bool,
    spread: TypedText,
    stored: Value,
}

/// Angles are shown in degrees.
const DEGREES: Unit = Unit::Angle(AngleUnit::Deg);

/// A value read for `ask` from `text`: one the asks take, as the fields
/// start with.
fn read(text: &str, ask: &Ask) -> TypedText {
    TypedText::read(text.to_owned(), ask)
}

/// What a pattern of `kind`'s spread is read with in `design`: a circular
/// one's an angle as the pattern stores it, a linear one's a length above
/// zero (the Flip direction gives it its sign) within the coordinate
/// limit.
fn spread_ask(kind: MotionKind, design: &Design) -> Ask {
    match kind {
        MotionKind::CircularPattern => Pattern::angle_ask(design),
        _ => Pattern::spacing_ask(design).positive(),
    }
}

/// What `field` of a session of `kind` is read with in `design`.
fn field_ask(kind: MotionKind, field: MotionField, design: &Design) -> Ask {
    match field {
        MotionField::Offset(_) | MotionField::Distance => Move::offset_ask(design),
        MotionField::Angle => Move::angle_ask(design),
        MotionField::Count => Pattern::count_ask(design),
        MotionField::Spread => spread_ask(kind, design),
        MotionField::Factor | MotionField::AxisFactor(_) => Scale::factor_ask(design),
        MotionField::Length => Scale::length_ask(design),
        MotionField::ChamferDistance | MotionField::ChamferSecond | MotionField::ChamferAngle => {
            chamfer::chamfer_ask(field, design).expect("a chamfer's field")
        }
        MotionField::Thickness => varde_document::Shell::thickness_ask(design),
        MotionField::Radius => varde_document::Fillet::radius_ask(design),
    }
}

/// How a pattern was last set up in a session, kept by the app for the
/// feature committed ([`Doc::pattern_shapes`]): the mode and Flip the
/// user chose and the spread as typed, which the stored values can't
/// tell apart (a spacing typed as a total, a circular angle typed as a
/// spacing). Taken again on editing it only while it gives the values
/// stored, see [`MotionSession::editing`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PatternShape {
    mode: PatternMode,
    flip: bool,
    spread: Value,
}

/// The value of `text`, an expression the session makes of a typed one
/// (a total divided, a spacing turned), read for `ask`: where it would be
/// past what an expression may hold (its length or nesting), `value`,
/// what it works out to, written exactly ([`varde_expr::exact`]), so a
/// long text typed isn't refused for what's put round it.
fn composed(text: &str, value: f64, ask: &Ask) -> Result<Value, varde_expr::Error> {
    Value::new(text, ask).or_else(|error| match error.kind {
        ErrorKind::TooLong | ErrorKind::TooDeep => {
            Value::new(&varde_expr::exact(value, ask.quantity), ask)
        }
        _ => Err(error),
    })
}

/// `value` with its sign turned, as text and value, read for `ask`: the
/// text without a leading "-" or "-(...)" where that gives the value
/// turned exactly, else the text turned, "-(...)" (the value turned
/// written exactly where that's too long, [`composed`]).
fn negated(value: &Value, ask: &Ask) -> Option<Value> {
    let text = value.text.trim();
    let inner = (text
        .strip_prefix("-(")
        .and_then(|rest| rest.strip_suffix(')')))
    .into_iter()
    .chain(text.strip_prefix('-'));
    for inner in inner {
        if let Ok(turned) = Value::new(inner, ask)
            && turned.value == -value.value
        {
            return Some(turned);
        }
    }
    composed(&format!("-({text})"), -value.value, ask)
        .ok()
        .filter(|turned| turned.value == -value.value)
}

impl MotionSession {
    /// A session setting up a new `kind` of `document`, of `bodies`: a
    /// move by nothing yet about the Z axis, a mirror keeping the
    /// original with its plane to pick, or a pattern as the UI mock's
    /// begins: a linear one 3 copies 100 (of the design's units) apart
    /// along the X axis, a circular one 4 round a full turn about the Z
    /// axis. Clicks pick bodies, or a mirror's plane once it has bodies.
    fn new(kind: MotionKind, document: &Document, mut bodies: Vec<BodyId>) -> Self {
        bodies.sort_unstable();
        bodies.dedup();
        bodies.truncate(MAX_FEATURE_BODIES);
        let design = document.design();
        let offset = Move::offset_ask(&design);
        let angle = Move::angle_ask(&design);
        let count = Pattern::count_ask(&design);
        let spread = spread_ask(kind, &design);
        let length = |design: &Design| varde_expr::format(0.0, Some(Unit::Length(design.units)));
        let zero = length(&design);
        let factor = Scale::factor_ask(&design);
        let picking = match kind {
            MotionKind::Mirror if !bodies.is_empty() => MotionPick::Reference,
            // An align moves one body, then picks its point on it.
            MotionKind::Align if bodies.len() == 1 => {
                MotionPick::Align(AlignSlot::new(AlignSide::Moved, AlignRole::Point))
            }
            // A split splits one body, then picks its tool.
            MotionKind::Split if !bodies.is_empty() => MotionPick::Tool,
            // A chamfer or a fillet picks edges, its body theirs.
            kind if kind.blends() => MotionPick::Edges,
            // A face session picks faces, its body theirs (a shell's, or
            // the one it starts with).
            kind if kind.picks_faces() => MotionPick::Faces,
            _ => MotionPick::Bodies,
        };
        if matches!(kind, MotionKind::Align | MotionKind::Split) {
            bodies.truncate(1);
        }
        if kind.picks_faces() {
            bodies.truncate(usize::from(faces::takes_body(kind)));
        }
        if kind.blends() {
            bodies.clear();
        }
        let [chamfer_distance, chamfer_second, chamfer_angle] = chamfer::chamfer_fields(&design);
        let (copies, spread_field, axis, mode) = match kind {
            MotionKind::CircularPattern => (
                read("4", &count),
                read(&varde_expr::format(PI / 2.0, Some(DEGREES)), &spread),
                Some(AxisRef::Origin(Axis3::Z)),
                PatternMode::Full,
            ),
            _ => {
                // A hundred of the design's units, with their symbol.
                let hundred =
                    Value::new("100", &spread).map(|value| TypedText::of(&value, &spread));
                (
                    read("3", &count),
                    hundred.unwrap_or_else(|_| read("100", &spread)),
                    match kind {
                        MotionKind::Move => Some(AxisRef::Origin(Axis3::Z)),
                        MotionKind::LinearPattern => Some(AxisRef::Origin(Axis3::X)),
                        _ => None,
                    },
                    PatternMode::Spacing,
                )
            }
        };
        Self {
            kind,
            feature: None,
            bodies,
            picking,
            fields: [
                read(&zero, &offset),
                read(&zero, &offset),
                read(&zero, &offset),
                read(&varde_expr::format(0.0, Some(DEGREES)), &angle),
                copies,
                spread_field,
                read(&zero, &offset),
                read("1", &factor),
                read("1", &factor),
                read("1", &factor),
                read("1", &factor),
                scale::empty_length(),
                chamfer_distance,
                chamfer_second,
                chamfer_angle,
                shell::thickness_field(&design),
                fillet::radius_field(&design),
            ],
            axis,
            plane: None,
            gone_bodies: Vec::new(),
            gone_reference: None,
            keep_original: true,
            flip: false,
            align: AlignSetup::default(),
            scale: ScaleSetup::default(),
            split: SplitSetup::default(),
            blend: BlendSetup::default(),
            chamfer_type: ChamferType::Equal,
            faces: Refs::default(),
            direction: ShellDirection::Inward,
            join: true,
            opened: None,
            mode,
            hover: None,
            edited_bodies: Vec::new(),
            design,
            highlight: Arc::default(),
            built: None,
            pivot: None,
        }
    }

    /// A session editing the move, mirror or pattern `feature` of
    /// `document`, with its values; `None` if it's none of those. A
    /// pattern takes `shape`, how it was last set up, if that gives the
    /// values it stores; else a linear one's spacing is read as typed
    /// (Flip on for a negative one, its text turned), and a circular one
    /// as Full 360° for a whole turn, else as the Total it stores.
    fn editing(
        document: &Document,
        feature: FeatureId,
        shape: Option<&PatternShape>,
    ) -> Option<Self> {
        let design = document.design();
        let mut session = match &document.feature(feature)?.kind {
            FeatureKind::Move(moved) => {
                let mut session = Self::new(MotionKind::Move, document, moved.bodies.clone());
                let offset = Move::offset_ask(&design);
                for (field, value) in session.fields.iter_mut().zip(&moved.offset) {
                    *field = TypedText::of(value, &offset);
                }
                if let Some((axis, angle)) = &moved.turn {
                    session.axis = Some(*axis);
                    session.fields[MotionField::Angle.index()] =
                        TypedText::of(angle, &Move::angle_ask(&design));
                }
                session
            }
            FeatureKind::Mirror(mirror) => {
                let mut session = Self::new(MotionKind::Mirror, document, mirror.bodies.clone());
                session.plane = Some(mirror.plane);
                session.keep_original = mirror.keep_original;
                session.picking = MotionPick::Bodies;
                session
            }
            kind @ FeatureKind::Pattern(pattern) => {
                let kind = MotionKind::of(kind)?;
                let mut session = Self::new(kind, document, pattern.bodies.clone());
                session.axis = Some(*pattern.kind.axis());
                session.join = pattern.joins();
                session.fields[MotionField::Count.index()] =
                    TypedText::of(pattern.kind.count_value(), &Pattern::count_ask(&design));
                session.take_shape(pattern, shape);
                session.opened = Some(Opened {
                    mode: session.mode,
                    flip: session.flip,
                    spread: session.field(MotionField::Spread).clone(),
                    stored: spread_of(&pattern.kind).clone(),
                });
                session
            }
            FeatureKind::Align(align) => {
                let mut session = Self::new(MotionKind::Align, document, vec![align.body]);
                session.align = AlignSetup::of(align);
                session.flip = align.flip;
                let ask = Move::offset_ask(&design);
                if let Some(offset) = &align.offset {
                    session.fields[MotionField::Distance.index()] = TypedText::of(offset, &ask);
                }
                if let Some(turn) = &align.turn {
                    session.fields[MotionField::Angle.index()] =
                        TypedText::of(turn, &Move::angle_ask(&design));
                }
                session.picking = MotionPick::Nothing;
                session
            }
            FeatureKind::Scale(scale) => {
                let mut session = Self::new(MotionKind::Scale, document, scale.bodies.clone());
                session.scale = ScaleSetup::of(scale);
                let factor = Scale::factor_ask(&design);
                let mut set = |field: MotionField, value: &Value, ask: &Ask| {
                    session.fields[field.index()] = TypedText::of(value, ask);
                };
                match &scale.factor {
                    varde_document::ScaleFactor::Uniform(value) => {
                        set(MotionField::Factor, value, &factor);
                    }
                    varde_document::ScaleFactor::PerAxis(values) => {
                        for (axis, value) in Axis3::ALL.into_iter().zip(values) {
                            set(MotionField::AxisFactor(axis), value, &factor);
                        }
                    }
                    varde_document::ScaleFactor::EdgeLength { length, .. } => {
                        set(MotionField::Length, length, &Scale::length_ask(&design));
                    }
                }
                session
            }
            FeatureKind::Split(split) => {
                let mut session = Self::new(MotionKind::Split, document, vec![split.body]);
                session.split = SplitSetup::of(document, split);
                session.picking = MotionPick::Nothing;
                session
            }
            FeatureKind::Chamfer(chamfer) => {
                let mut session = Self::new(MotionKind::Chamfer, document, Vec::new());
                session.open_chamfer(chamfer);
                session
            }
            FeatureKind::Shell(shell) => {
                let mut session = Self::new(MotionKind::Shell, document, vec![shell.body]);
                session.open_shell(shell);
                session
            }
            FeatureKind::Fillet(fillet) => {
                let mut session = Self::new(MotionKind::Fillet, document, Vec::new());
                session.open_fillet(fillet);
                session
            }
            _ => return None,
        };
        session.feature = Some(feature);
        session.edited_bodies = session.bodies.clone();
        Some(session)
    }

    /// Sets a pattern's mode, Flip and spread to `shape`'s if they give
    /// what `pattern` stores, else as [`MotionSession::editing`] reads
    /// them from it.
    fn take_shape(&mut self, pattern: &Pattern, shape: Option<&PatternShape>) {
        let ask = spread_ask(self.kind, &self.design);
        // A shape of the other kind's (a file swapped the kind since) is
        // never taken: its mode may be none this kind offers.
        if let Some(shape) = shape.filter(|shape| PatternMode::of(self.kind).contains(&shape.mode))
        {
            let mut taken = Self {
                mode: shape.mode,
                flip: shape.flip,
                ..self.clone()
            };
            taken.fields[MotionField::Spread.index()] = TypedText::of(&shape.spread, &ask);
            if matches!(taken.pattern(), Ok(Some(ref made)) if made.kind == pattern.kind) {
                *self = taken;
                return;
            }
        }
        let spread = &mut self.fields[MotionField::Spread.index()];
        match &pattern.kind {
            PatternKind::Linear { spacing, .. } => {
                self.mode = PatternMode::Spacing;
                self.flip = spacing.value < 0.0;
                let typed = if self.flip {
                    negated(spacing, &ask)
                } else {
                    Some(spacing.clone())
                };
                if let Some(typed) = typed {
                    *spread = TypedText::of(&typed, &ask);
                }
            }
            PatternKind::Circular { .. } if pattern.full_turn() => self.mode = PatternMode::Full,
            PatternKind::Circular { angle, .. } => {
                self.mode = PatternMode::Total;
                *spread = TypedText::of(angle, &ask);
            }
        }
    }

    /// The field `field`'s text as last read.
    fn field(&self, field: MotionField) -> &TypedText {
        &self.fields[field.index()]
    }

    /// The angle as it last read, in radians: zero until one is typed.
    fn angle(&self) -> Option<f64> {
        let value = self.field(MotionField::Angle).value.as_ref()?;
        Some(value.value)
    }

    /// The feature as set up, if it's whole: bodies, and a move's offsets
    /// and angle as they last read with an axis if the angle isn't zero
    /// (none stored if it is), a mirror's plane, or a pattern as
    /// [`MotionSession::pattern`] makes it.
    fn kind(&self) -> Option<FeatureKind> {
        if self.bodies.is_empty() {
            return None;
        }
        match self.kind {
            MotionKind::LinearPattern | MotionKind::CircularPattern => {
                self.pattern().ok().flatten().map(FeatureKind::Pattern)
            }
            MotionKind::Move => {
                let offset = |axis: Axis3| (self.field(MotionField::Offset(axis)).value).clone();
                let angle = self.field(MotionField::Angle).value.clone()?;
                let turn = if angle.value == 0.0 {
                    None
                } else {
                    Some((self.axis?, angle))
                };
                Some(FeatureKind::Move(Move {
                    bodies: self.bodies.clone(),
                    offset: [offset(Axis3::X)?, offset(Axis3::Y)?, offset(Axis3::Z)?],
                    turn,
                }))
            }
            MotionKind::Mirror => Some(FeatureKind::Mirror(Mirror {
                bodies: self.bodies.clone(),
                plane: self.plane?,
                keep_original: self.keep_original,
            })),
            MotionKind::Align => self.align().map(FeatureKind::from),
            MotionKind::Scale => self.scale().map(FeatureKind::Scale),
            MotionKind::Split => self.split().map(FeatureKind::Split),
            MotionKind::Chamfer => chamfer::chamfer_kind(self),
            MotionKind::Shell => shell::shell_kind(self),
            MotionKind::Fillet => fillet::fillet_kind(self),
        }
    }

    /// The pattern as set up, its count and spread as they last read:
    /// `Ok(None)` while it isn't whole (no bodies, no axis, no value),
    /// and why its spread is refused as a whole, in the UI mock's words,
    /// if it is (its own text read, the spacing, total or angle it comes
    /// to is beyond what's stored). Stored, a linear pattern's spacing is
    /// as typed, a Total's divided by the count less one, either turned
    /// by Flip direction ("-(...)"); a circular one's angle is 360° for
    /// Full 360°, the Total as typed, or a Spacing's times the count less
    /// one, refused where either comes to a full turn or more.
    fn pattern(&self) -> Result<Option<Pattern>, String> {
        let (Some(axis), false) = (self.axis, self.bodies.is_empty()) else {
            return Ok(None);
        };
        let Some(count) = self.field(MotionField::Count).value.clone() else {
            return Ok(None);
        };
        // Within the count's ask, so the conversion is exact.
        let range = 2.0..=f64::from(MAX_PATTERN_COUNT);
        if !(range.contains(&count.value) && count.value.fract() == 0.0) {
            return Ok(None);
        }
        let steps = count.value as u32 - 1;
        let spread = self.field(MotionField::Spread).value.as_ref();
        let design = &self.design;
        let circular = |angle: &Value| Pattern {
            bodies: Vec::new(),
            kind: PatternKind::Circular {
                about: axis,
                count: count.clone(),
                angle: angle.clone(),
            },
            copies: Copies::Joined,
        };
        let kind = match self.kind {
            MotionKind::LinearPattern => {
                let Some(spread) = spread else {
                    return Ok(None);
                };
                let limit = f64::from(varde_kernel::MAX_COORD);
                if self.mode != PatternMode::Total && spread.value * f64::from(steps) > limit {
                    let limit = varde_expr::format(limit, Some(Unit::Length(design.units)));
                    return Err(format!("The pattern runs past {limit}"));
                }
                let (text, value) = match self.mode {
                    PatternMode::Total if steps > 1 => (
                        format!("({}) / {steps}", spread.text),
                        spread.value / f64::from(steps),
                    ),
                    _ => (spread.text.clone(), spread.value),
                };
                let (text, value) = if self.flip {
                    (format!("-({text})"), -value)
                } else {
                    (text, value)
                };
                let spacing = if text == spread.text {
                    spread.clone()
                } else {
                    composed(&text, value, &Pattern::spacing_ask(design))
                        .map_err(|error| error.to_string())?
                };
                PatternKind::Linear {
                    along: axis,
                    count: count.clone(),
                    spacing,
                }
            }
            MotionKind::CircularPattern => {
                let ask = Pattern::angle_ask(design);
                let angle = match (self.mode, spread) {
                    (PatternMode::Full, _) => {
                        let full = varde_expr::format(TAU, Some(DEGREES));
                        composed(&full, TAU, &ask).map_err(|error| error.to_string())?
                    }
                    (_, None) => return Ok(None),
                    (PatternMode::Total, Some(spread)) => {
                        if circular(spread).full_turn() {
                            return Err(
                                "A whole turn puts the last copy on the first: use Full 360°"
                                    .to_owned(),
                            );
                        }
                        spread.clone()
                    }
                    (PatternMode::Spacing, Some(spread)) => {
                        let past = || {
                            let step = varde_expr::format(spread.value, Some(DEGREES));
                            format!("{} copies {step} apart go past a full turn", steps + 1)
                        };
                        if spread.value * f64::from(steps) > TAU {
                            return Err(past());
                        }
                        // Within what an expression holds, only a span
                        // past a turn is refused here.
                        let angle = if steps == 1 {
                            spread.clone()
                        } else {
                            let text = format!("({}) * {steps}", spread.text);
                            composed(&text, spread.value * f64::from(steps), &ask)
                                .map_err(|_| past())?
                        };
                        if circular(&angle).full_turn() {
                            return Err(past());
                        }
                        angle
                    }
                };
                circular(&angle).kind
            }
            MotionKind::Move
            | MotionKind::Mirror
            | MotionKind::Align
            | MotionKind::Scale
            | MotionKind::Split
            | MotionKind::Chamfer
            | MotionKind::Shell
            | MotionKind::Fillet => {
                return Ok(None);
            }
        };
        let mut kind = kind;
        if let Some(opened) = &self.opened
            && (opened.mode, opened.flip) == (self.mode, self.flip)
            && opened.spread == *self.field(MotionField::Spread)
        {
            let spread = spread_of_mut(&mut kind);
            if spread.value == opened.stored.value {
                *spread = opened.stored.clone();
            }
        }
        // Unjoined, the document lays the copy bodies out, keeping an
        // edited pattern's.
        let copies = if self.join {
            Copies::Joined
        } else {
            Copies::Separate(Vec::new())
        };
        Ok(Some(Pattern {
            bodies: self.bodies.clone(),
            kind,
            copies,
        }))
    }

    /// Why a pattern's spread is refused as a whole, if it is: see
    /// [`MotionSession::pattern`].
    fn spread_error(&self) -> Option<String> {
        self.pattern().err()
    }

    /// What's still to be done before it can be committed, the UI mock's
    /// words, if anything.
    fn need(&self) -> Option<&'static str> {
        if self.kind == MotionKind::Align {
            return self.align_need();
        }
        if self.kind == MotionKind::Scale {
            return self.scale_need();
        }
        if self.kind == MotionKind::Split {
            return self.split_need();
        }
        if self.kind.blends() {
            return self.blend_need();
        }
        if self.kind.picks_faces() {
            return self.faces_need();
        }
        if self.bodies.is_empty() {
            return Some(match self.kind {
                MotionKind::Move => "pick the bodies to move",
                MotionKind::Mirror => "pick the bodies to mirror",
                MotionKind::LinearPattern | MotionKind::CircularPattern => {
                    "pick the bodies to pattern"
                }
                MotionKind::Align => "pick the body to align",
                MotionKind::Scale => "pick the bodies to scale",
                MotionKind::Split => "pick the body to split",
                MotionKind::Chamfer => "pick the edges to chamfer",
                MotionKind::Shell => "pick faces to remove, or the body to hollow",
                MotionKind::Fillet => "pick the edges to fillet",
            });
        }
        match self.kind {
            MotionKind::LinearPattern if self.axis.is_none() => {
                Some("pick a direction: an origin axis or a straight edge")
            }
            MotionKind::CircularPattern if self.axis.is_none() => {
                Some("pick an axis: an origin axis or a straight edge")
            }
            MotionKind::LinearPattern | MotionKind::CircularPattern => None,
            MotionKind::Mirror if self.plane.is_none() => {
                Some("pick a plane: an origin plane or a planar face")
            }
            MotionKind::Mirror
            | MotionKind::Align
            | MotionKind::Scale
            | MotionKind::Split
            | MotionKind::Chamfer
            | MotionKind::Shell
            | MotionKind::Fillet => None,
            MotionKind::Move => {
                let angle = self.angle().unwrap_or(0.0);
                if angle != 0.0 && self.axis.is_none() {
                    return Some("pick an axis to rotate about");
                }
                let offsets = [Axis3::X, Axis3::Y, Axis3::Z].map(|axis| {
                    (self.field(MotionField::Offset(axis)).value.as_ref())
                        .map_or(0.0, |value| value.value)
                });
                (angle == 0.0 && offsets.iter().all(|&offset| offset == 0.0))
                    .then_some("enter a distance or an angle")
            }
        }
    }

    /// Where its handles stand, `pivot` taken by the move as set up, if
    /// it's a whole move whose turn's line is known (`line`, regenerating's
    /// for an edge or face axis).
    fn centre(&self, pivot: DVec3, line: Option<[DVec3; 2]>) -> Option<DVec3> {
        let Some(FeatureKind::Move(moved)) = self.kind() else {
            return None;
        };
        let at = motion_of(&moved, line)?.point(pivot);
        at.is_finite().then_some(at)
    }

    /// What's gone that it names, the UI mock's words, if anything: a
    /// body picked, or the axis a move turns about, a pattern's axis or a
    /// mirror's plane, which another is to be picked for. A move's axis
    /// is only gone while it turns.
    fn gone(&self) -> Option<&'static str> {
        if self.bodies.is_empty() {
            return None;
        }
        // A chamfer's body is its edges': with it gone, so are they.
        if self.kind.blends() {
            let body = (self.bodies.iter()).any(|body| self.gone_bodies.contains(body));
            return if body {
                Some("A picked edge is gone")
            } else {
                self.blend_gone()
            };
        }
        // A face session's faces go with their body; with none, it's a
        // shell's body picked that's gone.
        if self.kind.picks_faces() {
            return self.faces_gone();
        }
        if (self.bodies.iter()).any(|body| self.gone_bodies.contains(body)) {
            return Some("A picked body is gone");
        }
        if self.kind == MotionKind::Align {
            return self.align_gone();
        }
        if self.kind == MotionKind::Scale {
            return self.scale_gone();
        }
        if self.kind == MotionKind::Split {
            return self.split_gone();
        }
        match (self.kind, self.gone_reference) {
            (MotionKind::Move, Some(Reference::Axis(axis)))
                if self.axis == Some(axis) && self.angle().is_some_and(|angle| angle != 0.0) =>
            {
                Some("The axis is gone: pick another")
            }
            (MotionKind::Mirror, Some(Reference::Plane(plane))) if self.plane == Some(plane) => {
                Some("The plane is gone: pick another")
            }
            (MotionKind::LinearPattern, Some(Reference::Axis(axis))) if self.axis == Some(axis) => {
                Some("The direction is gone: pick another")
            }
            (MotionKind::CircularPattern, Some(Reference::Axis(axis)))
                if self.axis == Some(axis) =>
            {
                Some("The axis is gone: pick another")
            }
            _ => None,
        }
    }

    /// Why it can't be committed to a document of `design` as set up, if
    /// its own check refuses it. None while it isn't whole.
    fn refused(&self, design: &Design) -> Option<String> {
        let refused = match self.kind()? {
            FeatureKind::Move(moved) => moved.check_own(design).err().map(|why| why.to_string()),
            FeatureKind::Mirror(mirror) => mirror.check_own().err().map(|why| why.to_string()),
            FeatureKind::Pattern(pattern) => {
                pattern.check_own(design).err().map(|why| why.to_string())
            }
            FeatureKind::Align(align) => align.check_own(design).err().map(|why| why.to_string()),
            FeatureKind::Scale(scale) => scale.check_own(design).err().map(|why| why.to_string()),
            FeatureKind::Split(split) => split.check_own().err().map(|why| why.to_string()),
            FeatureKind::Chamfer(chamfer) => {
                chamfer.check_own(design).err().map(|why| why.to_string())
            }
            FeatureKind::Shell(shell) => shell.check_own(design).err().map(|why| why.to_string()),
            FeatureKind::Fillet(fillet) => {
                fillet.check_own(design).err().map(|why| why.to_string())
            }
            _ => None,
        };
        refused.map(|why| format!("it {why}"))
    }

    /// Whether it can be committed to a document of `design`: whole,
    /// with nothing still to do, no field refused, and passing its own
    /// check. What's left to the document, the bodies and the reference,
    /// the session keeps valid.
    fn ready(&self, design: &Design) -> bool {
        let fine = |field: MotionField| self.field(field).error.is_none();
        let typed = match self.kind {
            MotionKind::Move => MotionField::ALL[..4].iter().all(|&field| fine(field)),
            MotionKind::Mirror | MotionKind::Split => true,
            MotionKind::Align => fine(MotionField::Distance) && fine(MotionField::Angle),
            MotionKind::Scale => self.scale_fields().iter().all(|&field| fine(field)),
            MotionKind::Chamfer => self.chamfer_type.fields().iter().all(|&field| fine(field)),
            MotionKind::Shell => fine(MotionField::Thickness),
            MotionKind::Fillet => fine(MotionField::Radius),
            MotionKind::LinearPattern | MotionKind::CircularPattern => {
                fine(MotionField::Count)
                    && (self.mode == PatternMode::Full || fine(MotionField::Spread))
                    && self.spread_error().is_none()
            }
        };
        typed
            && self.need().is_none()
            && self.gone().is_none()
            && self.kind().is_some()
            && self.refused(design).is_none()
    }

    /// Picks `body`, or takes it out if it's picked. None are added past
    /// the limit.
    fn toggle(&mut self, body: BodyId) {
        match self.bodies.binary_search(&body) {
            Ok(at) => {
                self.bodies.remove(at);
            }
            Err(at) if self.bodies.len() < MAX_FEATURE_BODIES => self.bodies.insert(at, body),
            Err(_) => {}
        }
    }

    /// Notes the bodies `document` no longer holds or that aren't made
    /// before the feature edited any more, and an axis or plane on a body
    /// or of a face's maker the document no longer takes there, or on a
    /// body it no longer holds (an undo took them away): kept, so what
    /// was picked comes back with a redo, but said to be gone
    /// ([`MotionSession::gone`]), and neither previewed nor committed
    /// while it is, as the UI mock has it. The bodies of the feature
    /// edited, previewed while the axis or plane is picked, are let go of.
    fn prune(&mut self, document: &Document) {
        let edited = self.feature;
        self.gone_bodies = (self.bodies.iter().copied())
            .filter(|&body| !pickable(document, body, edited))
            .collect();
        self.edited_bodies
            .retain(|&body| pickable(document, body, edited));
        let features = document.features();
        let Some(index) = (match edited {
            Some(id) => features.iter().position(|feature| feature.id == id),
            None => Some(features.len()),
        }) else {
            return;
        };
        self.prune_align(document, index);
        self.prune_scale(document, index);
        self.prune_split(document, index);
        if self.kind.blends() {
            self.prune_blend(document, index);
        }
        if self.kind.picks_faces() {
            self.prune_faces(document, index);
        }
        let held = |body: BodyId| document.body(body).is_some();
        self.gone_reference = match (self.axis, self.plane) {
            (Some(axis), _)
                if document.check_axis_ref(index, &axis).is_err()
                    || axis_body(&axis).is_some_and(|body| !held(body)) =>
            {
                Some(Reference::Axis(axis))
            }
            (_, Some(plane))
                if document.check_plane_ref(index, &plane).is_err()
                    || plane_body(&plane).is_some_and(|body| !held(body)) =>
            {
                Some(Reference::Plane(plane))
            }
            _ => None,
        };
    }

    /// Moves the bodies `merges` (the merges before the feature) have
    /// merged into others on to the bodies holding them, as a click on
    /// one picks, as a combine's ([`super::CombineSession`]), and a
    /// scale's edge with them (it must be on one it scales; its names
    /// find it on the holder) and a split's tool body: whether any moved.
    fn follow(&mut self, merges: &Merges) -> bool {
        // A chamfer's body is its edges'.
        if self.kind.blends() {
            return self.follow_refs::<EdgeRef>(merges);
        }
        // A face session's body is its faces', or a shell's the one
        // picked.
        if self.kind.picks_faces() {
            return self.follow_faces(merges);
        }
        let held = |body: BodyId| merges.holder(body).unwrap_or(body);
        let mut bodies: Vec<BodyId> = self.bodies.iter().map(|&body| held(body)).collect();
        bodies.sort_unstable();
        bodies.dedup();
        let mut moved = bodies != self.bodies;
        self.bodies = bodies;
        if let Some(edge) = &mut self.scale.edge {
            let body = held(edge.body);
            moved |= body != edge.body;
            edge.body = body;
        }
        // A split's tool body too.
        let tool = self.kind == MotionKind::Split && self.follow_split(merges);
        // An align's target merged into the body aligned is gone.
        if self.kind == MotionKind::Align {
            self.follow_align_merges(merges);
        }
        moved || tool
    }

    /// Keeps each value where the design's units changed since its field
    /// was read, as the document does its own.
    fn follow_units(&mut self, document: &Document) {
        let design = document.design();
        if design == self.design {
            return;
        }
        let asks = MotionField::ALL.map(|field| field_ask(self.kind, field, &self.design));
        for (field, ask) in self.fields.iter_mut().zip(&asks) {
            field.follow_units(ask);
        }
        self.design = design;
    }

    /// The draft previewing it while the axis or plane isn't picked: the
    /// feature as set up, if it's whole and nothing it names is gone (a
    /// new one only once it does something: [`MotionSession::need`]).
    /// While it is, an edited feature is previewed as a move leaving its
    /// bodies where they are (those still there; turning them by nothing
    /// about a move's or pattern's axis, unless it's gone, so where it is
    /// shows), and a new one not at all: the model shown is the history
    /// as of the feature, which the edges and faces clicked are named as.
    fn draft(&self, design: &Design) -> Option<(Option<FeatureId>, FeatureKind)> {
        // A chamfer is previewed as set up while it's whole and nothing it
        // names is gone, its edges picked meanwhile (on the preview they
        // go, as they're cut off: a row's cross takes one out). Not whole,
        // an edited one shows its body as of the feature, by a move of
        // nothing, so its edges are there to pick.
        // A face session too, its faces picked meanwhile (on a shell's
        // preview what's left of a face removed is lit as it, a click
        // there taking it out); with no body, an edited one shows its
        // body as of the feature.
        if self.kind.blends() || self.kind.picks_faces() {
            if self.gone().is_none()
                && let Some(kind) = self.kind()
            {
                return Some((self.feature, kind));
            }
            return self.unmoved(design);
        }
        // A split isn't previewed while a face or body is picked as its
        // tool: the model shown is the document's, a new one's the history
        // as of the feature, and an edited one's with its split as stored,
        // whose pieces [`Naming`] names as the body split. Regions and
        // curves are picked on its sketches, previewed meanwhile.
        if self.kind == MotionKind::Split {
            let model = self.picking == MotionPick::Tool
                && matches!(self.split.mode, SplitMode::Face | SplitMode::Body);
            if model || self.gone().is_some() || (self.feature.is_none() && self.need().is_some()) {
                return None;
            }
            return Some((self.feature, self.kind()?));
        }
        if matches!(self.picking, MotionPick::Bodies | MotionPick::Nothing) {
            // Nothing is previewed while something it names is gone, and
            // a new one that does nothing yet shows as the document does.
            if self.gone().is_some() || (self.feature.is_none() && self.need().is_some()) {
                return None;
            }
            return Some((self.feature, self.kind()?));
        }
        self.unmoved(design)
    }

    /// The draft showing an edited feature's place while it isn't
    /// previewed itself ([`MotionSession::draft`]): a move of its bodies
    /// by nothing. `None` for a new one.
    fn unmoved(&self, design: &Design) -> Option<(Option<FeatureId>, FeatureKind)> {
        let feature = self.feature?;
        // What's gone is left out: the bodies still there, and the axis
        // if it is, so the model shown is the feature's place even while
        // another axis is picked for one an undo took away.
        let bodies: Vec<BodyId> = (self.bodies.iter())
            .filter(|body| !self.gone_bodies.contains(body))
            .copied()
            .collect();
        let bodies = if bodies.is_empty() {
            self.edited_bodies.clone()
        } else {
            bodies
        };
        if bodies.is_empty() {
            return None;
        }
        let zero = || Value::new("0", &Move::offset_ask(design)).ok();
        let gone = |axis: AxisRef| self.gone_reference == Some(Reference::Axis(axis));
        let turn = match self.axis {
            Some(axis) if self.kind.takes_axis() && !gone(axis) => {
                Some((axis, Value::new("0", &Move::angle_ask(design)).ok()?))
            }
            _ => None,
        };
        Some((
            Some(feature),
            FeatureKind::Move(Move {
                bodies,
                offset: [zero()?, zero()?, zero()?],
                turn,
            }),
        ))
    }
}

/// A linear pattern's spacing, or a circular one's angle.
fn spread_of(kind: &PatternKind) -> &Value {
    match kind {
        PatternKind::Linear { spacing, .. } => spacing,
        PatternKind::Circular { angle, .. } => angle,
    }
}

/// The same, to change.
fn spread_of_mut(kind: &mut PatternKind) -> &mut Value {
    match kind {
        PatternKind::Linear { spacing, .. } => spacing,
        PatternKind::Circular { angle, .. } => angle,
    }
}

/// Whether a session of `kind` starts by picking in the viewport, not
/// with the focus in its first field: a mirror's, an align's, a split's,
/// a blend's and a face session's.
fn picks_first(kind: MotionKind) -> bool {
    matches!(
        kind,
        MotionKind::Mirror | MotionKind::Align | MotionKind::Split
    ) || kind.blends()
        || kind.picks_faces()
}

/// Why a model edge or face can't be a move's axis or a mirror's plane,
/// in the words the status bar shows.
const NOT_AN_AXIS: &str = "Only a straight or round edge, or a round face, can be the axis";
const NOT_A_DIRECTION: &str =
    "Only a straight or round edge, or a round face, can give the direction";
const NOT_A_PLANE: &str = "Only a flat face can be the mirror plane";
const OUT_OF_DATE: &str = "The model shown is out of date: try again once it's regenerated";

impl Doc {
    /// Starts setting up a new move, mirror or pattern (`kind`), in a
    /// document that can be changed and outside a sketch, or cancels the
    /// one being set up (one of another kind is replaced). What's
    /// selected in the model gives it its bodies; with nothing selected, a
    /// model of one body gives that one, as the UI mock's. Another
    /// operation being set up is dropped. A move's first offset field, or
    /// a pattern's count, takes the focus.
    pub(crate) fn start_motion(&mut self, kind: MotionKind) {
        if (self.motion.take()).is_some_and(|session| session.kind == kind)
            || !self.editable()
            || self.sketch.is_some()
        {
            return;
        }
        self.picking_plane = None;
        self.extrude = None;
        self.revolve = None;
        self.combine = None;
        let mut bodies = self.selected_bodies();
        if bodies.is_empty() {
            bodies.extend(self.only_body());
        }
        self.motion = Some(MotionSession::new(kind, self.editor.document(), bodies));
        // A chamfer's edges are those selected that it takes.
        if kind.blends() {
            self.refs_selected::<EdgeRef>();
        }
        // A face session's faces are those selected that it takes.
        if kind.picks_faces() {
            self.refs_selected::<FaceRef>();
        }
        if !picks_first(kind) {
            self.focus = Some(Focus::All);
        }
    }

    /// The model's only body, if it has just one: of the bodies a new
    /// feature can name, those not merged into another.
    fn only_body(&self) -> Option<BodyId> {
        let document = self.editor.document();
        let merged = self.feed.merged_before(document, None);
        let mut bodies = (document.bodies().iter())
            .map(|body| body.id)
            .filter(|&body| merged.holder(body).is_none() && pickable(document, body, None));
        let only = bodies.next()?;
        bodies.next().is_none().then_some(only)
    }

    /// Edits the move, mirror or pattern feature `id`, if the document
    /// holds it, in a session with its values (a pattern's mode as last
    /// set up, [`Doc::pattern_shapes`]) and what of them is gone noted
    /// ([`MotionSession::prune`]), outside a sketch, in a document that
    /// can be changed. Another operation being set up is dropped.
    pub(crate) fn edit_motion(&mut self, id: FeatureId) {
        if self.sketch.is_some() || !self.editable() {
            return;
        }
        let shape = self.pattern_shapes.get(&id);
        let document = self.editor.document();
        let Some(mut session) = MotionSession::editing(document, id, shape) else {
            return;
        };
        // An axis or plane already gone (its body removed) is said to be
        // from the start.
        session.prune(document);
        self.picking_plane = None;
        self.extrude = None;
        self.revolve = None;
        self.combine = None;
        self.selected_feature = Some(id);
        if !picks_first(session.kind) {
            self.focus = Some(Focus::All);
        }
        self.motion = Some(session);
    }

    /// Takes `message`, changing the move, mirror or pattern being set up.
    pub(crate) fn motion_look(&mut self, message: MotionLook) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = &mut self.motion else {
            return;
        };
        match message {
            MotionLook::Cancel => self.motion = None,
            _ if !editable => {}
            // An align's references are its own; others pick none.
            MotionLook::Picking(MotionPick::Align(_) | MotionPick::Nothing)
                if session.kind != MotionKind::Align => {}
            MotionLook::Picking(MotionPick::Point | MotionPick::Edge)
                if session.kind != MotionKind::Scale => {}
            MotionLook::Picking(MotionPick::Reference)
                if matches!(
                    session.kind,
                    MotionKind::Align | MotionKind::Scale | MotionKind::Split
                ) => {}
            MotionLook::Picking(MotionPick::Tool) if session.kind != MotionKind::Split => {}
            // A chamfer picks only its edges: its field clicked turns
            // picking them off and on.
            MotionLook::Picking(MotionPick::Edges) if session.kind.blends() => {
                session.picking = match session.picking {
                    MotionPick::Edges => MotionPick::Nothing,
                    _ => MotionPick::Edges,
                };
            }
            MotionLook::Picking(_) if session.kind.blends() => {}
            MotionLook::Picking(MotionPick::Edges) => {}
            // A shell picks only its faces, the same way.
            MotionLook::Picking(MotionPick::Faces) if session.kind.picks_faces() => {
                session.picking = match session.picking {
                    MotionPick::Faces => MotionPick::Nothing,
                    _ => MotionPick::Faces,
                };
            }
            MotionLook::Picking(_) if session.kind.picks_faces() => {}
            MotionLook::Picking(MotionPick::Faces) => {}
            // A split's tool field clicked again while it picks stops
            // picking, so the split shows as set up.
            MotionLook::Picking(MotionPick::Tool) if session.picking == MotionPick::Tool => {
                session.picking = MotionPick::Nothing;
            }
            MotionLook::Picking(MotionPick::Edge)
                if session.scale.mode != varde_view::ScaleMode::EdgeLength => {}
            // A scale's point or edge field clicked again while it picks
            // hands the clicks back to the bodies.
            MotionLook::Picking(picking @ (MotionPick::Point | MotionPick::Edge))
                if session.picking == picking =>
            {
                session.picking = MotionPick::Bodies;
            }
            // An align's field clicked again while it picks stops picking,
            // so the align shows as set up without picking all it can take.
            MotionLook::Picking(picking @ MotionPick::Align(_)) if session.picking == picking => {
                session.picking = MotionPick::Nothing;
            }
            MotionLook::Picking(picking) => session.picking = picking,
            MotionLook::OriginAxis(axis) if session.kind == MotionKind::Align => {
                self.align_origin(Some(axis));
            }
            MotionLook::OriginPoint if session.kind == MotionKind::Scale => session.scale_origin(),
            MotionLook::OriginPoint => self.align_origin(None),
            MotionLook::ScaleMode(mode) if session.kind == MotionKind::Scale => {
                session.scale_mode(mode);
            }
            MotionLook::AxisOnly if session.kind == MotionKind::Scale => {
                session.scale.axis_only = !session.scale.axis_only;
            }
            MotionLook::ScaleMode(_) | MotionLook::AxisOnly => {}
            MotionLook::SplitWith(mode) if session.kind == MotionKind::Split => {
                session.split_mode(mode, document);
            }
            MotionLook::SplitRegion { sketch, region } if session.kind == MotionKind::Split => {
                session.split_region(sketch, region, document);
            }
            MotionLook::SplitCurve { sketch, curve } if session.kind == MotionKind::Split => {
                session.split_curve(sketch, curve, document);
            }
            MotionLook::Original(side) if session.kind == MotionKind::Split => {
                session.split.original = side;
            }
            MotionLook::Keep(keep) if session.kind == MotionKind::Split => {
                session.split.keep = keep;
            }
            MotionLook::OriginPlane(plane) if session.kind == MotionKind::Split => {
                session.split_origin(plane);
            }
            MotionLook::SplitWith(_)
            | MotionLook::SplitRegion { .. }
            | MotionLook::SplitCurve { .. }
            | MotionLook::Original(_)
            | MotionLook::Keep(_) => {}
            MotionLook::Clear(slot) if session.kind == MotionKind::Align => {
                session.align.clear(slot);
                // Clicks go on to what's needed first now, or to nothing
                // (and the preview) once it's whole; picking bodies stays.
                if session.picking != MotionPick::Bodies {
                    session.picking = session.align.next();
                }
            }
            MotionLook::Clear(_) => {}
            MotionLook::ChamferType(kind) if session.kind == MotionKind::Chamfer => {
                session.chamfer_type = kind;
            }
            MotionLook::Chain if session.kind.blends() => {
                session.blend.chains = !session.blend.chains;
            }
            MotionLook::DropEdge(edge) if session.kind.blends() => {
                session.blend.edges.drop_ref(&edge);
                session.blend_body();
            }
            MotionLook::ChamferType(_) | MotionLook::Chain | MotionLook::DropEdge(_) => {}
            MotionLook::DropFace(face) if session.kind.picks_faces() => {
                session.faces.drop_ref(&face);
                session.faces_body();
            }
            MotionLook::ShellDirection(direction) if session.kind == MotionKind::Shell => {
                session.direction = direction;
            }
            MotionLook::DropFace(_) | MotionLook::ShellDirection(_) => {}
            // A chamfer's body is its edges', a shell's its faces' (it
            // has no body rows).
            MotionLook::Drop(_) if session.kind.blends() || session.kind.picks_faces() => {}
            MotionLook::Drop(body) => {
                session.bodies.retain(|&picked| picked != body);
            }
            MotionLook::Input { field, text } => {
                let ask = field_ask(session.kind, field, &document.design());
                session.fields[field.index()].input(text, &ask);
            }
            // Only as the handles offer it: a move's, while its bodies
            // are picked, turning by nothing yet or about that world
            // axis already (the offsets are worked out as turning on
            // from there).
            MotionLook::Turn {
                axis,
                angle,
                offset,
            } if session.kind == MotionKind::Move
                && session.picking == MotionPick::Bodies
                && (session.angle().is_none_or(|angle| angle == 0.0)
                    || session.axis == Some(AxisRef::Origin(axis))) =>
            {
                let design = document.design();
                session.axis = Some(AxisRef::Origin(axis));
                (session.fields[MotionField::Angle.index()])
                    .input(angle, &Move::angle_ask(&design));
                let ask = Move::offset_ask(&design);
                for (axis, text) in Axis3::ALL.into_iter().zip(offset) {
                    session.fields[MotionField::Offset(axis).index()].input(text, &ask);
                }
            }
            MotionLook::Turn { .. } => {}
            MotionLook::OriginAxis(axis) if session.kind.takes_axis() => {
                session.axis = Some(AxisRef::Origin(axis));
                session.picking = MotionPick::Bodies;
            }
            MotionLook::OriginPlane(plane) if session.kind == MotionKind::Mirror => {
                session.plane = Some(PlaneRef::Origin(plane));
                session.picking = MotionPick::Bodies;
            }
            MotionLook::OriginAxis(_) | MotionLook::OriginPlane(_) => {}
            MotionLook::Copy => session.keep_original = !session.keep_original,
            MotionLook::Flip
                if matches!(
                    session.kind,
                    MotionKind::LinearPattern | MotionKind::Align | MotionKind::Chamfer
                ) =>
            {
                session.flip = !session.flip;
            }
            MotionLook::Mode(mode) if PatternMode::of(session.kind).contains(&mode) => {
                session.mode = mode;
            }
            MotionLook::Join if session.kind.pattern() => session.join = !session.join,
            MotionLook::Flip | MotionLook::Mode(_) | MotionLook::Join => {}
        }
    }

    /// Takes a click on the model while a move or mirror is set up: the
    /// body of what it's on while bodies are picked
    /// ([`Doc::motion_body`]), else the edge or face as the axis or plane
    /// ([`Doc::motion_reference`]). A click on nothing, or on a model no
    /// longer shown, does nothing.
    pub(crate) fn motion_click(&mut self, pick: Option<Pick>) {
        let Some(pick) = pick.filter(|pick| pick.model == self.feed.model()) else {
            return;
        };
        let Some(session) = &self.motion else {
            return;
        };
        if !self.picks() || !self.editable() {
            return;
        }
        let picked = match session.picking {
            MotionPick::Bodies => {
                self.motion_body(pick.body);
                Ok(())
            }
            MotionPick::Reference => self.motion_reference(pick),
            MotionPick::Align(slot) => self.align_pick(slot, pick),
            MotionPick::Point => self.scale_point(pick),
            MotionPick::Edge => self.scale_edge(pick),
            MotionPick::Tool => self.split_tool(pick),
            MotionPick::Edges => self.refs_click::<EdgeRef>(pick),
            MotionPick::Faces => self.refs_click::<FaceRef>(pick),
            MotionPick::Nothing => Ok(()),
        };
        if let Err(why) = picked {
            self.notice = Some(why.into_owned());
        }
    }

    /// Picks `body` for the move or mirror being set up, or takes it out:
    /// a body merged into another before the feature as the one holding
    /// it, which the model draws it as. Only bodies the feature can name,
    /// in a document that can be changed.
    pub(crate) fn motion_body(&mut self, body: BodyId) {
        if !self.editable() {
            return;
        }
        let body = self.named_body(body);
        let document = self.editor.document();
        let Some(session) = &mut self.motion else {
            return;
        };
        // A chamfer's body is its edges', never picked itself.
        if !pickable(document, body, session.feature) || session.kind.blends() {
            return;
        }
        // An align moves one body, a split splits one: another replaces
        // it. A face session's body is its faces': a shell takes one while
        // it has none (a closed one), which are then all on it.
        match session.kind {
            MotionKind::Align => self.align_body(body),
            MotionKind::Split => self.split_body(body),
            kind if kind.picks_faces() => {
                if session.faces.refs.is_empty() && faces::takes_body(kind) {
                    session.bodies = vec![body];
                } else if session.faces.body() != Some(body) {
                    let noun = refs::noun(kind);
                    self.notice = Some(format!(
                        "A {noun}'s faces are all on one body: take them out to pick another"
                    ));
                }
            }
            _ => session.toggle(body),
        }
    }

    /// Takes `pick` as the axis or plane of the move or mirror being set
    /// up ([`Doc::reference_of`]), handing the clicks back to bodies, or
    /// says why it can't be.
    fn motion_reference(&mut self, pick: Pick) -> Result<(), Cow<'static, str>> {
        if !self.feed.answers_request() || self.feed.predates_replacement() {
            return Err(OUT_OF_DATE.into());
        }
        let reference = self.reference_of(pick.target, pick.at)?;
        let Some(session) = &mut self.motion else {
            return Ok(());
        };
        match reference {
            Reference::Axis(axis) => session.axis = Some(axis),
            Reference::Plane(plane) => session.plane = Some(plane),
        }
        session.picking = MotionPick::Bodies;
        Ok(())
    }

    /// `target` of the model shown, picked at `at`, as the axis or plane
    /// of the move or mirror being set up, named as the feature stores it
    /// ([`Naming`], the history stopped at the feature): for a move a
    /// straight or round edge, or a round face (a cylinder's, cone's,
    /// torus's or other surface of revolution's); for a mirror a flat
    /// face. Refused, why, if it's none of those, made by the feature or
    /// a later one, or on a body which can't be told there.
    fn reference_of(&self, target: Picked, at: DVec3) -> Result<Reference, Cow<'static, str>> {
        let session = self.motion.as_ref().ok_or("Nothing is set up")?;
        let index = self.feed.pick_index();
        let naming = self.motion_naming().ok_or("Nothing is set up")?;
        let refused = |why: Unnamed, what: &str| unnamed(why, what, session.kind);
        let summary =
            |face: u32| (index.picking().faces().get(face as usize)).map(|face| face.summary);
        let not_an_axis = match session.kind {
            MotionKind::LinearPattern => NOT_A_DIRECTION,
            _ => NOT_AN_AXIS,
        };
        match (session.kind.takes_axis(), target) {
            (true, Picked::Edge(edge)) => {
                let keys = index.chain_keys(edge).ok_or(not_an_axis)?;
                let straight = index.edge_ends(edge, &keys).is_some();
                let round = round_edge(index, edge);
                if !(straight || round) {
                    return Err(not_an_axis.into());
                }
                let edge = naming
                    .edge_ref(index, edge, at)
                    .map_err(|why| refused(why, "edge"))?;
                Ok(Reference::Axis(AxisRef::Edge(edge)))
            }
            (true, Picked::Face(face)) => {
                let round = matches!(
                    summary(face),
                    Some(
                        Summary::Cylinder { .. }
                            | Summary::Cone { .. }
                            | Summary::Torus { .. }
                            | Summary::Revolved { .. }
                    )
                );
                if !round {
                    return Err(not_an_axis.into());
                }
                let face = naming
                    .checked_face_ref(index, face, at)
                    .map_err(|why| refused(why, "face"))?;
                Ok(Reference::Axis(AxisRef::Face(face)))
            }
            (false, Picked::Face(face)) => {
                if !matches!(summary(face), Some(Summary::Plane { .. })) {
                    return Err(NOT_A_PLANE.into());
                }
                let face = naming
                    .checked_face_ref(index, face, at)
                    .map_err(|why| refused(why, "face"))?;
                Ok(Reference::Plane(PlaneRef::Face(face)))
            }
            (true, _) => Err(not_an_axis.into()),
            (false, _) => Err(NOT_A_PLANE.into()),
        }
    }

    /// Whether the move or mirror being set up can be committed: the
    /// document can be changed, no sketch edits wait on the solver, and
    /// the session is ready ([`MotionSession::ready`]).
    pub(crate) fn motion_ready(&self) -> bool {
        let design = self.editor.document().design();
        self.motion.as_ref().is_some_and(|session| {
            self.editable()
                && !self.proposing()
                && session.ready(&design)
                && self.motion_held().is_none()
        })
    }

    /// Why the pattern being edited can't be set up as it is, if it
    /// would drop a copy body (fewer copies, a body taken out, joined to
    /// the original again) that a later feature names: the document
    /// refuses that rather than drop the feature, so the panel says so
    /// at once, and how to get past it, as an extrude's that would stop
    /// making the body a combine names.
    pub(crate) fn motion_held(&self) -> Option<String> {
        let session = self.motion.as_ref()?;
        if session.kind == MotionKind::Split {
            return self.split_held(session);
        }
        let edited = session.feature?;
        let kind = session.kind()?;
        let document = self.editor.document();
        let (feature, body) = copy_user(document, edited, &kind)?;
        let (feature, body) = (&feature.name, &body.name);
        Some(format!(
            "{feature} uses {body}, a copy this pattern would no longer make: take {body} out of {feature} or delete it first"
        ))
    }

    /// What the panel warns of for the pattern being set up, if anything:
    /// a linear one's copies, each a body of its own, overlapping, as the
    /// UI mock has it: the spacing shorter than one of its bodies is long
    /// that way (its faces in the model shown, so within its mesh), so
    /// that body's copies overlap each other, or the mock's words for it.
    /// Each body on its own: bodies far apart whose copies miss each
    /// other aren't one long body.
    fn motion_warning(&self, session: &MotionSession) -> Option<String> {
        if session.kind != MotionKind::LinearPattern
            || session.join
            || session.need().is_some()
            || session.gone().is_some()
        {
            return None;
        }
        let Ok(Some(pattern)) = session.pattern() else {
            return None;
        };
        let PatternKind::Linear { along, spacing, .. } = &pattern.kind else {
            return None;
        };
        let direction = match along {
            AxisRef::Origin(axis) => axis.direction(),
            _ => self.feed.draft_reference()?[1],
        };
        let shown = self.shown_bodies(&session.bodies);
        let index = self.feed.pick_index();
        let long = (shown.iter())
            .filter_map(|&body| index.bodies_extent(&[body], direction))
            .fold(None, |longest: Option<f64>, long| {
                Some(longest.map_or(long, |longest| longest.max(long)))
            })?;
        // The mock's slack, for copies end to end.
        (spacing.value.abs() < long - 1e-6).then(|| {
            let units = self.editor.document().units();
            let long = varde_expr::format(long, Some(Unit::Length(units)));
            format!(
                "The copies overlap ({long} long this way): tick Join to original to merge them"
            )
        })
    }

    /// Adds the move or mirror being set up, or changes the one edited,
    /// as one undo step, and ends the session: if it's ready
    /// ([`Doc::motion_ready`]), its preview failed only if `accept`
    /// ([`Doc::commit_by`]), and the document takes it. Refused, the
    /// session stays, and why shows.
    pub(crate) fn commit_motion(&mut self, accept: bool) {
        if !self.commit_by(self.motion_ready(), accept) {
            return;
        }
        let Some(session) = &self.motion else {
            return;
        };
        let Some(kind) = session.kind() else {
            return;
        };
        let edited = session.feature;
        let shape = (session.kind.pattern()).then(|| {
            let spread = session.field(MotionField::Spread).value.clone();
            (session.mode, session.flip, spread)
        });
        if self.commit_feature(edited, kind) {
            self.motion = None;
            // Its shape is kept for editing it again: the feature edited,
            // or the one added, which is selected.
            if let (Some((mode, flip, Some(spread))), Some(id)) =
                (shape, edited.or(self.selected_feature))
            {
                (self.pattern_shapes).insert(id, PatternShape { mode, flip, spread });
            }
        }
    }

    /// Ends the move or mirror session if the feature edited is gone, the
    /// document can't be changed any more, or it was replaced whole
    /// (`replaced`); lets go of what it can't name any more, keeps its
    /// values where the units changed, and follows bodies merged since.
    pub(crate) fn prune_motion(&mut self, replaced: bool) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = &mut self.motion else {
            return;
        };
        // The feature edited is still there, of the session's kind (an
        // undo may have set another kind of feature in its place).
        let edited = session.feature.is_none_or(|feature| {
            (document.feature(feature)).and_then(|feature| MotionKind::of(&feature.kind))
                == Some(session.kind)
        });
        if !(editable && !replaced && edited) {
            self.motion = None;
            return;
        }
        session.prune(document);
        session.follow_units(document);
        self.follow_motion_merges();
    }

    /// Moves the bodies of the move or mirror being set up on to the
    /// bodies holding them where the model shown has them merged before
    /// it ([`MotionSession::follow`]): whether they moved.
    pub(crate) fn follow_motion_merges(&mut self) -> bool {
        let Some(session) = &self.motion else {
            return false;
        };
        let merges = (self.feed).merged_before(self.editor.document(), session.feature);
        (self.motion.as_mut()).is_some_and(|session| session.follow(&merges))
    }

    /// Finds where the handles of the move being set up stand
    /// ([`Pivot`]) if it isn't known for its bodies and the document as
    /// they are: once the model shown answers what was asked last, from
    /// its bodies' box centre, through the move it was asked with (none
    /// for a new one not previewed yet); not while that preview failed,
    /// when the model shows the bodies where the document has them.
    pub(crate) fn follow_motion_pivot(&mut self) {
        let generation = self.editor.generation();
        let Some(session) = &self.motion else {
            return;
        };
        let current =
            |pivot: &Pivot| pivot.bodies == session.bodies && pivot.generation == generation;
        if session.kind != MotionKind::Move
            || session.picking != MotionPick::Bodies
            || session.bodies.is_empty()
            || session.pivot.as_ref().is_some_and(current)
            || !self.feed.answers_request()
            || self.feed.predates_replacement()
        {
            return;
        }
        let shown = self.shown_bodies(&session.bodies);
        let Some([low, high]) = self.feed.pick_index().bodies_bounds(&shown) else {
            return;
        };
        let centre = (low + high) / 2.0;
        let at = match self.motion_draft() {
            None if session.feature.is_none() => Some(centre),
            Some((_, FeatureKind::Move(moved))) if self.feed.draft_error().is_none() => {
                undo_motion(&moved, self.feed.draft_reference(), centre)
            }
            _ => None,
        };
        let Some(at) = at.filter(|at| at.is_finite()) else {
            return;
        };
        if let Some(session) = &mut self.motion {
            session.pivot = Some(Pivot {
                bodies: session.bodies.clone(),
                generation,
                at,
            });
        }
    }

    /// `bodies` where the model shown has them: a merged one as the body
    /// holding it.
    fn shown_bodies(&self, bodies: &[BodyId]) -> Vec<BodyId> {
        let merged = self.feed.merged_bodies();
        (bodies.iter())
            .map(|&body| {
                (merged.iter())
                    .find(|(consumed, _)| *consumed == body)
                    .map_or(body, |&(_, holder)| holder)
            })
            .collect()
    }

    /// The move, mirror or pattern being set up as the regeneration lane
    /// previews it, see [`MotionSession::draft`]; none where that would
    /// drop a copy body a later feature names ([`copy_user`]), which the
    /// document refuses: the model is then shown as committed, rather
    /// than as a failure (a pattern previewed as a move by nothing while
    /// its axis is picked has no copy bodies).
    pub(crate) fn motion_draft(&self) -> Option<(Option<FeatureId>, FeatureKind)> {
        let session = self.motion.as_ref()?;
        let document = self.editor.document();
        let (feature, kind) = session.draft(&document.design())?;
        if let Some(edited) = feature
            && copy_user(document, edited, &kind).is_some()
        {
            return None;
        }
        if self.split_held(session).is_some() {
            return None;
        }
        Some((feature, kind))
    }

    /// The naming of picks as of the move, mirror, pattern, align, scale
    /// or split being set up ([`Naming`]): the history stopped at its
    /// feature, all of it for a new one.
    fn motion_naming(&self) -> Option<Naming> {
        let session = self.motion.as_ref()?;
        let document = self.editor.document();
        let features = document.features();
        let before = (session.feature)
            .and_then(|id| features.iter().position(|feature| feature.id == id))
            .unwrap_or(features.len());
        Some(Naming::before(document, before, self.shown()))
    }

    /// The edge or face hovered, if it's one a click takes as the axis or
    /// plane: lit as the cursor's over it.
    fn takes_reference(&self, pick: Pick) -> bool {
        let Some(session) = &self.motion else {
            return false;
        };
        // A blend's edges wait only for a model of the document as it is.
        if session.picking == MotionPick::Edges {
            return self.refs_take::<EdgeRef>(pick);
        }
        if session.picking == MotionPick::Faces {
            return self.refs_take::<FaceRef>(pick);
        }
        (self.feed.answers_request() && !self.feed.predates_replacement())
            && match session.picking {
                MotionPick::Align(slot) => self.align_reference(slot, pick).is_ok(),
                MotionPick::Point => self.scale_point_of(pick).is_ok(),
                MotionPick::Edge => self.scale_edge_of(pick).is_ok(),
                MotionPick::Tool => self.split_tool_of(pick).is_ok(),
                _ => self.reference_of(pick.target, pick.at).is_ok(),
            }
    }

    /// Rebuilds the move's or mirror's highlight if what it's of changed:
    /// its bodies' faces as selected; while bodies are picked the body
    /// hovered, and while the axis or plane is the edge or face hovered
    /// if a click takes it, as hovered; a body whose row in the panel is
    /// hovered lit, even one picked, which in the view keeps its colour
    /// under the cursor.
    pub(crate) fn refresh_motion_highlight(&mut self) {
        if !self.picks() {
            return;
        }
        self.follow_align();
        self.follow_scale();
        let Some(session) = &self.motion else {
            return;
        };
        if session.kind.blends() {
            self.follow_ref_marks::<EdgeRef>();
        } else if session.kind.picks_faces() {
            self.follow_ref_marks::<FaceRef>();
        }
        let Some(session) = &self.motion else {
            return;
        };
        // A chamfer's edge or a shell's face whose row is hovered.
        let panel_edge = match self.panel_hover() {
            Some(PanelHover::Edge(at)) => self.ref_hovered::<EdgeRef>(at),
            Some(PanelHover::Face(at)) => self.ref_hovered::<FaceRef>(at),
            _ => None,
        };
        let panel = self.panel_hover().and_then(PanelHover::body);
        let hovered: Option<(Picked, bool)> = match (panel, self.pick.hover()) {
            (Some(body), _) => {
                let index = self.feed.pick_index();
                index
                    .body_faces(body)
                    .next()
                    .map(|face| (Picked::Face(face), true))
            }
            // A chamfer's edge hovered in its panel lights alone.
            (None, _) if panel_edge.is_some() => panel_edge.map(|edge| (edge, false)),
            (None, Some(pick)) => match session.picking {
                MotionPick::Bodies => {
                    (!session.bodies.contains(&pick.body)).then_some((pick.target, false))
                }
                MotionPick::Reference
                | MotionPick::Align(_)
                | MotionPick::Point
                | MotionPick::Edge
                | MotionPick::Tool
                | MotionPick::Edges
                | MotionPick::Faces => self.takes_reference(pick).then_some((pick.target, false)),
                MotionPick::Nothing => None,
            },
            (None, None) => None,
        };
        let lit = match session.kind {
            MotionKind::Scale => [Vec::new(), self.scale_lit()],
            MotionKind::Split => [Vec::new(), self.split_lit()],
            _ if session.kind.blends() => [self.blend_lit(), Vec::new()],
            _ if session.kind.picks_faces() => [self.faces_lit(), Vec::new()],
            _ => self.align_lit(),
        };
        // A split's tool body lights whole while hovered.
        let tool_body = session.kind == MotionKind::Split && session.split.mode == SplitMode::Body;
        let key = (
            self.feed.model(),
            hovered,
            session.picking,
            session.bodies.clone(),
            lit,
            tool_body,
            session.blend.chains,
        );
        if session.built.as_ref() == Some(&key) {
            return;
        }
        let index = self.feed.pick_index();
        let faces = |body: BodyId| index.body_faces(body).map(Picked::Face);
        let lit = hovered
            .filter(|&(_, panel)| panel)
            .and_then(|(target, _)| index.body(target));
        // While an align's references are picked, those picked for its
        // directions are lit, the moved side's as selected, the target's
        // in the second colour, rather than its body.
        let (picked, second): (Vec<Picked>, Vec<Picked>) = match session.picking {
            MotionPick::Align(_) => {
                let [moved, target] = key.4.clone();
                (moved, target)
            }
            // A chamfer's edges and a shell's faces lit as selected, its
            // body as it is.
            _ if session.kind.blends() || session.kind.picks_faces() => {
                (key.4[0].clone(), Vec::new())
            }
            _ => (
                (session.bodies.iter())
                    .filter(|&&body| Some(body) != lit)
                    .flat_map(|&body| faces(body))
                    .collect(),
                // A scale's edge in the second colour, on its bodies; a
                // split's piece not keeping the body's id.
                if matches!(session.kind, MotionKind::Scale | MotionKind::Split) {
                    key.4[1].clone()
                } else {
                    Vec::new()
                },
            ),
        };
        let hover: Vec<Picked> = match (hovered, session.picking) {
            (Some((target, true)), _) | (Some((target, false)), MotionPick::Bodies) => index
                .body(target)
                .map(|body| faces(body).collect())
                .unwrap_or_default(),
            (Some((target, false)), MotionPick::Tool) if tool_body => index
                .body(target)
                .map(|body| faces(body).collect())
                .unwrap_or_default(),
            // A chamfer's edge with its tangent chain, while it takes it in.
            (Some((target, false)), MotionPick::Edges) => match self.pick.hover() {
                Some(pick) if panel_edge.is_none() => self.blend_hover(pick),
                _ => vec![target],
            },
            (
                Some((target, false)),
                MotionPick::Reference
                | MotionPick::Align(_)
                | MotionPick::Point
                | MotionPick::Edge
                | MotionPick::Tool
                | MotionPick::Faces,
            ) => vec![target],
            // A chamfer's edge hovered in its panel, while its edges
            // aren't picked.
            (Some((target, false)), MotionPick::Nothing) if panel_edge.is_some() => vec![target],
            (Some((_, false)), MotionPick::Nothing) | (None, _) => Vec::new(),
        };
        let highlight = Arc::new(index.highlight_with(&hover, &picked, &second));
        if let Some(session) = &mut self.motion {
            session.highlight = highlight;
            session.built = Some(key);
        }
    }

    /// The move's or mirror's highlight, if one is set up and it's built
    /// for the model shown.
    pub(crate) fn motion_highlight(&self) -> Option<&Arc<ModelHighlight>> {
        let session = self.motion.as_ref()?;
        let current = (session.built.as_ref()).is_some_and(|built| built.0 == self.feed.model());
        Some(&session.highlight).filter(|highlight| current && !highlight.is_empty())
    }

    /// The move or mirror being set up, for the view.
    pub(crate) fn motion_state(&self) -> Option<MotionState<'_>> {
        let session = self.motion.as_ref()?;
        let document = self.editor.document();
        let design = document.design();
        // A body gone from the document stays listed, as the UI mock's.
        let named = |body: BodyId| CombineBody {
            body,
            name: (document.body(body)).map_or("Missing body", |body| body.name.as_str()),
        };
        let editing = session
            .feature
            .and_then(|feature| document.feature(feature))
            .map(|feature| feature.name.as_str());
        let (reference, origin) = match session.kind {
            MotionKind::Move | MotionKind::LinearPattern | MotionKind::CircularPattern => {
                let axis = session.axis.as_ref();
                let origin = axis.and_then(|axis| match axis {
                    AxisRef::Origin(axis) => Some([DVec3::ZERO, axis.direction()]),
                    _ => None,
                });
                (axis.map(|axis| axis_name(document, axis)), origin)
            }
            MotionKind::Align
            | MotionKind::Scale
            | MotionKind::Chamfer
            | MotionKind::Shell
            | MotionKind::Fillet => (None, None),
            MotionKind::Split => self.split_reference(session),
            MotionKind::Mirror => {
                let plane = session.plane.as_ref();
                let origin = plane.and_then(|plane| match plane {
                    PlaneRef::Origin(plane) => Some([DVec3::ZERO, plane.placement().normal]),
                    _ => None,
                });
                (plane.map(|plane| plane_name(document, plane)), origin)
            }
        };
        // A move turning by nothing doesn't show its axis unless it's
        // being picked.
        let turning = session.kind != MotionKind::Move
            || session.picking == MotionPick::Reference
            || session.angle().is_some_and(|angle| angle != 0.0);
        // A flipped linear pattern's arrow points the way its copies go.
        let flipped = session.kind == MotionKind::LinearPattern && session.flip;
        let line = origin
            .or_else(|| self.feed.draft_reference())
            .filter(|_| turning && reference.is_some())
            .map(|[point, along]| [point, if flipped { -along } else { along }]);
        // The bodies where the model shown has them: a merged one in its
        // holder.
        let shown = self.shown_bodies(&session.bodies);
        let bounds = self.feed.pick_index().bodies_bounds(&shown);
        let generation = self.editor.generation();
        let centre = (session.pivot.as_ref())
            .filter(|pivot| pivot.bodies == session.bodies && pivot.generation == generation)
            .and_then(|pivot| {
                let line = match session.axis {
                    Some(AxisRef::Origin(_)) | None => None,
                    Some(_) => self.feed.draft_reference(),
                };
                session.centre(pivot.at, line)
            });
        let fields = MotionField::ALL.map(|field| session.field(field).field());
        Some(MotionState {
            kind: session.kind,
            editing,
            bodies: session.bodies.iter().map(|&body| named(body)).collect(),
            picking: session.picking,
            fields,
            reference,
            line,
            bounds,
            centre,
            origin_axis: match session.axis {
                Some(AxisRef::Origin(axis)) => Some(axis),
                _ => None,
            },
            units: document.units(),
            keep_original: session.keep_original,
            flip: session.flip,
            join: session.join,
            warning: (self.motion_warning(session))
                .or_else(|| self.scale_note(session))
                .or_else(|| session.shell_warning()),
            mode: session.mode,
            spread_error: session.spread_error(),
            copies: match session.kind() {
                Some(FeatureKind::Pattern(pattern)) => Some(pattern_copies(document, &pattern)),
                _ => None,
            },
            need: session.need(),
            refused: (session.gone().map(str::to_owned))
                .or_else(|| session.refused(&design))
                .or_else(|| self.motion_held()),
            error: self.feed.draft_error(),
            show_error: self.draft_framed(),
            checking: self.proposals.slow(),
            ready: self.commit_by(self.motion_ready(), false),
            accept: self.commit_by(self.motion_ready(), true),
            editable: self.editable(),
            hover: self.panel_hover(),
            align: (session.kind == MotionKind::Align).then(|| Box::new(self.align_view(session))),
            scale: (session.kind == MotionKind::Scale).then(|| Box::new(self.scale_view(session))),
            split: (session.kind == MotionKind::Split).then(|| Box::new(self.split_view(session))),
            chamfer: (session.kind == MotionKind::Chamfer)
                .then(|| Box::new(self.chamfer_view(session))),
            shell: (session.kind == MotionKind::Shell).then(|| Box::new(self.shell_view(session))),
            fillet: (session.kind == MotionKind::Fillet)
                .then(|| Box::new(self.fillet_view(session))),
        })
    }
}

/// The motion of `moved`: its turn, about an origin axis or the line
/// `line` regenerating found for another axis, then its shift, as
/// regenerating works it out; `None` where the line isn't known.
fn motion_of(moved: &Move, line: Option<[DVec3; 2]>) -> Option<Motion> {
    let turn = match &moved.turn {
        Some((axis, angle)) => {
            let [point, along] = match axis {
                AxisRef::Origin(axis) => [DVec3::ZERO, axis.direction()],
                _ => line?,
            };
            Some((point, along, angle.value / (PI / 180.0)))
        }
        None => None,
    };
    let shift = Motion::translation(moved.offset_vector())?;
    match turn {
        Some((point, along, degrees)) => Some(Motion::turn(point, along, degrees)?.then(&shift)),
        None => Some(shift),
    }
}

/// The point `moved` takes to `p`, see [`motion_of`].
fn undo_motion(moved: &Move, line: Option<[DVec3; 2]>, p: DVec3) -> Option<DVec3> {
    let back = Motion::translation(-moved.offset_vector())?;
    let back = match &moved.turn {
        Some((axis, angle)) => {
            let [point, along] = match axis {
                AxisRef::Origin(axis) => [DVec3::ZERO, axis.direction()],
                _ => line?,
            };
            back.then(&Motion::turn(point, along, -(angle.value / (PI / 180.0)))?)
        }
        None => back,
    };
    let at = back.point(p);
    at.is_finite().then_some(at)
}

/// Whether `edge` of `index`'s model is round as the mesh draws it: a
/// circle or an arc of one, every point of its polyline as far from its
/// snap point (a round edge's centre) within the rounding of the mesh's
/// `f32` points and a millionth of its radius; not an ellipse, which
/// regenerating refuses as an axis. Regenerating decides from the exact
/// curves.
fn round_edge(index: &varde_view::PickIndex, edge: u32) -> bool {
    edge_radius(index, edge).is_some()
}

/// The radius of `edge` of `index`'s model if it's round as
/// [`round_edge`] tells: its largest distance from its centre.
fn edge_radius(index: &varde_view::PickIndex, edge: u32) -> Option<f64> {
    let Some(Some(centre)) = index.picking().snaps().get(edge as usize) else {
        return None;
    };
    let centre = DVec3::from(*centre);
    let mesh = index.mesh();
    let polyline = mesh.polyline(edge as usize)?;
    let points: Vec<DVec3> = (polyline.iter())
        .filter_map(|&i| mesh.positions().get(i as usize))
        .map(|&p| glam::Vec3::from(p).as_dvec3())
        .collect();
    let radii = points.iter().map(|p| p.distance(centre));
    let (low, high) = radii.fold((f64::INFINITY, 0.0_f64), |(low, high), r| {
        (low.min(r), high.max(r))
    });
    let scale = (points.iter())
        .map(|p| p.abs().max_element())
        .fold(centre.abs().max_element(), f64::max);
    let slack = 1e-6 * high + 8.0 * f64::from(f32::EPSILON) * scale;
    (points.len() >= 2 && low > slack && high - low <= slack).then_some(high)
}

/// The body `axis`'s edge or face is on, if it names one.
fn axis_body(axis: &AxisRef) -> Option<BodyId> {
    match axis {
        AxisRef::Origin(_) => None,
        AxisRef::Edge(edge) => Some(edge.body),
        AxisRef::Face(face) => Some(face.body),
    }
}

/// The body `plane`'s face is on, if it names one.
fn plane_body(plane: &PlaneRef) -> Option<BodyId> {
    match plane {
        PlaneRef::Origin(_) => None,
        PlaneRef::Face(face) => Some(face.body),
    }
}

/// An edge or face taken as a move's axis or a mirror's plane.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Reference {
    Axis(AxisRef),
    Plane(PlaneRef),
}

/// The first feature naming ([`FeatureKind::bodies`]) a copy body that
/// setting the pattern `edited` to `kind` would drop
/// ([`Document::copies_dropped`]), and that body: the document refuses
/// the edit while there's one.
fn copy_user<'a>(
    document: &'a Document,
    edited: FeatureId,
    kind: &FeatureKind,
) -> Option<(&'a varde_document::Feature, &'a varde_document::Body)> {
    let dropped = document.copies_dropped(edited, kind);
    if dropped.is_empty() {
        return None;
    }
    (document.features().iter()).find_map(|feature| {
        let named = feature.kind.bodies();
        let body = dropped.iter().find(|body| named.contains(body))?;
        Some((feature, document.body(*body)?))
    })
}

/// `Naming`'s refusal `why` of a pick of `what` ("corner", "edge",
/// "face", "body") for the `kind` being set up, as words for the status
/// bar: "Only an edge made before the scale can be picked".
fn unnamed(why: Unnamed, what: &str, kind: MotionKind) -> Cow<'static, str> {
    let noun = kind.noun().to_lowercase();
    match why {
        Unnamed::Missing => format!("That {what} isn't in the model").into(),
        Unnamed::Later => {
            let article = if what.starts_with('e') { "an" } else { "a" };
            format!("Only {article} {what} made before the {noun} can be picked").into()
        }
        Unnamed::Unclear => {
            format!("Which body that {what} is on at the {noun} can't be told: pick another").into()
        }
    }
}

mod align;
mod blend;
mod chamfer;
mod faces;
mod fillet;
mod refs;
mod scale;
mod shell;
mod split;

#[cfg(test)]
mod tests;
