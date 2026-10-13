//! The plain Rust halves of what only the web uses, apart from `web` so
//! native tests build them too: the store directories' shared logic
//! ([`dir`]), the downloads list ([`downloads`]), the checks of the numbers
//! JS hands over ([`numbers`]), the names in the Origin Private File System
//! ([`names`]) and the IO worker's messages ([`wire`]).

pub(crate) mod dir;
pub(crate) mod downloads;
pub(crate) mod names;
pub(crate) mod numbers;
pub(crate) mod wire;
