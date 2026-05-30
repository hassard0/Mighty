//! `std.url` — URL parsing, building, and percent-encoding.
//!
//! Backed by the `url` crate (an RFC 3986 + WHATWG URL Standard
//! implementation) with a Mighty-shaped surface on top — `Url` is a
//! plain struct with named fields rather than the crate's
//! getter-method shape, because that's what reads well from Mighty
//! source.
//!
//! ```ignore
//! use std.url.{Url, percent_encode};
//!
//! let u: Url = Url.parse("https://example.com/path?q=hello world")?;
//! log("host={}, query={}", u.host, u.query);
//! let enc: Str = percent_encode("hello world");  // "hello%20world"
//! ```

pub mod build;
pub mod encode;
pub mod parse;

pub use build::UrlBuilder;
pub use encode::{percent_decode, percent_encode, percent_encode_component};
pub use parse::{parse, Url, UrlErr};
