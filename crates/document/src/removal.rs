//! What removing a feature or a body takes with it: [`Document::removal`].

use std::collections::BTreeSet;

use crate::{BodyId, Document, FeatureId, Operation};

/// A feature or a body to remove, see [`Document::removal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removable {
    Feature(FeatureId),
    Body(BodyId),
}

/// Everything a removal takes: the features and the bodies they make,
/// each sorted by id. Empty if what was asked
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
    /// goes with every later feature that uses it ([`FeatureKind::uses`])
    /// or names a body it makes ([`FeatureKind::bodies`]: a combine's
    /// target or tool), directly or through others, and the bodies those
    /// make; a body goes with the feature that makes it, and so the same. A sketch on a face
    /// of a body that goes stays ([`Plane::Face`]), naming a body that
    /// isn't there. Bodies other
    /// features only exclude don't hold them back: they're dropped from
    /// the lists ([`Command::RemoveFeature`] and [`Command::RemoveBody`]
    /// apply exactly this).
    ///
    /// [`FeatureKind::uses`]: crate::FeatureKind::uses
    /// [`FeatureKind::bodies`]: crate::FeatureKind::bodies
    /// [`Plane::Face`]: crate::Plane::Face
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
        // all.
        let mut features = BTreeSet::from([feature]);
        for later in &self.features[first + 1..] {
            let uses = later.kind.uses().iter().any(|used| features.contains(used));
            let names = (later.kind.bodies().into_iter())
                .filter_map(|body| self.body(body))
                .any(|body| features.contains(&body.created_by));
            if uses || names {
                features.insert(later.id);
            }
        }
        let features: Vec<FeatureId> = features.into_iter().collect();
        let bodies = self
            .bodies
            .iter()
            .filter(|body| features.binary_search(&body.created_by).is_ok())
            .map(|body| body.id)
            .collect();
        Removal { features, bodies }
    }

    /// What removing all of `targets` takes with it: each one's
    /// [`Document::removal`], together.
    pub fn removal_of(&self, targets: &[Removable]) -> Removal {
        let mut all = Removal::default();
        for &target in targets {
            let removal = self.removal(target);
            all.features.extend(removal.features);
            all.bodies.extend(removal.bodies);
        }
        // Sorted for searching, as one removal's are.
        all.features.sort_unstable();
        all.features.dedup();
        all.bodies.sort_unstable();
        all.bodies.dedup();
        all
    }

    /// What removing just `targets` takes, changing nothing: each
    /// feature, a body's maker for a body (a pattern with all its copy
    /// bodies for one of them), and the bodies those make. The later
    /// features [`Document::removal`] would take stay, naming a sketch or
    /// a body that isn't there, which regenerating fails until they're
    /// given another ([`Command::RemoveOnly`] applies exactly this).
    ///
    /// [`Command::RemoveOnly`]: crate::Command::RemoveOnly
    pub fn breaking_removal(&self, targets: &[Removable]) -> Removal {
        let mut features: Vec<FeatureId> = (targets.iter())
            .filter_map(|&target| match target {
                Removable::Feature(id) => self.feature(id).map(|feature| feature.id),
                Removable::Body(id) => self.body(id).map(|body| body.created_by),
            })
            .collect();
        features.sort_unstable();
        features.dedup();
        let bodies = (self.bodies.iter())
            .filter(|body| features.binary_search(&body.created_by).is_ok())
            .map(|body| body.id)
            .collect();
        Removal { features, bodies }
    }

    /// Removes what `removal` lists, and drops its bodies from the other
    /// features' excluded lists.
    pub(crate) fn remove(&mut self, removal: &Removal) {
        // Rolled back to before a feature removed: to before the next one
        // kept, or to the end.
        if let Some(rollback) = self.rollback {
            let at = self.feature_index(rollback).unwrap_or(self.features.len());
            self.rollback = (self.features[at..].iter())
                .map(|feature| feature.id)
                .find(|id| removal.features.binary_search(id).is_err());
        }
        self.features
            .retain(|feature| removal.features.binary_search(&feature.id).is_err());
        self.reindex();
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
