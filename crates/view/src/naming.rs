//! Naming the faces, edges and corners of the model shown as a feature
//! of the document stores them, with the history stopped at the feature:
//! the picks of the model shown, through regen's [`Naming`].

use glam::DVec3;
use varde_document::{EdgeRef, FaceRef, PointRef};
use varde_regen::{Naming, Unnamed};

use crate::pick::PickIndex;

/// [`Naming`] of what's picked in the model shown: its faces, edges and
/// corners as a feature stores them.
pub trait NamingPicks {
    /// The reference to face `face` of `index`'s model picked at `near`,
    /// as a feature stores it. The model shows it on the body holding it
    /// at the end of the history, but a join after the feature may have
    /// merged the body it's on there into that one: it's named by the
    /// body it was made on, which regenerating follows on to the body
    /// holding it wherever the feature is ([`Naming::face_body`]). None
    /// if there's no such face, or if which body it's on there can't be
    /// told.
    fn face_ref(&self, index: &PickIndex, face: u32, near: DVec3) -> Option<FaceRef>;

    /// The reference to face `face` of `index`'s model picked at `near`,
    /// as a feature stores it ([`NamingPicks::face_ref`]), refused as
    /// [`NamingPicks::edge_ref`] refuses an edge: not in the model's
    /// tables ([`Unnamed::Missing`]), or as
    /// [`Naming::checked_face_body`] refuses it.
    fn checked_face_ref(
        &self,
        index: &PickIndex,
        face: u32,
        near: DVec3,
    ) -> Result<FaceRef, Unnamed>;

    /// The reference to edge `edge` of `index`'s model picked at `near`,
    /// as a feature stores it ([`EdgeRef`]): the keys of its two faces,
    /// sorted, and the body its first face is on where the history stops.
    /// Refused if it isn't an edge between two faces of the model
    /// ([`Unnamed::Missing`]), or as [`Naming::edge_body`] refuses it.
    fn edge_ref(&self, index: &PickIndex, edge: u32, near: DVec3) -> Result<EdgeRef, Unnamed>;

    /// The reference to corner `corner` of `index`'s model (an index into
    /// its [`varde_regen::Picking::corners`]) as an align stores it: the
    /// keys of three faces meeting there, sorted, the body the first is
    /// on where the history stops, and the corner's point. Refused if
    /// it isn't in the model's tables ([`Unnamed::Missing`]), or as
    /// [`Naming::corner_body`] refuses it.
    fn corner_ref(&self, index: &PickIndex, corner: u32) -> Result<PointRef, Unnamed>;
}

impl NamingPicks for Naming {
    fn face_ref(&self, index: &PickIndex, face: u32, near: DVec3) -> Option<FaceRef> {
        let mut found = index.face_ref(face, near)?;
        found.body = self.face_body(found.body, &found.key)?;
        Some(found)
    }

    fn checked_face_ref(
        &self,
        index: &PickIndex,
        face: u32,
        near: DVec3,
    ) -> Result<FaceRef, Unnamed> {
        let mut found = index.face_ref(face, near).ok_or(Unnamed::Missing)?;
        found.body = self.checked_face_body(found.body, &found.key)?;
        Ok(found)
    }

    fn edge_ref(&self, index: &PickIndex, edge: u32, near: DVec3) -> Result<EdgeRef, Unnamed> {
        let faces = index.edge_faces(edge).ok_or(Unnamed::Missing)?;
        let shown = index.face_body(faces[0]).ok_or(Unnamed::Missing)?;
        let keys = index.chain_keys(edge).ok_or(Unnamed::Missing)?;
        let body = self.edge_body(shown, &keys)?;
        Ok(EdgeRef {
            body,
            faces: keys,
            near,
        })
    }

    fn corner_ref(&self, index: &PickIndex, corner: u32) -> Result<PointRef, Unnamed> {
        let picking = index.picking();
        let found = picking
            .corners()
            .get(corner as usize)
            .ok_or(Unnamed::Missing)?;
        let shown = index.face_body(found.faces[0]).ok_or(Unnamed::Missing)?;
        let keys = picking.corner_keys(corner).ok_or(Unnamed::Missing)?;
        let body = self.corner_body(shown, &keys)?;
        Ok(PointRef::Corner {
            body,
            faces: keys,
            near: DVec3::from(found.point),
        })
    }
}
