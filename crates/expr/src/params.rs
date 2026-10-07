//! Parameters: values a design names once, such as `height = 40 mm`, for
//! its other values to use by name, `height / 2`.
//!
//! A parameter's text is an expression like any other, and may use other
//! parameters, in any order; [`Params::evaluate`] resolves a design's list
//! of them at once, each to a value or the error that kept it from one: a
//! name nothing defines, a parameter that uses itself through others, or
//! one that uses a parameter in error. A value that uses a parameter in
//! error is refused; a parameter in error that nothing uses is harmless.
//!
//! A parameter is a length, an angle or a number, whichever its text
//! comes to; bare numbers alone are a number, so `50` is a number, refused
//! where a length is asked for, and `50 mm` is a length. Where a value
//! uses a parameter its units are already settled, so it takes none of
//! the design's.

use std::collections::BTreeMap;
use std::fmt;

use crate::eval::{Dim, Quant, evaluate_param, pin_param_units};
use crate::parse::{name_spans, unit, word_char};
use crate::{Error, ErrorKind, LengthUnit, MAX_LEN, Quantity, Span};

/// The most parameters a design may have.
pub const MAX_PARAMS: usize = 1000;

/// The most bytes a parameter's name may have.
pub const MAX_NAME_LEN: usize = 64;

/// What a parameter came to: its value in model units (millimetres,
/// radians) and which of a length, an angle or a number it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resolved {
    pub value: f64,
    pub quantity: Quantity,
}

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    name: String,
    /// Its expression as given, for sending the list on whole.
    text: String,
    result: Result<Resolved, Error>,
}

/// A design's parameters, resolved: each name, in the design's order,
/// with its value or why it has none. What an [`Ask`](crate::Ask) looks
/// names up in.
#[derive(Debug, Clone)]
pub struct Params {
    entries: Vec<Entry>,
    /// Where the first of each name is.
    by_name: BTreeMap<String, usize>,
    /// Whether an unknown name looks for a near one to suggest: off while
    /// [`Params::evaluate`] runs, which suggests within a budget after.
    suggest: bool,
}

/// The most candidate names [`Params::evaluate`] compares unknown names
/// with, all told, for suggestions: past it they suggest nothing, so a
/// design full of unknown names still evaluates quickly.
const SUGGESTION_BUDGET: usize = 20_000;

impl Default for Params {
    fn default() -> Self {
        Params::EMPTY.clone()
    }
}

/// The same parameters, results and all; the rest follows from them.
impl PartialEq for Params {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl Params {
    /// No parameters: any name is unknown.
    pub const EMPTY: &'static Params = &Params {
        entries: Vec::new(),
        by_name: BTreeMap::new(),
        suggest: true,
    };

    /// Resolves `list`, each parameter's name and text, in a design in
    /// `units`. Each comes out with a value or an error: its text's own,
    /// [`ErrorKind::Cycle`] where it uses itself through others,
    /// [`ErrorKind::BrokenParam`] where it uses one in error,
    /// [`ErrorKind::BadName`] or [`ErrorKind::Duplicate`] for its name
    /// (the first of a name is the one used), and
    /// [`ErrorKind::TooManyParams`] past [`MAX_PARAMS`].
    pub fn evaluate<'t>(
        list: impl IntoIterator<Item = (&'t str, &'t str)>,
        units: LengthUnit,
    ) -> Params {
        let list: Vec<(&str, &str)> = list.into_iter().collect();
        let mut params = Params {
            entries: list
                .iter()
                .map(|&(name, text)| Entry {
                    name: name.to_owned(),
                    text: text.to_owned(),
                    // Replaced below before anything reads it.
                    result: Err(Error {
                        kind: ErrorKind::Cycle,
                        span: Span::new(0, 0),
                    }),
                })
                .collect(),
            by_name: BTreeMap::new(),
            suggest: false,
        };
        for (index, &(name, _)) in list.iter().enumerate() {
            params.by_name.entry(name.to_owned()).or_insert(index);
        }
        // Whether each has its result: errors about the name, or once
        // evaluated or found in a cycle.
        let mut done = vec![false; list.len()];
        for (index, &(name, text)) in list.iter().enumerate() {
            let all = Span::new(0, text.len());
            let kind = if index >= MAX_PARAMS {
                ErrorKind::TooManyParams
            } else if let Err(error) = check_name(name) {
                ErrorKind::BadName(error)
            } else if params.index(name) != Some(index) {
                ErrorKind::Duplicate(name.to_owned())
            } else {
                continue;
            };
            params.entries[index].result = Err(Error { kind, span: all });
            done[index] = true;
        }
        // Each one's uses of the others, by index, with the name's span.
        let uses: Vec<Vec<(usize, Span)>> = list
            .iter()
            .enumerate()
            .map(|(index, &(_, text))| {
                if done[index] {
                    return Vec::new();
                }
                name_spans(text)
                    .into_iter()
                    .filter_map(|span| Some((params.index(&text[span.range()])?, span)))
                    .collect()
            })
            .collect();

        // Depth first through the uses, without recursion so that a long
        // chain can't overflow the stack: a parameter is evaluated once
        // all it uses are, and one met again while still open closes a
        // cycle.
        #[derive(Clone, Copy, PartialEq)]
        enum State {
            New,
            Open,
            Closed,
        }
        let mut state = vec![State::New; list.len()];
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for root in 0..list.len() {
            if state[root] != State::New {
                continue;
            }
            state[root] = State::Open;
            stack.push((root, 0));
            while let Some(&mut (at, ref mut next)) = stack.last_mut() {
                if let Some(&(used, _)) = uses[at].get(*next) {
                    *next += 1;
                    match state[used] {
                        State::New => {
                            state[used] = State::Open;
                            stack.push((used, 0));
                        }
                        State::Open => {
                            let from = stack
                                .iter()
                                .position(|&(open, _)| open == used)
                                .expect("open ones are on the stack");
                            for &(member, next) in &stack[from..] {
                                if !done[member] {
                                    // The use that leads on round the cycle.
                                    let (_, span) = uses[member][next - 1];
                                    params.entries[member].result = Err(Error {
                                        kind: ErrorKind::Cycle,
                                        span,
                                    });
                                    done[member] = true;
                                }
                            }
                        }
                        State::Closed => {}
                    }
                    continue;
                }
                stack.pop();
                state[at] = State::Closed;
                if !done[at] {
                    let result = evaluate_param(list[at].1, units, &params);
                    params.entries[at].result = result;
                    done[at] = true;
                }
            }
        }
        params.suggest_within_budget();
        params.suggest = true;
        params
    }

    /// Fills in suggestions for the unknown names in the parameters'
    /// errors, as far as [`SUGGESTION_BUDGET`] goes.
    fn suggest_within_budget(&mut self) {
        let mut budget = SUGGESTION_BUDGET;
        let mut found = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if let Err(Error {
                kind: ErrorKind::UnknownName { name, .. },
                ..
            }) = &entry.result
            {
                if budget == 0 {
                    break;
                }
                if let Some(near) = self.nearest(name, &mut budget) {
                    found.push((index, near));
                }
            }
        }
        for (index, near) in found {
            if let Err(Error {
                kind: ErrorKind::UnknownName { suggestion, .. },
                ..
            }) = &mut self.entries[index].result
            {
                *suggestion = Some(near);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Each parameter's name and result, in the design's order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Result<Resolved, Error>)> {
        self.entries
            .iter()
            .map(|entry| (entry.name.as_str(), &entry.result))
    }

    /// Each parameter's name and text as they were given to
    /// [`Params::evaluate`], in order: for evaluating them again elsewhere,
    /// such as across to a Web Worker.
    pub fn sources(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.text.as_str()))
    }

    /// The result of the parameter at `index` in the design's order.
    pub fn result(&self, index: usize) -> Option<&Result<Resolved, Error>> {
        self.entries.get(index).map(|entry| &entry.result)
    }

    /// The result of the parameter named `name`, the first of the name.
    pub fn get(&self, name: &str) -> Option<&Result<Resolved, Error>> {
        self.index(name).and_then(|index| self.result(index))
    }

    /// Where the first parameter named `name` is.
    pub fn index(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }

    /// A parameter's `text` with the design's length unit `units` written
    /// in where its bare numbers took it, as [`pin_units`](crate::pin_units)
    /// does for a value, these being the parameters it's read with: for
    /// changing the design's units, `units` being the ones before. Refused
    /// where the text is in error, the units take it over [`MAX_LEN`], or
    /// it wouldn't come to the same; the caller can write the value
    /// instead ([`exact`](crate::exact)).
    pub fn pin_units(&self, text: &str, units: LengthUnit) -> Result<String, Error> {
        let pinned = pin_param_units(text, units, self)?;
        let before = evaluate_param(text, units, self)?;
        match evaluate_param(&pinned, units, self)? {
            after if after == before => Ok(pinned),
            after => Err(Error {
                kind: ErrorKind::Disagrees {
                    stored: before.value,
                    evaluated: after.value,
                },
                span: Span::new(0, text.len()),
            }),
        }
    }

    /// The value of `name`, used at `span` in an expression.
    pub(crate) fn quant(&self, name: &str, span: Span) -> Result<Quant, Error> {
        let Some(result) = self.get(name) else {
            return Err(Error {
                kind: ErrorKind::UnknownName {
                    name: name.to_owned(),
                    suggestion: if self.suggest {
                        // One name: all of them may be compared.
                        self.nearest(name, &mut self.entries.len())
                    } else {
                        None
                    },
                },
                span,
            });
        };
        match result {
            Ok(resolved) => Ok(Quant {
                value: resolved.value,
                dim: match resolved.quantity {
                    Quantity::Length => Dim::LENGTH,
                    Quantity::Angle => Dim::ANGLE,
                    Quantity::Number => Dim::NONE,
                },
                bare: false,
            }),
            Err(_) => Err(Error {
                kind: ErrorKind::BrokenParam(name.to_owned()),
                span,
            }),
        }
    }

    /// The name most like `name`, for a misspelling: the same but for
    /// case, or at most two edits from it (and fewer than its length).
    /// Each name compared takes one from `budget`; none are once it's
    /// spent. Names whose lengths differ by more than the edits allowed
    /// aren't compared, and those compared stop once past them.
    fn nearest(&self, name: &str, budget: &mut usize) -> Option<String> {
        if name.len() > MAX_NAME_LEN {
            return None;
        }
        let most = 2.min(name.len().saturating_sub(1));
        let mut best: Option<(usize, &str)> = None;
        for entry in &self.entries {
            if entry.name.len().abs_diff(name.len()) > most {
                continue;
            }
            let Some(left) = budget.checked_sub(1) else {
                break;
            };
            *budget = left;
            let distance = if entry.name.eq_ignore_ascii_case(name) {
                Some(0)
            } else {
                edits_within(&entry.name, name, most)
            };
            if let Some(distance) = distance
                && best.is_none_or(|(least, _)| distance < least)
            {
                best = Some((distance, &entry.name));
                if distance == 0 {
                    break;
                }
            }
        }
        best.map(|(_, name)| name.to_owned())
    }
}

/// The edit distance between two words, by bytes (names are ASCII), if
/// it's at most `most`: only the band of the table within `most` of its
/// diagonal is filled, and it stops once a row is all past `most`.
fn edits_within(a: &str, b: &str, most: usize) -> Option<usize> {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len().abs_diff(b.len()) > most {
        return None;
    }
    let past = most + 1;
    // Row `i` of the table, cells outside the band held at `past`.
    let mut row: Vec<usize> = (0..=b.len()).map(|j| j.min(past)).collect();
    for (i, &x) in a.iter().enumerate() {
        let low = (i + 1).saturating_sub(most);
        let high = (i + 1 + most).min(b.len());
        let mut diagonal = row[low.saturating_sub(1)];
        if low == 0 {
            diagonal = row[0];
            row[0] = (i + 1).min(past);
        } else {
            // Left of the band: out of reach.
            row[low - 1] = past;
        }
        let mut least = if low == 0 { row[0] } else { past };
        for j in low.max(1)..=high {
            let above = row[j];
            let cell = (above + 1)
                .min(row[j - 1] + 1)
                .min(diagonal + usize::from(x != b[j - 1]))
                .min(past);
            row[j] = cell;
            least = least.min(cell);
            diagonal = above;
        }
        if least > most {
            return None;
        }
    }
    (row[b.len()] <= most).then_some(row[b.len()])
}

/// Why a parameter's name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NameError {
    Empty,
    /// Over [`MAX_NAME_LEN`] bytes.
    TooLong,
    /// Not a letter or `_` and then letters, digits and `_`.
    NotAWord,
    /// A unit's name, such as `mm` or `deg`, in any case.
    Unit,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::Empty => write!(f, "enter a name"),
            NameError::TooLong => write!(f, "too long, at most {MAX_NAME_LEN} characters"),
            NameError::NotAWord => write!(
                f,
                "use letters, digits and '_', starting with a letter or '_'"
            ),
            NameError::Unit => write!(f, "that's the name of a unit"),
        }
    }
}

impl std::error::Error for NameError {}

/// Checks `name` can name a parameter: a word of ASCII letters, digits
/// and `_`, not starting with a digit, at most [`MAX_NAME_LEN`] bytes, and
/// not a unit's name in any case.
pub fn check_name(name: &str) -> Result<(), NameError> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(NameError::Empty);
    };
    if name.len() > MAX_NAME_LEN {
        return Err(NameError::TooLong);
    }
    if !(first.is_ascii_alphabetic() || first == '_') || !chars.all(word_char) {
        return Err(NameError::NotAWord);
    }
    if unit(name).is_some() {
        return Err(NameError::Unit);
    }
    Ok(())
}

/// The spans of the parameter names `text` uses, in order, including
/// those past a part that doesn't lex.
pub fn names(text: &str) -> Vec<Span> {
    name_spans(text)
}

/// Whether `text` uses the parameter `name`.
pub fn uses(text: &str, name: &str) -> bool {
    names(text)
        .into_iter()
        .any(|span| &text[span.range()] == name)
}

/// `text` with each use of the parameter `old` naming `new` instead.
/// Refused as [`ErrorKind::TooLong`] where that takes it over
/// [`MAX_LEN`].
pub fn rename(text: &str, old: &str, new: &str) -> Result<String, Error> {
    let mut renamed = String::with_capacity(text.len());
    let mut from = 0;
    for span in names(text) {
        if &text[span.range()] == old {
            renamed.push_str(&text[from..span.start]);
            renamed.push_str(new);
            from = span.end;
            if renamed.len() > MAX_LEN {
                break;
            }
        }
    }
    renamed.push_str(&text[from..]);
    if renamed.len() > MAX_LEN {
        return Err(Error {
            kind: ErrorKind::TooLong,
            span: Span::new(0, text.len()),
        });
    }
    Ok(renamed)
}

#[cfg(test)]
mod tests;
