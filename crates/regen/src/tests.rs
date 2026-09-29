use varde_document::{Document, Editor};

use super::*;

fn regenerate(editor: &Editor) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
    }
}

#[test]
fn regenerate_tessellates_the_snapshot() {
    let editor = Editor::new(Document::example());
    let Response::Regenerated { generation, mesh } = handle(regenerate(&editor)) else {
        panic!("regeneration failed");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(*mesh, crate::tessellate(editor.document()).unwrap());
    assert_eq!(mesh.triangle_count(), 12);
}
