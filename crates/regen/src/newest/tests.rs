use std::sync::Arc;

use varde_document::Document;

use super::*;

fn regenerate(generation: u64) -> Request {
    Request::Regenerate {
        generation: generation.into(),
        document: Arc::new(Document::default()),
        exclude: None,
        draft: None,
        inspect: None,
    }
}

fn generation(request: Option<Request>) -> Option<u64> {
    request
        .as_ref()
        .map(|request| request.generation().expect("a regeneration").into())
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
        Request::Regenerate { .. } => panic!("not an export"),
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
    assert_eq!(replaced.inspect(), Some(1));
    // An older generation's, picked on a model the app no longer shows.
    let refused = newest.push(measuring(2, 3)).unwrap();
    assert_eq!(refused.inspect(), Some(3));
    let next = newest.pop().unwrap();
    assert_eq!(
        (generation(Some(next.clone())), next.inspect()),
        (Some(3), Some(2))
    );
    // An export waiting doesn't drop the measure behind it.
    newest.push(measuring(4, 4));
    newest.push(export(1));
    assert_eq!(exported(newest.pop()), Some(1));
    assert_eq!(newest.pop().unwrap().inspect(), Some(4));
}
