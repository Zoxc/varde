//! Bytes received from the other side of a Web Worker, copied out once and
//! only if they're within a bound: the length is the other side's to say,
//! so a broken message mustn't allocate without end. Plain Rust, so it's
//! tested natively.

use std::fmt;

/// Bytes received from the other side, copied once into the vector they
/// end up in: a JS `Uint8Array` on the web, a slice in tests.
pub trait Buffer {
    fn byte_len(&self) -> usize;
    /// Copies all the bytes to `out`, which is exactly `byte_len` long.
    fn copy_into(&self, out: &mut [u8]);
}

impl Buffer for [u8] {
    fn byte_len(&self) -> usize {
        self.len()
    }

    fn copy_into(&self, out: &mut [u8]) {
        out.copy_from_slice(self);
    }
}

impl<B: Buffer + ?Sized> Buffer for &B {
    fn byte_len(&self) -> usize {
        (**self).byte_len()
    }

    fn copy_into(&self, out: &mut [u8]) {
        (**self).copy_into(out);
    }
}

#[cfg(target_arch = "wasm32")]
impl Buffer for js_sys::Uint8Array {
    fn byte_len(&self) -> usize {
        self.length() as usize
    }

    fn copy_into(&self, out: &mut [u8]) {
        self.copy_to(out);
    }
}

/// The bytes of `buffer`, copied out of it if it's at most `max` bytes.
pub fn copy(buffer: &(impl Buffer + ?Sized), max: usize) -> Result<Vec<u8>, TooLarge> {
    let len = buffer.byte_len();
    if len > max {
        return Err(TooLarge { len });
    }
    let mut out = vec![0; len];
    buffer.copy_into(&mut out);
    Ok(out)
}

/// Bytes refused for being more than their bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge {
    /// How many bytes there were.
    pub len: usize,
}

impl fmt::Display for TooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "message too large: {} bytes", self.len)
    }
}

impl std::error::Error for TooLarge {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claims more bytes than there are, and must never be copied.
    struct Huge(usize);

    impl Buffer for Huge {
        fn byte_len(&self) -> usize {
            self.0
        }

        fn copy_into(&self, _: &mut [u8]) {
            unreachable!("copied a buffer too large");
        }
    }

    #[test]
    fn copies_bytes_within_the_bound() {
        assert_eq!(copy(&[1, 2, 3][..], 3), Ok(vec![1, 2, 3]));
        assert_eq!(copy(&[][..], 0), Ok(Vec::new()));
    }

    #[test]
    fn refuses_bytes_over_the_bound_before_copying() {
        assert_eq!(copy(&[1, 2, 3][..], 2), Err(TooLarge { len: 3 }));
        assert_eq!(
            copy(&Huge(usize::MAX), 1 << 30),
            Err(TooLarge { len: usize::MAX })
        );
    }
}
