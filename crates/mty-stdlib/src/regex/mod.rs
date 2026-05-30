//! `std.regex` — regular expressions.
//!
//! v0.40 T4 adds the regex surface that every web app, log parser,
//! validator, and templating layer eventually wants. Backed by the
//! [`regex`] crate (RE2-style finite automata — guaranteed linear time
//! in the input, no catastrophic backtracking).
//!
//! Mighty surface:
//!
//! ```ignore
//! use std.regex.Regex;
//!
//! let r = Regex.new(r"\d{4}-\d{2}-\d{2}")?;
//! let m: Option<Match> = r.find("date: 2026-05-30");
//! let all: Vec<Match>  = r.find_all("2026-05-30 to 2026-06-01");
//! let caps: Option<Captures> = Regex.new(r"(\w+)=(\w+)")?.captures("k=v");
//! let new_str: Str = r.replace_all("date: 2026-05-30", "[date]");
//! ```
//!
//! Syntax: see the [`regex` crate's syntax
//! reference](https://docs.rs/regex/latest/regex/#syntax) — Unicode
//! categories, ASCII shorthands (`\d \w \s`), groups, alternation,
//! repetition, anchors, lookarounds (not supported — RE2 trade-off),
//! and Unicode word boundaries.
//!
//! No capability required: a `Regex` is pure data and matching a
//! `Regex` against a `Str` is a pure function.

pub mod r#match;
// `regex::regex` is intentional — the crate name shadowing reads
// well from Mighty source (`std.regex.Regex`) and matches the
// crate-as-namespace convention used elsewhere in the workspace.
#[allow(clippy::module_inception)]
pub mod regex;

pub use r#match::{Captures, Match};
pub use regex::{Regex, RegexErr};
