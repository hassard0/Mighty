//! Wadler/Lindig pretty-printer Doc.
//!
//! A `Doc` is an abstract document description that the printer in
//! [`crate::printer`] lays out within a column budget. The combinators here
//! follow the classic Wadler/Lindig algebra: `text` for literal strings,
//! `line` for forced breaks, `softline` for breaks that only fire inside a
//! broken group, `nest` to increase the indentation of nested layouts, and
//! `group` to mark a region whose breaks all collapse together when they
//! fit on the current line.

use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Doc {
    /// Empty document.
    Nil,
    /// A literal piece of text. Must not contain newlines.
    Text(Rc<str>),
    /// A hard break: always emits a newline in `Break` mode and a single
    /// space in `Flat` mode.
    Line,
    /// A soft break: emits a newline only when the enclosing group is
    /// rendered in `Break` mode; otherwise emits nothing.
    SoftLine,
    /// Increase the current indent by `n` columns for the inner doc.
    Nest(usize, Box<Doc>),
    /// Mark a region that should be laid out flat if it fits within the
    /// remaining width; otherwise the inner breaks are honored.
    Group(Box<Doc>),
    /// Sequential composition of two docs.
    Concat(Box<Doc>, Box<Doc>),
}

impl Doc {
    /// The empty document.
    pub fn nil() -> Self {
        Doc::Nil
    }

    /// A literal text fragment.
    pub fn text(s: impl Into<Rc<str>>) -> Self {
        Doc::Text(s.into())
    }

    /// A hard line break (space in flat mode, newline in break mode).
    pub fn line() -> Self {
        Doc::Line
    }

    /// A soft line break (nothing in flat mode, newline in break mode).
    pub fn softline() -> Self {
        Doc::SoftLine
    }

    /// Indent the inner doc by `n` additional columns.
    pub fn nest(n: usize, d: Doc) -> Self {
        Doc::Nest(n, Box::new(d))
    }

    /// Group a doc so its breaks all collapse together when it fits.
    pub fn group(d: Doc) -> Self {
        Doc::Group(Box::new(d))
    }

    /// Concatenate two docs.
    pub fn concat(a: Doc, b: Doc) -> Self {
        Doc::Concat(Box::new(a), Box::new(b))
    }

    /// Concatenate an iterator of docs left-to-right.
    pub fn concat_all(parts: impl IntoIterator<Item = Doc>) -> Self {
        parts.into_iter().fold(Doc::nil(), Doc::concat)
    }

    /// Join an iterator of docs with `sep` between adjacent elements.
    pub fn join(sep: Doc, parts: impl IntoIterator<Item = Doc>) -> Self {
        let mut iter = parts.into_iter();
        match iter.next() {
            None => Doc::nil(),
            Some(first) => iter.fold(first, |acc, d| {
                Doc::concat(Doc::concat(acc, sep.clone()), d)
            }),
        }
    }
}
