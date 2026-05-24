//! Layout engine for [`crate::doc::Doc`].
//!
//! Implements the standard Wadler/Lindig algorithm: walk the doc tree with
//! an explicit stack, tracking the current column. When a `Group` is
//! encountered, use the `fits` lookahead to decide whether the whole group
//! can render on the current line in `Flat` mode; otherwise switch the
//! group to `Break` mode so its `Line`s become newlines.

use crate::doc::Doc;

/// Layout configuration for [`pretty`].
pub struct Layout {
    /// Maximum desired line width in columns.
    pub width: usize,
}

impl Default for Layout {
    fn default() -> Self {
        Self { width: 100 }
    }
}

#[derive(Copy, Clone)]
enum Mode {
    Flat,
    Break,
}

/// Render a doc as a string within `layout`'s column budget.
pub fn pretty(doc: &Doc, layout: &Layout) -> String {
    let mut out = String::new();
    // Stack of (indent, mode, doc) entries, popped LIFO. We push the right
    // child of a Concat first so the left child is processed first.
    let mut stack: Vec<(usize, Mode, &Doc)> = vec![(0, Mode::Flat, doc)];
    let mut col: usize = 0;
    while let Some((indent, mode, d)) = stack.pop() {
        match d {
            Doc::Nil => {}
            Doc::Text(s) => {
                out.push_str(s);
                col += s.chars().count();
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    out.push(' ');
                    col += 1;
                }
                Mode::Break => {
                    out.push('\n');
                    for _ in 0..indent {
                        out.push(' ');
                    }
                    col = indent;
                }
            },
            Doc::SoftLine => {
                if let Mode::Break = mode {
                    out.push('\n');
                    for _ in 0..indent {
                        out.push(' ');
                    }
                    col = indent;
                }
            }
            Doc::Nest(n, inner) => {
                stack.push((indent + n, mode, inner));
            }
            Doc::Group(inner) => {
                let m = if fits(layout.width, col, indent, Mode::Flat, inner) {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                stack.push((indent, m, inner));
            }
            Doc::Concat(a, b) => {
                stack.push((indent, mode, b));
                stack.push((indent, mode, a));
            }
        }
    }
    out
}

/// Lookahead: would `doc`, starting at column `col` with the given
/// `indent` and `mode`, fit within `width`? A hard `Line` in `Break` mode
/// resets the analysis (the line ends, so anything after fits trivially).
fn fits(width: usize, mut col: usize, indent: usize, mode: Mode, doc: &Doc) -> bool {
    let mut stack: Vec<(usize, Mode, &Doc)> = vec![(indent, mode, doc)];
    while col <= width {
        let Some((ind, m, d)) = stack.pop() else {
            return true;
        };
        match d {
            Doc::Nil => {}
            Doc::Text(s) => col += s.chars().count(),
            Doc::Line => match m {
                Mode::Flat => col += 1,
                Mode::Break => return true,
            },
            Doc::SoftLine => {
                if let Mode::Break = m {
                    return true;
                }
            }
            Doc::Nest(n, inner) => stack.push((ind + n, m, inner)),
            // For fitting purposes we assume nested groups also try Flat
            // first; this matches the standard Wadler algorithm.
            Doc::Group(inner) => stack.push((ind, Mode::Flat, inner)),
            Doc::Concat(a, b) => {
                stack.push((ind, m, b));
                stack.push((ind, m, a));
            }
        }
    }
    false
}
