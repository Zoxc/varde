//! A pattern in the move's session ([`MotionKind::LinearPattern`],
//! [`MotionKind::CircularPattern`]): opening a stored one (its mode, Flip
//! and spread taken from how it was last set up while that gives what's
//! stored, see [`PatternShape`]) and making the pattern of the count and
//! spread typed, Flip and mode.

use std::f64::consts::TAU;

use varde_document::{Copies, Design, MAX_PATTERN_COUNT, Pattern, PatternKind};
use varde_expr::{Ask, ErrorKind, Unit, Value};
use varde_view::{MotionField, MotionKind, PatternMode};

use super::{DEGREES, MotionSession};
use crate::doc::regions::TypedText;

/// An edited pattern's spacing or angle as stored, and the mode, Flip
/// and spread field the session opened it with: while those are as they
/// opened, the pattern keeps the stored value, text and all, so OK with
/// nothing changed writes nothing even where the session would write the
/// same value another way ("-15" read with Flip on would come back
/// "-(15)", "360" for Full 360° as "360°").
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Opened {
    mode: PatternMode,
    flip: bool,
    spread: TypedText,
    stored: Value,
}

/// What a pattern of `kind`'s spread is read with in `design`: a circular
/// one's an angle as the pattern stores it, a linear one's a length above
/// zero (the Flip direction gives it its sign) within the coordinate
/// limit.
pub(super) fn spread_ask<'p>(kind: MotionKind, design: &Design<'p>) -> Ask<'p> {
    match kind {
        MotionKind::CircularPattern => Pattern::angle_ask(design),
        _ => Pattern::spacing_ask(design).positive(),
    }
}

/// How a pattern was last set up in a session, kept by the app for the
/// feature committed ([`Doc::pattern_shapes`](crate::doc::Doc::pattern_shapes)): the mode and Flip the
/// user chose and the spread as typed, which the stored values can't
/// tell apart (a spacing typed as a total, a circular angle typed as a
/// spacing). Taken again on editing it only while it gives the values
/// stored, see [`MotionSession::editing`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PatternShape {
    pub(super) mode: PatternMode,
    pub(super) flip: bool,
    pub(super) spread: Value,
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
    /// Opens the pattern `pattern` in this session: its axis, joining and
    /// count, its mode, Flip and spread as [`MotionSession::take_shape`]
    /// takes them (`shape`, how it was last set up, if it gives what's
    /// stored), and those as opened, see [`Opened`].
    pub(super) fn open_pattern(&mut self, pattern: &Pattern, shape: Option<&PatternShape>) {
        self.axis = Some(*pattern.kind.axis());
        self.join = pattern.joins();
        self.fields[MotionField::Count.index()] = TypedText::of(
            pattern.kind.count_value(),
            &Pattern::count_ask(&self.read_in.design()),
        );
        self.take_shape(pattern, shape);
        self.opened = Some(Opened {
            mode: self.mode,
            flip: self.flip,
            spread: self.field(MotionField::Spread).clone(),
            stored: pattern.kind.spread().clone(),
        });
    }

    /// Sets a pattern's mode, Flip and spread to `shape`'s if they give
    /// what `pattern` stores, else as [`MotionSession::editing`] reads
    /// them from it.
    fn take_shape(&mut self, pattern: &Pattern, shape: Option<&PatternShape>) {
        let ask = spread_ask(self.kind, &self.read_in.design());
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

    /// The pattern as set up, its count and spread as they last read:
    /// `Ok(None)` while it isn't whole (no bodies, no axis, no value),
    /// and why its spread is refused as a whole, in the UI mock's words,
    /// if it is (its own text read, the spacing, total or angle it comes
    /// to is beyond what's stored). Stored, a linear pattern's spacing is
    /// as typed, a Total's divided by the count less one, either turned
    /// by Flip direction ("-(...)"); a circular one's angle is 360° for
    /// Full 360°, the Total as typed, or a Spacing's times the count less
    /// one, refused where either comes to a full turn or more.
    pub(super) fn pattern(&self) -> Result<Option<Pattern>, String> {
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
        let design = &self.read_in.design();
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
                let limit = f64::from(varde_kernel_compat::MAX_COORD);
                if self.mode != PatternMode::Total && spread.value * f64::from(steps) > limit {
                    let limit = varde_expr::Shown::current()
                        .format(limit, Some(Unit::Length(design.units)));
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
                            let step =
                                varde_expr::Shown::current().format(spread.value, Some(DEGREES));
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
            | MotionKind::Fillet
            | MotionKind::OffsetFace
            | MotionKind::Draft
            | MotionKind::Sweep
            | MotionKind::Loft => {
                return Ok(None);
            }
        };
        let mut kind = kind;
        if let Some(opened) = &self.opened
            && (opened.mode, opened.flip) == (self.mode, self.flip)
            && opened.spread == *self.field(MotionField::Spread)
        {
            let spread = kind.spread_mut();
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
    pub(super) fn spread_error(&self) -> Option<String> {
        self.pattern().err()
    }
}
