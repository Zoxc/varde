use std::sync::Arc;

use varde_document::Document;

use super::*;

fn regenerate(generation: u64) -> Request {
    Request::Regenerate {
        generation: generation.into(),
        document: Arc::new(Document::default()),
    }
}

/// What `next` says to do: `Some(Some(generation))` to post that generation,
/// `Some(None)` to start a worker, `None` for nothing.
fn act(next: Next) -> Option<Option<u64>> {
    match next {
        Next::Nothing => None,
        Next::Post(request) => Some(Some(request.generation().into())),
        Next::Start => Some(None),
    }
}

const NOTHING: Option<Option<u64>> = None;
const START: Option<Option<u64>> = Some(None);

fn post(generation: u64) -> Option<Option<u64>> {
    Some(Some(generation))
}

/// A mailbox whose first worker is ready.
fn ready() -> Mailbox {
    let mut mailbox = Mailbox::default();
    assert_eq!(act(mailbox.ready()), NOTHING);
    mailbox
}

#[test]
fn requests_wait_until_the_worker_is_ready() {
    let mut mailbox = Mailbox::default();
    assert_eq!(act(mailbox.send(regenerate(0))), NOTHING);
    assert_eq!(act(mailbox.ready()), post(0));
    assert_eq!(act(mailbox.done()), NOTHING);
    assert_eq!(act(mailbox.send(regenerate(1))), post(1));
}

#[test]
fn burst_while_busy_ends_with_the_newest() {
    let mut mailbox = ready();
    assert_eq!(act(mailbox.send(regenerate(0))), post(0));
    for r in 1..=5 {
        assert_eq!(act(mailbox.send(regenerate(r))), NOTHING);
    }
    assert_eq!(act(mailbox.done()), post(5));
    assert_eq!(act(mailbox.done()), NOTHING);
}

#[test]
fn older_request_does_not_replace_a_newer_pending_one() {
    let mut mailbox = Mailbox::default();
    mailbox.send(regenerate(3));
    mailbox.send(regenerate(2));
    assert_eq!(act(mailbox.ready()), post(3));
}

#[test]
fn ready_and_done_out_of_turn_are_ignored() {
    let mut mailbox = ready();
    assert_eq!(act(mailbox.send(regenerate(1))), post(1));
    mailbox.send(regenerate(2));
    // Neither hands the busy worker another request.
    assert_eq!(act(mailbox.ready()), NOTHING);
    assert_eq!(act(mailbox.done()), post(2));
    assert_eq!(act(mailbox.done()), NOTHING);
    assert_eq!(act(mailbox.done()), NOTHING);
}

#[test]
fn fail_reports_the_request_being_worked_on() {
    let mut mailbox = ready();
    mailbox.send(regenerate(4));
    let (failed, next) = mailbox.fail();
    assert_eq!((failed.map(u64::from), act(next)), (Some(4), NOTHING));

    // No worker is left: the next request starts one, and waits for it.
    assert_eq!(act(mailbox.send(regenerate(5))), START);
    assert_eq!(act(mailbox.send(regenerate(6))), NOTHING);
    assert_eq!(act(mailbox.ready()), post(6));
}

#[test]
fn a_waiting_request_outlives_a_worker_that_dies_on_another() {
    let mut mailbox = ready();
    mailbox.send(regenerate(5));
    mailbox.send(regenerate(6));
    // 5 killed the worker; 6 never reached it, so a new one starts for it.
    let (failed, next) = mailbox.fail();
    assert_eq!((failed.map(u64::from), act(next)), (Some(5), START));
    assert_eq!(act(mailbox.send(regenerate(7))), NOTHING);
    assert_eq!(act(mailbox.ready()), post(7));
}

#[test]
fn a_worker_that_never_started_fails_what_waits_for_it() {
    let mut mailbox = Mailbox::default();
    mailbox.send(regenerate(2));
    // Another worker would likely not start either, so 2 isn't kept for it.
    let (failed, next) = mailbox.fail();
    assert_eq!((failed.map(u64::from), act(next)), (Some(2), NOTHING));

    // Nor after a worker died on a request, and the one started for the
    // request waiting didn't load.
    assert_eq!(act(mailbox.send(regenerate(3))), START);
    assert_eq!(act(mailbox.ready()), post(3));
    mailbox.send(regenerate(4));
    let (failed, next) = mailbox.fail();
    assert_eq!((failed.map(u64::from), act(next)), (Some(3), START));
    let (failed, next) = mailbox.fail();
    assert_eq!((failed.map(u64::from), act(next)), (Some(4), NOTHING));
}

#[test]
fn a_worker_that_fails_to_start_is_not_started_again() {
    // However often it fails, a worker that isn't ready never has the
    // mailbox start another, so failing to start can't loop.
    let mut mailbox = Mailbox::default();
    for generation in 0..3 {
        let (_, next) = mailbox.fail();
        assert_eq!(act(next), NOTHING);
        assert_eq!(act(mailbox.send(regenerate(generation))), START);
        let (failed, next) = mailbox.fail();
        assert_eq!(
            (failed.map(u64::from), act(next)),
            (Some(generation), NOTHING)
        );
    }
}

#[test]
fn an_idle_worker_owes_nothing() {
    let mut mailbox = ready();
    mailbox.send(regenerate(1));
    mailbox.done();
    let (failed, next) = mailbox.fail();
    assert_eq!((failed.map(u64::from), act(next)), (None, NOTHING));
    assert_eq!(act(mailbox.send(regenerate(2))), START);
    assert_eq!(act(mailbox.ready()), post(2));
}

#[test]
fn older_request_than_the_one_with_the_worker_is_dropped() {
    let mut mailbox = ready();
    assert_eq!(act(mailbox.send(regenerate(5))), post(5));
    assert_eq!(act(mailbox.send(regenerate(3))), NOTHING);
    assert_eq!(act(mailbox.done()), NOTHING);
}
