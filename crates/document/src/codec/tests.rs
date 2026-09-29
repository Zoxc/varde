use std::error::Error;

use super::*;
use crate::testing::with_body;

#[test]
fn a_document_round_trips() {
    let document = with_body();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// A payload that begins like a document, as an auto-save of a new design
/// does, isn't taken for the empty document its first bytes decode as.
#[test]
fn bytes_after_a_document_are_refused() {
    // Laid out as the IO lane's auto-saves: no base (the tail of a design
    // file), no name, the document and whether it was downloaded.
    let auto_saved = (None::<(u64, u32, u64)>, None::<String>, with_body(), false);
    assert!(matches!(
        Document::from_postcard(&postcard::to_stdvec(&auto_saved).unwrap()),
        Err(error) if error.source().is_none() && error.to_string().contains("after the end")
    ));
    let mut padded = with_body().to_postcard();
    padded.push(0);
    assert!(Document::from_postcard(&padded).is_err());
}

#[test]
fn malformed_bytes_are_refused() {
    assert!(Document::from_postcard(&[]).is_err());
    assert!(Document::from_postcard(&[0xff; 16]).is_err());
}

/// Deserializing a document any way checks it, not only through
/// [`Document::from_postcard`], which keeps the message of what's wrong.
#[test]
fn deserializing_a_document_checks_it() {
    let mut bytes = with_body().to_postcard();
    // The next id, now 0, which the body's id 1 isn't below.
    *bytes.last_mut().unwrap() = 0;
    assert!(postcard::from_bytes::<Document>(&bytes).is_err());
    assert!(matches!(
        Document::from_postcard(&bytes),
        Err(error) if matches!(
            error.source().and_then(|why| why.downcast_ref()),
            Some(CheckError::NextId(_))
        )
            && error.to_string().contains("not below the next id")
    ));
}

/// Checking a document as it's deserialized doesn't change its bytes.
#[test]
fn a_document_encodes_as_before() {
    let mut editor = crate::Editor::new(Document::default());
    let xy = crate::Plane::Origin(crate::OriginPlane::XY);
    editor.apply(editor.document().add_sketch(xy)).unwrap();
    assert_eq!(
        editor.document().to_postcard(),
        [
            // No bodies.
            0,
            // The feature: id 0, "Sketch 1", visible, a sketch on XY,
            // empty.
            1, 0, 8, 83, 107, 101, 116, 99, 104, 32, 49, 1, 0, 0, 0, 0, 0, 0, 0, 0,
            // Millimetres, the tolerance, 0.001 as an f64, and the next
            // id.
            0, 0xfc, 0xa9, 0xf1, 0xd2, 0x4d, 0x62, 0x50, 0x3f, 1
        ]
    );
}
