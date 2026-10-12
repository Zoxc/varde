use std::sync::Arc;

use varde_document::Document;

use super::*;

fn regenerate(generation: u64) -> Request {
    Request::Regenerate {
        sight: None,
        generation: generation.into(),
        document: Arc::new(Document::default()),
        exclude: None,
        until: None,
        draft: None,
        inspect: None,
    }
}

fn generation(request: Option<Request>) -> Option<u64> {
    request.as_ref().map(|request| {
        request
            .asked()
            .map(|asked| asked.generation)
            .expect("a regeneration")
            .into()
    })
}

fn export(export: u64) -> Request {
    Request::Export {
        export,
        document: Arc::new(Document::default()),
    }
}

/// The tag of `request`, an export.
fn exported(request: Option<Request>) -> Option<u64> {
    match request? {
        Request::Export { export, .. } => Some(export),
        Request::Regenerate { .. } | Request::Query { .. } => {
            panic!("not an export")
        }
    }
}

#[test]
fn waiting_request_is_replaced_by_a_newer_one() {
    let mut newest = Newest::default();
    assert_eq!(generation(newest.push(regenerate(0))), None);
    assert_eq!(generation(newest.push(regenerate(1))), Some(0));
    assert_eq!(generation(newest.pop()), Some(1));
    assert!(newest.is_empty());
}

#[test]
fn older_request_does_not_replace_a_newer_waiting_one() {
    // What two clones sending at once can leave behind: the newer request
    // lands in the slot first, then the older one.
    let mut newest = Newest::default();
    assert_eq!(generation(newest.push(regenerate(1))), None);
    assert_eq!(generation(newest.push(regenerate(0))), Some(0));
    assert_eq!(generation(newest.pop()), Some(1));
}

#[test]
fn request_older_than_one_taken_is_refused() {
    let mut newest = Newest::default();
    assert_eq!(generation(newest.push(regenerate(1))), None);
    assert_eq!(generation(newest.pop()), Some(1));
    // The lane is working on the newer one: the older one must not follow.
    assert_eq!(generation(newest.push(regenerate(0))), Some(0));
    assert!(newest.is_empty());
}

#[test]
fn exports_wait_in_order_and_no_regeneration_replaces_them() {
    let mut newest = Newest::default();
    assert!(newest.push(regenerate(0)).is_none());
    assert!(newest.push(export(7)).is_none());
    assert_eq!(generation(newest.push(regenerate(1))), Some(0));
    assert!(newest.push(export(8)).is_none());
    // A burst of regenerations after them replaces only the regeneration.
    assert_eq!(generation(newest.push(regenerate(2))), Some(1));
    assert_eq!(exported(newest.pop()), Some(7));
    assert_eq!(exported(newest.pop()), Some(8));
    assert_eq!(generation(newest.pop()), Some(2));
    assert!(newest.is_empty());
    // A regeneration taken refuses an older one, not an export.
    assert!(newest.push(export(9)).is_none());
    assert_eq!(generation(newest.push(regenerate(1))), Some(1));
    assert_eq!(exported(newest.pop()), Some(9));
    assert!(newest.is_empty());
}

/// A regeneration of `generation` measuring with revision `revision`.
fn measuring(generation: u64, revision: u64) -> Request {
    let mut request = regenerate(generation);
    if let Request::Regenerate { inspect, .. } = &mut request {
        *inspect = Some(Box::new(crate::Inspect {
            revision,
            first: crate::InspectPick {
                body: varde_document::BodyId::NEW,
                entity: crate::Entity::Body,
                near: [0.0; 3],
            },
            second: None,
        }));
    }
    request
}

/// Measures ride on the regeneration: a newer pick of the same
/// generation replaces the one waiting, so the newest picks are always
/// what's answered next, and one of an older generation never follows.
#[test]
fn a_newer_measure_replaces_the_one_waiting() {
    let mut newest = Newest::default();
    assert!(newest.push(measuring(3, 1)).is_none());
    let replaced = newest.push(measuring(3, 2)).unwrap();
    assert_eq!(replaced.asked().and_then(|asked| asked.inspect), Some(1));
    // An older generation's, picked on a model the app no longer shows.
    let refused = newest.push(measuring(2, 3)).unwrap();
    assert_eq!(refused.asked().and_then(|asked| asked.inspect), Some(3));
    let next = newest.pop().unwrap();
    assert_eq!(
        (
            generation(Some(next.clone())),
            next.asked().and_then(|asked| asked.inspect)
        ),
        (Some(3), Some(2))
    );
    // An export waiting doesn't drop the measure behind it.
    newest.push(measuring(4, 4));
    newest.push(export(1));
    assert_eq!(exported(newest.pop()), Some(1));
    assert_eq!(
        newest
            .pop()
            .unwrap()
            .asked()
            .and_then(|asked| asked.inspect),
        Some(4)
    );
}

fn topology(tag: u64) -> Request {
    Request::Query {
        tag,
        generation: 0.into(),
        document: Arc::new(Document::default()),
        until: None,
        query: crate::Query::Topology(varde_document::BodyId::NEW),
    }
}

/// Edge facts wait in order among exports, ahead of the regeneration
/// waiting, and neither replaces nor is replaced.
#[test]
fn edge_facts_queue_in_order_with_exports() {
    let mut newest = Newest::default();
    assert!(newest.push(topology(1)).is_none());
    assert!(newest.push(regenerate(3)).is_none());
    assert!(newest.push(export(2)).is_none());
    assert!(newest.push(topology(4)).is_none());
    assert_eq!(generation(newest.push(regenerate(5))), Some(3));
    let tag = |request: Option<Request>| match request {
        Some(Request::Query { tag, .. }) => tag,
        other => panic!("{other:?} isn't edge facts"),
    };
    assert_eq!(tag(newest.pop()), 1);
    assert_eq!(exported(newest.pop()), Some(2));
    assert_eq!(tag(newest.pop()), 4);
    assert_eq!(generation(newest.pop()), Some(5));
    assert!(newest.is_empty());
}

/// A client's measure of tag `tag`.
fn client_measure(tag: u64) -> Request {
    let Request::Regenerate { inspect, .. } = measuring(0, 1) else {
        unreachable!()
    };
    Request::Query {
        tag,
        generation: 0.into(),
        document: Arc::new(Document::default()),
        until: None,
        query: crate::Query::Measure(inspect.unwrap()),
    }
}

/// A client's measure waits in order among exports and edge facts,
/// ahead of the regeneration waiting: the user's measure riding on that
/// regeneration doesn't replace it, nor it the user's.
#[test]
fn client_measures_queue_in_order_beside_the_user_s() {
    let mut newest = Newest::default();
    assert!(newest.push(measuring(3, 1)).is_none());
    assert!(newest.push(client_measure(1)).is_none());
    assert!(newest.push(export(2)).is_none());
    assert!(newest.push(client_measure(3)).is_none());
    assert!(newest.push(topology(4)).is_none());
    // The user's newer picks replace only the user's.
    assert_eq!(
        newest
            .push(measuring(3, 2))
            .unwrap()
            .asked()
            .and_then(|asked| asked.inspect),
        Some(1)
    );
    let tag = |request: Option<Request>| match request {
        Some(Request::Query { tag, .. }) => tag,
        other => panic!("{other:?} isn't a measure"),
    };
    assert_eq!(tag(newest.pop()), 1);
    assert_eq!(exported(newest.pop()), Some(2));
    assert_eq!(tag(newest.pop()), 3);
    assert_eq!(tag(newest.pop()), 4);
    assert_eq!(
        newest
            .pop()
            .unwrap()
            .asked()
            .and_then(|asked| asked.inspect),
        Some(2)
    );
    assert!(newest.is_empty());
}
