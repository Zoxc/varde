//! The worker's side of the web lane, see the parent module: queues the
//! requests the page posts and handles them in order, one at a time. Run
//! by the worker's `main`, never on the page.

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use varde_lane::Pending;
use varde_lane::worker::{self, Message, Refused};

use super::disk::Handed;
use super::files::Files;
use crate::queue::{Queue, Queued};
use crate::web::message_bytes;
use crate::wire::{self, ToWorker};
use crate::{Request, Stores};

/// A request the worker was posted, with its number and what was posted
/// along with it, see `page::picked`.
#[derive(Debug)]
struct Numbered {
    seq: u64,
    request: Request,
    object: Option<Handed>,
}

impl Queued for Numbered {
    fn request(&self) -> &Request {
        &self.request
    }
}

/// Runs the worker's side: handles each request posted to it in order, one
/// at a time. Called by the worker's `main`, in the worker.
pub fn serve() {
    let (queued, woken) = unbounded();
    worker::serve("the file worker", move |Message { parts, objects }| {
        // The bytes, and the object posted with them, if any.
        let ([buffer], [] | [_]) = (&parts[..], &objects[..]) else {
            return Err(Refused::Else);
        };
        let object = match objects.into_iter().next() {
            Some(object) => Some(Handed::from_js(object).ok_or(Refused::Else)?),
            None => None,
        };
        let bytes = message_bytes(buffer)?;
        let ToWorker { seq, request } = wire::decode(&bytes)?;
        let _ = queued.unbounded_send(Numbered {
            seq,
            request,
            object,
        });
        Ok(())
    });
    wasm_bindgen_futures::spawn_local(run(woken));
}

/// Handles the requests posted, in order, as they come: each waits in the
/// queue, which replaces waiting saves, while the one before is handled.
async fn run(mut woken: UnboundedReceiver<Numbered>) {
    let mut files = Files::new(Stores::user());
    let mut queue = Queue::default();
    while let Some(first) = woken.next().await {
        queue.push(first);
        loop {
            // What was posted while the last request was handled.
            while let Ok(next) = woken.try_recv() {
                queue.push(next);
            }
            let Some(Numbered {
                seq,
                request,
                object,
            }) = queue.pop()
            else {
                break;
            };
            let relist = files.relists(&request);
            answer(seq, &mut files, request, object).await;
            if relist {
                answer(seq, &mut files, Request::ListRecovered, None).await;
            }
        }
    }
}

/// Handles `request` and posts its answer, numbered `seq`: the number of
/// the request it answers, or for recovered designs listed after one (see
/// `Files::relists`), of that one.
async fn answer(seq: u64, files: &mut Files, request: Request, object: Option<Handed>) {
    let failed = request.failure();
    let response = files.handle(request, object).await;
    // A response too large to post loses only its request.
    let bytes = wire::encode_response(seq, response, failed, wire::MAX_MESSAGE_BYTES)
        .unwrap_or_else(|e| wasm_bindgen::throw_str(&e.to_string()));
    worker::post(&[&bytes]);
}
