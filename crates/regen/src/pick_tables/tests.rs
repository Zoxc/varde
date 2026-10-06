use std::sync::Arc;

use varde_document::{Document, Editor};

use super::*;
use crate::tests::regenerate_with;
use crate::{Response, handle};

/// The example plate's mesh, its picking tables and its hierarchies, as
/// the lane answers them.
fn plate() -> (Arc<RenderMesh>, Arc<Picking>, Arc<PickTables>) {
    let editor = Editor::new(Document::example());
    let Response::Regenerated {
        mesh,
        picking,
        tables,
        ..
    } = handle(regenerate_with(&editor, None))
    else {
        panic!("the plate regenerates");
    };
    (mesh, picking, tables)
}

#[test]
fn the_lane_builds_the_tables_and_the_upload() {
    let (mesh, picking, tables) = plate();
    assert_eq!(*tables, PickTables::new(&mesh, &picking));
    assert!(!tables.triangles().nodes().is_empty());
    assert!(!tables.segments().nodes().is_empty());
    assert!(!tables.vertices().nodes().is_empty());
    assert_eq!(*mesh.upload(), varde_kernel::MeshUpload::new(&mesh));
}

#[test]
fn tables_round_trip_their_parts() {
    let (mesh, _, tables) = plate();
    assert_eq!(
        PickTables::from_parts(&mesh, tables.to_parts()),
        Ok((*tables).clone())
    );
    // None at all, as for tables that don't go with their mesh.
    let none = PickTables::default();
    assert_eq!(PickTables::from_parts(&mesh, none.to_parts()), Ok(none));
}

#[test]
fn trees_that_are_not_trees_are_refused() {
    let (mesh, _, tables) = plate();
    let refused = |change: &dyn Fn(&mut PickTablesParts)| {
        let mut parts = tables.to_parts();
        change(&mut parts);
        PickTables::from_parts(&mesh, parts)
    };
    let inner = |nodes: &[BvhNode]| nodes.iter().position(|n| n.count == 0).unwrap();
    // A second child pointing back at the first: reached twice.
    let back = |parts: &mut PickTablesParts| {
        let nodes = &mut parts.triangles.0;
        let at = inner(nodes);
        nodes[at].start = at as u32 + 1;
    };
    // Past the nodes.
    let past = |parts: &mut PickTablesParts| {
        let nodes = &mut parts.segments.0;
        let at = inner(nodes);
        nodes[at].start = nodes.len() as u32;
    };
    // An inner node's second child the same as another's: a node shared,
    // which a walk would visit twice.
    let shared = |parts: &mut PickTablesParts| {
        let nodes = &mut parts.triangles.0;
        let root = nodes[0].start;
        let second = (1..nodes.len()).find(|&i| nodes[i].count == 0).unwrap();
        nodes[second].start = root;
    };
    // A leaf past the items.
    let leaf = |parts: &mut PickTablesParts| {
        let (nodes, items) = &mut parts.vertices;
        let at = nodes.iter().position(|n| n.count > 0).unwrap();
        nodes[at].start = items.len() as u32;
    };
    // An item naming a triangle the mesh hasn't.
    let item = |parts: &mut PickTablesParts| {
        parts.triangles.1[0] = mesh.triangle_count() as u32;
    };
    // More items than what it's over, or more nodes than a tree of its
    // items has.
    let items = |parts: &mut PickTablesParts| {
        parts.vertices.1.resize(mesh.corners().len() + 1, 0);
    };
    let nodes = |parts: &mut PickTablesParts| {
        let (nodes, items) = &mut parts.segments;
        let leaf = *nodes.iter().find(|n| n.count > 0).unwrap();
        nodes.resize(2 * items.len() + 1, leaf);
    };
    for change in [
        &back as &dyn Fn(&mut _),
        &past,
        &shared,
        &leaf,
        &item,
        &items,
        &nodes,
    ] {
        assert_eq!(refused(change), Err(PickTablesError::Tree));
    }
    let unsorted = |parts: &mut PickTablesParts| parts.corner_faces.swap(0, 1);
    let face = |parts: &mut PickTablesParts| parts.corner_faces.last_mut().unwrap()[1] = u32::MAX;
    for change in [&unsorted as &dyn Fn(&mut _), &face] {
        assert_eq!(refused(change), Err(PickTablesError::CornerFaces));
    }
    let short = |parts: &mut PickTablesParts| {
        parts.chain_starts.pop();
    };
    let edge = |parts: &mut PickTablesParts| parts.chain_items[0] = u32::MAX;
    let order = |parts: &mut PickTablesParts| parts.chain_starts[1] = u32::MAX;
    for change in [&short as &dyn Fn(&mut _), &edge, &order] {
        assert_eq!(refused(change), Err(PickTablesError::Chains));
    }
}
