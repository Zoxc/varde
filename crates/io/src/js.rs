//! Glue between Rust and the JS the web build calls: the conversions of
//! the numbers JS hands over, which are `f64`s, and of its exceptions to IO
//! errors. Used by the Origin Private File System's store (`src/opfs.rs`)
//! and by files of the user's (`src/pick.rs`).
//!
//! Plain Rust, tested natively; waiting for JS promises and turning its
//! exceptions into IO errors is `src/web/js.rs`'s.

use std::io;

use crate::wire::MAX_MESSAGE_BYTES;

/// The largest integer an `f64` holds exactly, and so the largest size or
/// offset that crosses to JS and back unchanged.
pub(crate) const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// A size or count JS gave, as a whole number of bytes.
pub(crate) fn size(value: f64) -> io::Result<u64> {
    let whole = value.is_finite() && value >= 0.0 && value.fract() == 0.0;
    if whole && value <= MAX_SAFE_INTEGER as f64 {
        // Exact: a whole number within the safe range.
        Ok(value as u64)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the browser gave {value} as a size"),
        ))
    }
}

/// `n` as a JS number: refused past `MAX_SAFE_INTEGER`, where it'd round.
pub(crate) fn number(n: u64) -> io::Result<f64> {
    if n <= MAX_SAFE_INTEGER {
        // Exact: within the safe range.
        Ok(n as f64)
    } else {
        Err(io::Error::from(io::ErrorKind::FileTooLarge))
    }
}

/// The offset `n` bytes past `at`, as JS takes it.
pub(crate) fn offset(at: u64, n: usize) -> io::Result<f64> {
    let offset = u64::try_from(n)
        .ok()
        .and_then(|n| at.checked_add(n))
        .ok_or_else(|| io::Error::from(io::ErrorKind::FileTooLarge))?;
    number(offset)
}

/// A length JS gave, at most `max`; `too_long` is the error for a whole
/// number past it.
pub(crate) fn len(
    value: f64,
    max: usize,
    too_long: impl FnOnce(u64) -> io::Error,
) -> io::Result<usize> {
    let len = size(value)?;
    usize::try_from(len)
        .ok()
        .filter(|&len| len <= max)
        .ok_or_else(|| too_long(len))
}

/// The size of a file of the user's, as JS gave it, as the length to read
/// it into: refused if it's larger than a message can carry.
pub(crate) fn file_len(size: f64) -> io::Result<usize> {
    len(size, MAX_MESSAGE_BYTES, |size| {
        io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("the file is too large to open: {size} bytes"),
        )
    })
}

/// A time JS gave in milliseconds since the Unix epoch, in seconds.
pub(crate) fn seconds(millis: f64) -> Option<i64> {
    let seconds = (millis / 1000.0).floor();
    // `i64::MAX as f64` rounds up, so the bound is exclusive.
    (seconds.is_finite() && seconds >= i64::MIN as f64 && seconds < i64::MAX as f64)
        .then_some(seconds as i64)
}

/// What a `DOMException` named `name` means as an IO error.
pub(crate) fn error_kind(name: &str) -> io::ErrorKind {
    match name {
        "NotFoundError" => io::ErrorKind::NotFound,
        // Another tab, or this one, holds a sync access handle on it.
        "NoModificationAllowedError" => io::ErrorKind::ResourceBusy,
        "QuotaExceededError" => io::ErrorKind::StorageFull,
        "NotAllowedError" | "SecurityError" => io::ErrorKind::PermissionDenied,
        "TypeMismatchError" | "TypeError" => io::ErrorKind::InvalidInput,
        _ => io::ErrorKind::Other,
    }
}

#[cfg(test)]
mod tests;
