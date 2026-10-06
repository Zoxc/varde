//! The IO worker's side of the web build, see the parent module: run by the
//! web app's `serve_worker` in the role [`WORKER_ROLE`](crate::WORKER_ROLE),
//! never on the page.

mod disk;
mod files;
mod opfs;
pub(crate) mod serve;
mod settings;
