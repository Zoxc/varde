//! Renaming features (sketches among them) and bodies, keeping names
//! apart: a name another has gets a number after it, "Name (1)".

use crate::{BodyId, Command, Document, FeatureId, MAX_NAME_LEN};

/// What can be renamed: a feature, a sketch among them, or a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Named {
    Feature(FeatureId),
    Body(BodyId),
}

/// A rename [`Document::rename`] makes.
#[derive(Debug, Clone, PartialEq)]
pub struct Rename {
    /// The command giving the name.
    pub command: Command,
    /// The name asked for, if another feature or body had it, so the
    /// command gives one with a number after it.
    pub taken: Option<String>,
}

/// The room a number after a name takes at most: " (" , the digits of a
/// `u64`, ")".
const SUFFIX_ROOM: usize = 2 + 20 + 1;

impl Document {
    /// The name of `target`, if it's there.
    pub fn name_of(&self, target: Named) -> Option<&str> {
        match target {
            Named::Feature(id) => self.feature(id).map(|feature| feature.name.as_str()),
            Named::Body(id) => self.body(id).map(|body| body.name.as_str()),
        }
    }

    pub(crate) fn name_mut(&mut self, target: Named) -> Option<&mut String> {
        match target {
            Named::Feature(id) => {
                let index = self.feature_index(id)?;
                Some(&mut self.features[index].name)
            }
            Named::Body(id) => {
                let index = self.body_index(id)?;
                Some(&mut self.bodies[index].name)
            }
        }
    }

    /// Whether a feature or body other than `target` is named `name`.
    fn name_taken(&self, target: Named, name: &str) -> bool {
        let features = (self.features.iter())
            .filter(|feature| target != Named::Feature(feature.id))
            .map(|feature| feature.name.as_str());
        let bodies = (self.bodies.iter())
            .filter(|body| target != Named::Body(body.id))
            .map(|body| body.name.as_str());
        features.chain(bodies).any(|other| other == name)
    }

    /// The command naming `target` `wanted`, trimmed and cut to
    /// [`MAX_NAME_LEN`], or `None` if it's not there, the name is empty or
    /// it's the one `target` has. Names are kept apart across features
    /// and bodies: if another has the name, `target` gets the first free
    /// "name (N)" from 1 on, a number already ending the name asked for
    /// taken off first ("Name (1)" taken gives "Name (2)"), and the
    /// rename says so.
    pub fn rename(&self, target: Named, wanted: &str) -> Option<Rename> {
        let old = self.name_of(target)?;
        let wanted = cut(wanted.trim(), MAX_NAME_LEN);
        if wanted.is_empty() || wanted == old {
            return None;
        }
        if !self.name_taken(target, wanted) {
            return Some(Rename {
                command: Command::Rename {
                    target,
                    name: wanted.to_owned(),
                },
                taken: None,
            });
        }
        let base = cut(strip_number(wanted), MAX_NAME_LEN - SUFFIX_ROOM).trim_end();
        // Among one more number than there are names, one is free.
        let names = self.features.len() + self.bodies.len();
        let name = (1..=names.saturating_add(1))
            .map(|n| format!("{base} ({n})"))
            .find(|name| !self.name_taken(target, name))?;
        Some(Rename {
            command: Command::Rename { target, name },
            taken: Some(wanted.to_owned()),
        })
    }
}

/// `name` without a " (N)" ending it, if one does.
fn strip_number(name: &str) -> &str {
    name.strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
        .filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .map_or(name, |(base, _)| base)
}

/// `name` cut to at most `len` bytes, at a character's boundary.
fn cut(name: &str, len: usize) -> &str {
    if name.len() <= len {
        return name;
    }
    let end = (0..=len)
        .rev()
        .find(|&i| name.is_char_boundary(i))
        .unwrap_or(0);
    &name[..end]
}

#[cfg(test)]
mod tests;
