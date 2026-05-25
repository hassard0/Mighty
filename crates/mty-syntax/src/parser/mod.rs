use crate::{lexer::LexedToken, SyntaxKind};
use rowan::GreenNodeBuilder;

pub mod agents;
pub mod concurrency;
pub mod exprs;
pub mod extern_;
pub mod items;
pub mod macros;
pub mod paths;
pub mod patterns;
pub mod recovery;
pub mod stmts;
pub mod types;
pub mod unsafe_;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

pub struct ParseResult {
    pub green: rowan::GreenNode,
    pub errors: Vec<ParseError>,
}

/// Parser knobs that callers (LSP, formatter, build driver) can use
/// to bound resource usage on adversarial inputs. v0.8 adds
/// `max_diagnostics` so a 10 KLOC file with a stray brace can't emit
/// 50 000 diagnostics and freeze the IDE.
#[derive(Debug, Clone, Copy)]
pub struct ParseOpts {
    /// Cap the number of diagnostics emitted. Defaults to `usize::MAX`
    /// (no cap). Set to e.g. 256 for the LSP path.
    pub max_diagnostics: usize,
}

impl Default for ParseOpts {
    fn default() -> Self {
        Self {
            max_diagnostics: usize::MAX,
        }
    }
}

pub struct Parser<'src> {
    pub(crate) tokens: Vec<LexedToken<'src>>,
    pub(crate) pos: usize,
    pub(crate) builder: GreenNodeBuilder<'static>,
    pub(crate) errors: Vec<ParseError>,
    /// When true, a `{` immediately following a primary expression is **not**
    /// treated as the start of a struct literal. Used when parsing control-flow
    /// conditions (`if`, `while`, `for`) so `if x { ... }` parses as
    /// "condition `x`, body `{ ... }`" rather than "struct literal `x { ... }`".
    pub(crate) no_struct_literal: bool,
    /// v0.8: maximum diagnostics this parser will emit. Reached via
    /// [`ParseOpts`]. Once hit, further `error_at` calls become no-ops.
    pub(crate) max_diagnostics: usize,
}

impl<'src> Parser<'src> {
    pub fn new(src: &'src str) -> Self {
        Self::with_opts(src, ParseOpts::default())
    }

    pub fn with_opts(src: &'src str, opts: ParseOpts) -> Self {
        Self {
            tokens: crate::lex(src),
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            no_struct_literal: false,
            max_diagnostics: opts.max_diagnostics,
        }
    }

    pub fn parse_file(mut self) -> ParseResult {
        self.builder.start_node(SyntaxKind::FILE.into());
        self.skip_trivia();
        while !self.at(SyntaxKind::EOF) {
            if !items::item(&mut self) {
                let t_start = self.tokens[self.pos].start;
                let t_end = self.tokens[self.pos].end;
                let t_text = self.tokens[self.pos].text.to_string();
                self.error_at(format!("unexpected token `{}`", t_text), t_start, t_end);
                self.bump_any();
                self.skip_trivia();
            }
        }
        self.builder.finish_node();
        ParseResult {
            green: self.builder.finish(),
            errors: self.errors,
        }
    }

    // ---- cursor primitives ----

    pub(crate) fn peek(&self) -> SyntaxKind {
        self.tokens[self.pos].kind
    }
    pub(crate) fn peek_n(&self, n: usize) -> SyntaxKind {
        self.tokens
            .get(self.pos + n)
            .map(|t| t.kind)
            .unwrap_or(SyntaxKind::EOF)
    }
    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.peek() == kind
    }
    #[allow(dead_code)] // used by upcoming parser productions in slice-2
    pub(crate) fn at_set(&self, set: &[SyntaxKind]) -> bool {
        set.contains(&self.peek())
    }

    pub(crate) fn bump_any(&mut self) {
        let t = &self.tokens[self.pos];
        if t.kind != SyntaxKind::EOF {
            self.builder.token(t.kind.into(), t.text);
            self.pos += 1;
        }
    }
    pub(crate) fn bump(&mut self, kind: SyntaxKind) {
        assert_eq!(self.peek(), kind);
        self.bump_any();
    }
    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump_any();
            self.skip_trivia();
            true
        } else {
            false
        }
    }
    pub(crate) fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            true
        } else {
            let t_text = self.tokens[self.pos].text.to_string();
            let s = self.tokens[self.pos].start;
            let e = self.tokens[self.pos].end;
            self.error_at(format!("expected {:?}, got `{}`", kind, t_text), s, e);
            false
        }
    }

    pub(crate) fn skip_trivia(&mut self) {
        while self.peek().is_trivia() {
            self.bump_any();
        }
    }

    pub(crate) fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(kind.into());
    }
    pub(crate) fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    pub(crate) fn checkpoint(&self) -> rowan::Checkpoint {
        self.builder.checkpoint()
    }
    pub(crate) fn start_node_at(&mut self, cp: rowan::Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(cp, kind.into());
    }

    pub(crate) fn error_at(&mut self, message: String, start: usize, end: usize) {
        // v0.8 diag throttle: silently drop additional diagnostics
        // once the per-parse cap is reached. The first N still surface
        // so the user sees the prefix of error spam, not none of it.
        if self.errors.len() >= self.max_diagnostics {
            return;
        }
        self.errors.push(ParseError {
            message,
            start,
            end,
        });
    }

    /// Temporarily disable struct-literal parsing while `f` runs, then
    /// restore the previous state. Used for control-flow conditions.
    pub(crate) fn with_no_struct_literal<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let prev = self.no_struct_literal;
        self.no_struct_literal = true;
        let r = f(self);
        self.no_struct_literal = prev;
        r
    }

    /// Re-enable struct literals inside `f` (used when entering a
    /// delimited subexpression like `(...)` or `[...]` where the
    /// outer no-struct-literal context shouldn't apply).
    pub(crate) fn with_struct_literal<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let prev = self.no_struct_literal;
        self.no_struct_literal = false;
        let r = f(self);
        self.no_struct_literal = prev;
        r
    }

    pub(crate) fn error(&mut self, message: impl Into<String>) {
        let s = self.tokens[self.pos].start;
        let e = self.tokens[self.pos].end;
        self.error_at(message.into(), s, e);
    }
}

pub fn parse(src: &str) -> ParseResult {
    Parser::new(src).parse_file()
}

/// Parse with caller-supplied options. v0.8 entry point.
pub fn parse_with_opts(src: &str, opts: ParseOpts) -> ParseResult {
    Parser::with_opts(src, opts).parse_file()
}

pub fn parse_type(src: &str) -> ParseResult {
    let mut p = Parser::new(src);
    p.builder.start_node(SyntaxKind::FILE.into());
    p.skip_trivia();
    types::type_expr(&mut p);
    p.builder.finish_node();
    ParseResult {
        green: p.builder.finish(),
        errors: p.errors,
    }
}

pub fn parse_pattern(src: &str) -> ParseResult {
    let mut p = Parser::new(src);
    p.builder.start_node(SyntaxKind::FILE.into());
    p.skip_trivia();
    patterns::pattern(&mut p);
    p.builder.finish_node();
    ParseResult {
        green: p.builder.finish(),
        errors: p.errors,
    }
}

pub fn parse_expr(src: &str) -> ParseResult {
    let mut p = Parser::new(src);
    p.builder.start_node(SyntaxKind::FILE.into());
    p.skip_trivia();
    exprs::expr(&mut p);
    p.builder.finish_node();
    ParseResult {
        green: p.builder.finish(),
        errors: p.errors,
    }
}

#[cfg(test)]
mod opts_tests {
    use super::*;

    #[test]
    fn diag_throttle_caps_emitted_errors() {
        // Garbage input: hundreds of unrecognised tokens. Default cap
        // would emit hundreds of diagnostics.
        let mut s = String::new();
        for _ in 0..500 {
            s.push_str("@ ");
        }
        // Uncapped: many errors.
        let uncapped = parse(&s);
        assert!(
            uncapped.errors.len() > 10,
            "expected lots of errors, got {}",
            uncapped.errors.len()
        );
        // Capped: at most 16.
        let capped = parse_with_opts(
            &s,
            ParseOpts {
                max_diagnostics: 16,
            },
        );
        assert!(
            capped.errors.len() <= 16,
            "diag throttle leaked: {}",
            capped.errors.len()
        );
    }

    #[test]
    fn diag_throttle_default_uncapped() {
        let opts = ParseOpts::default();
        assert_eq!(opts.max_diagnostics, usize::MAX);
    }
}
