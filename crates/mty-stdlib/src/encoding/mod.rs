//! `std.encoding` — byte ↔ string codec primitives.
//!
//! v0.39 T1 ships two submodules: [`base64`] (RFC 4648 § 4 + § 5) and
//! [`hex`] (RFC 4648 § 8). Both are pure functions — no capability.
//!
//! Mighty surface:
//!
//! ```ignore
//! use std.encoding.{base64, hex};
//!
//! let s: Str = base64.encode(b"hello");      // "aGVsbG8="
//! let b: Vec<U8> = base64.decode(s)?;
//! let hx: Str = hex.encode(b"\xde\xad\xbe\xef");  // "deadbeef"
//! ```

pub mod base64;
pub mod hex;
