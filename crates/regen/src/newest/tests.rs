use std::sync::Arc;

use varde_document::Document;

use super::*;

fn regenerate(generation: u64) -> Request {
    Request::Regenerate {
        generation: generation.into(),
        document: Arc::new(Document::default()),
    }
}

fn generation(request: Option<Request>) -> Option<u64> {
    request.as_ref().map(|request| request.generation().into())
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
