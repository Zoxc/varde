//! What removing a feature or a body takes with it: [`Document::removal`].

use crate::{BodyId, Document, FeatureId, Operation};

/// A feature or a body to remove, see [`Document::removal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removable {
    Feature(FeatureId),
    Body(BodyId),
}

/// Everything a removal takes: the features, in the timeline's order, and
/// the bodies they make, in the bodies' order. Empty if what was asked
/// for isn't there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Removal {
    pub features: Vec<FeatureId>,
    pub bodies: Vec<BodyId>,
}

impl Removal {
    pub fn is_empty(&self) -> bool {
        self.features.is_empty() && self.bodies.is_empty()
    }
}

impl Document {
    /// What removing `target` takes with it, changing nothing: a feature
    /// goes with every later feature that uses it ([`FeatureKind::uses`]),
    /// directly or through others, and the bodies those make; a body goes
    /// with the feature that makes it, and so the same. Bodies other
    /// features only exclude don't hold them back: they're dropped from
    /// the lists ([`Command::RemoveFeature`] and [`Command::RemoveBody`]
    /// apply exactly this).
    ///
    /// [`FeatureKind::uses`]: crate::FeatureKind::uses
    /// [`Command::RemoveFeature`]: crate::Command::RemoveFeature
    /// [`Command::RemoveBody`]: crate::Command::RemoveBody
    pub fn removal(&self, target: Removable) -> Removal {
        let feature = match target {
            Removable::Feature(feature) => feature,
            Removable::Body(body) => match self.body(body) {
                Some(body) => body.created_by,
                None => return Removal::default(),
            },
        };
        let Some(first) = self.feature_index(feature) else {
            return Removal::default();
        };
        // Features only use earlier ones, so one pass in order finds them
        // all, and the list stays sorted by id for searching.
        let mut features = vec![feature];
        for later in &self.features[first + 1..] {
            if later
                .kind
                .uses()
                .iter()
                .any(|used| features.binary_search(used).is_ok())
            {
                features.push(later.id);
            }
        }
        let bodies = self
            .bodies
            .iter()
            .filter(|body| features.binary_search(&body.created_by).is_ok())
            .map(|body| body.id)
            .collect();
        Removal { features, bodies }
    }

    /// Removes what `removal` lists, and drops its bodies from the other
    /// features' excluded lists.
    pub(crate) fn remove(&mut self, removal: &Removal) {
        self.features
            .retain(|feature| removal.features.binary_search(&feature.id).is_err());
        self.bodies
            .retain(|body| removal.bodies.binary_search(&body.id).is_err());
        self.drop_excluded(&removal.bodies);
    }

    /// Drops `bodies`, sorted, from every feature's excluded list.
    pub(crate) fn drop_excluded(&mut self, bodies: &[BodyId]) {
        for feature in &mut self.features {
            if let Some(excluded) = feature
                .kind
                .operation_mut()
                .and_then(Operation::excluded_mut)
            {
                excluded.retain(|body| bodies.binary_search(body).is_err());
            }
        }
    }
}
