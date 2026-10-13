//! Keeping a sketch's sketch face ([`sketch_face`]): the link to the face
//! a sketch on a face holds, as its sketch and plane change.

use varde_sketch::{LinkKind, Sketch};

use super::{Command, EditError};
use crate::{Document, FeatureId, FeatureKind, Id, LinkSource, OutsideRef, Plane, sketch_face};

impl Document {
    /// The command giving the sketch feature `feature` `sketch` whole:
    /// [`Command::SetSketch`], or, as `sketch` lacks the sketch face
    /// ([`sketch_face`]) the feature's plane has it hold, [`Command::AddLink`]
    /// with one added to it (last, making nothing until relinked), so
    /// drawing a sketch afresh keeps it.
    pub fn set_sketch_whole(&self, feature: FeatureId, mut sketch: Sketch) -> Command {
        let face = (self.feature(feature)).and_then(|found| match &found.kind {
            FeatureKind::Sketch { plane, .. } => plane.face().copied(),
            _ => None,
        });
        let held = (self.feature_index(feature)).and_then(|index| self.sketch_face_of(index));
        let lacks = held
            .is_none_or(|id| (sketch.link(id)).is_none_or(|link| link.kind != LinkKind::Project));
        if let Some(face) = face.filter(|_| lacks)
            && let Ok(link) = sketch.add_link(LinkKind::Project)
        {
            return Command::AddLink {
                feature,
                sketch: Box::new(sketch),
                source: LinkSource {
                    link,
                    source: OutsideRef::Face(face),
                },
            };
        }
        Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        }
    }

    /// The document with a feature holding an empty sketch on `plane`
    /// added, as [`Command::AddSketch`] does.
    pub(super) fn with_new_sketch(
        &self,
        name: String,
        plane: Plane,
    ) -> Result<Document, EditError> {
        let mut next = self.clone();
        let id = next.push_feature(
            name,
            FeatureKind::Sketch {
                plane,
                sketch: Sketch::default(),
                sources: Vec::new(),
            },
        )?;
        let index = next.feature_index(id).expect("just added");
        next.follow_sketch_face(index, None)?;
        Ok(next)
    }

    /// Gives the sketch feature at `index` `sketch`, keeping the sources
    /// of the links it still has, and adding `source` for its link, in
    /// its place by link (one there for the link already is replaced).
    /// The check is the caller's.
    pub(super) fn set_sketch(&mut self, index: usize, sketch: Sketch, source: Option<LinkSource>) {
        if let FeatureKind::Sketch {
            sketch: old,
            sources,
            ..
        } = &mut self.features[index].kind
        {
            sources.retain(|kept| sketch.link(kept.link).is_some());
            if let Some(source) = source {
                sources.retain(|kept| kept.link != source.link);
                let at = sources.partition_point(|kept| kept.link < source.link);
                sources.insert(at, source);
            }
            *old = sketch;
        }
    }

    /// The sketch face ([`sketch_face`]) of feature `index`, if it's a
    /// sketch on a face with one.
    pub(crate) fn sketch_face_of(&self, index: usize) -> Option<Id> {
        match &self.features.get(index)?.kind {
            FeatureKind::Sketch {
                plane,
                sketch,
                sources,
            } => sketch_face(plane, sketch, sources),
            _ => None,
        }
    }

    /// Refuses a sketch set at `index` that leaves it without the sketch
    /// face it had in `before`, as nothing removes it but leaving the face.
    pub(super) fn keeps_sketch_face(
        &self,
        before: &Document,
        index: usize,
    ) -> Result<(), EditError> {
        match before.sketch_face_of(index) {
            Some(_) if self.sketch_face_of(index).is_none() => {
                Err(EditError::SketchFace(self.features[index].id))
            }
            _ => Ok(()),
        }
    }

    /// Gives the sketch feature at `index` its sketch face
    /// ([`sketch_face`]) as its plane has it, `was` being the link that
    /// was its sketch face before its plane changed: on a face, the one
    /// there is kept, else `was` follows the new face, else a new link is
    /// added (making nothing until it's relinked: construction, out of
    /// profiles); a sketch face that's no longer one, as the sketch left
    /// its face or a link of the new face was there already, is deleted.
    /// The check is the caller's.
    pub(super) fn follow_sketch_face(
        &mut self,
        index: usize,
        was: Option<Id>,
    ) -> Result<(), EditError> {
        let feature = self.features[index].id;
        let FeatureKind::Sketch {
            plane,
            sketch,
            sources,
        } = &mut self.features[index].kind
        else {
            return Ok(());
        };
        let now = sketch_face(plane, sketch, sources);
        let drop = |sketch: &mut Sketch, sources: &mut Vec<LinkSource>, link: Id| {
            sketch.delete(&[link]);
            sources.retain(|from| from.link != link);
        };
        match (plane.face().copied(), now, was) {
            (Some(_), Some(now), Some(was)) if now != was => drop(sketch, sources, was),
            (Some(_), Some(_), _) | (None, _, None) => {}
            (Some(face), None, Some(was)) => {
                if let Some(from) = sources.iter_mut().find(|from| from.link == was) {
                    from.source = OutsideRef::Face(face);
                }
            }
            (Some(face), None, None) => {
                let link = (sketch.add_link(LinkKind::Project))
                    .map_err(|why| EditError::Sketch(feature, why.into()))?;
                sources.push(LinkSource {
                    link,
                    source: OutsideRef::Face(face),
                });
            }
            (None, _, Some(was)) => drop(sketch, sources, was),
        }
        Ok(())
    }

    /// The document with every sketch on a face given its sketch face
    /// ([`sketch_face`]) where it lacks one, as a document read is: one
    /// whose sketch has no id left stays without. Passes the check as it did.
    pub(crate) fn with_sketch_faces(mut self) -> Document {
        for index in 0..self.features.len() {
            // With no link before, the only change is the link added,
            // which leaves the sketch as it was when it fails.
            let _ = self.follow_sketch_face(index, None);
        }
        debug_assert_eq!(self.check(), Ok(()));
        self
    }
}
