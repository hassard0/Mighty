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
    /// v0.22: numeric diagnostic code (matches `mty_diagnostics::DiagCode`).
    /// Defaults to `1` = MT0001 UNEXPECTED_TOKEN so existing call-sites
    /// preserve behaviour. Set explicitly for MT0004 (unknown duration
    /// unit), MT0030 (depth limit exceeded), etc.
    pub code: u16,
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
        // v0.22 Coverage Closure: pre-lex pass that surfaces MT0004
        // (unknown duration unit) and MT0030 (depth limit exceeded) so
        // the codes don't keep funnelling to MT0001.
        self.pre_lex_scan();
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

    /// v0.22 Coverage Closure: one-pass scan over the lexed token stream
    /// to detect two shapes that previously funnelled through the generic
    /// MT0001 path:
    ///
    /// - **MT0004 UNKNOWN_DURATION_UNIT**: an `INT_LITERAL` immediately
    ///   followed (zero source gap) by an `IDENT`. The lexer's duration
    ///   regex (`[0-9]+(?:ns|us|ms|s|m|h)`) and size regex
    ///   (`[0-9]+(?:KiB|MiB|GiB|B|k|M)`) only accept the canonical units;
    ///   anything else (e.g. `5sec`, `10seconds`, `2z`) survives as
    ///   `INT IDENT` and would otherwise hit MT0001.
    ///
    /// - **MT0030 DEPTH_LIMIT_EXCEEDED**: maximum nesting depth of
    ///   `(`/`[`/`{` exceeds 256. Adversarial input that nests deeper
    ///   gets a single MT0030 emitted at the point of overflow.
    fn pre_lex_scan(&mut self) {
        use crate::SyntaxKind::*;
        const DEPTH_LIMIT: i32 = 256;
        let mut depth: i32 = 0;
        let mut depth_exceeded = false;
        // First pass: collect MT0004 / MT0030 sites then push diagnostics
        // (so we don't borrow self.tokens while pushing).
        let mut to_emit: Vec<(u16, String, usize, usize)> = vec![];
        for i in 0..self.tokens.len() {
            let t = &self.tokens[i];
            match t.kind {
                L_PAREN | L_BRACK | L_BRACE => {
                    depth += 1;
                    if depth > DEPTH_LIMIT && !depth_exceeded {
                        depth_exceeded = true;
                        to_emit.push((
                            30,
                            format!("recursion depth limit exceeded ({} levels)", DEPTH_LIMIT),
                            t.start,
                            t.end,
                        ));
                    }
                }
                R_PAREN | R_BRACK | R_BRACE => {
                    depth -= 1;
                }
                INT_LITERAL | DURATION_LITERAL => {
                    // Look ahead for an IDENT with zero gap.
                    //
                    // INT_LITERAL+IDENT covers shapes like `5xs`,
                    // `2nanoseconds` (lexer's duration regex doesn't
                    // recognise the prefix because `x` / `na` aren't
                    // valid prefix chars).
                    //
                    // DURATION_LITERAL+IDENT covers shapes like
                    // `10seconds` — the lexer greedily matches the
                    // canonical `s` suffix, leaving `econds` as a
                    // trailing IDENT. The author clearly meant a
                    // longer (invalid) unit, so we still surface
                    // MT0004 unconditionally on that shape.
                    if let Some(next) = self.tokens.get(i + 1) {
                        if next.kind == IDENT && next.start == t.end {
                            let should_fire = if t.kind == DURATION_LITERAL {
                                // DURATION_LITERAL already produced a
                                // valid duration — but an IDENT glued
                                // to it signals a mistyped longer
                                // unit. Always fire.
                                true
                            } else {
                                is_duration_unit_like(next.text)
                            };
                            if should_fire {
                                let full_unit = if t.kind == DURATION_LITERAL {
                                    // Strip the leading digits to recover
                                    // the canonical-unit fragment (`s`,
                                    // `ms`, ...) so we can compose a
                                    // human-friendly note.
                                    let prefix: String =
                                        t.text.chars().skip_while(|c| c.is_ascii_digit()).collect();
                                    format!("{}{}", prefix, next.text)
                                } else {
                                    next.text.to_string()
                                };
                                to_emit.push((
                                    4,
                                    format!(
                                        "unknown duration unit `{}` (expected one of `ns`, `us`, `ms`, `s`, `m`, `h`)",
                                        full_unit
                                    ),
                                    t.start,
                                    next.end,
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for (code, msg, s, e) in to_emit {
            self.error_at_code(code, msg, s, e);
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
    /// Consume the current token but emit it under a *different* SyntaxKind
    /// in the green tree. Used for contextual / "soft" keywords like
    /// `budget` (v0.29 Track E): the lexer produces IDENT; the parser
    /// recognises it positionally and tags the token as BUDGET_KW so HIR
    /// / formatter / snapshots see a stable kind.
    pub(crate) fn bump_remap(&mut self, kind: SyntaxKind) {
        let t = &self.tokens[self.pos];
        if t.kind != SyntaxKind::EOF {
            self.builder.token(kind.into(), t.text);
            self.pos += 1;
        }
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
            code: 1, // MT0001 UNEXPECTED_TOKEN
        });
    }

    /// v0.22: emit a parse error with an explicit diagnostic code (e.g.
    /// MT0004 = 4, MT0030 = 30). Used by lex-pass / depth-guard sites
    /// that need to override the default MT0001 code.
    pub(crate) fn error_at_code(&mut self, code: u16, message: String, start: usize, end: usize) {
        if self.errors.len() >= self.max_diagnostics {
            return;
        }
        self.errors.push(ParseError {
            message,
            start,
            end,
            code,
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

/// v0.22 Coverage Closure: classifier for MT0004 (UNKNOWN_DURATION_UNIT).
///
/// Returns true iff the trailing identifier text looks like the author
/// intended a duration unit but got the spelling wrong. Conservative —
/// we surface MT0004 only when the ident is plausibly a (mistyped)
/// duration unit, so legitimate identifiers that happen to follow a
/// literal without whitespace (extremely rare in this grammar but
/// possible through macros) don't get mis-tagged.
///
/// Heuristic: short (<=12 chars) lowercase identifier that contains the
/// substring "s", "m", "h", "min", "sec", "hour", "day", "msec", "usec",
/// or "nsec", AND is not one of the canonical valid units already
/// consumed by the duration regex (`ns`, `us`, `ms`, `s`, `m`, `h`). The
/// canonical units never appear in this path because the lexer's regex
/// has higher priority.
fn is_duration_unit_like(text: &str) -> bool {
    if text.is_empty() || text.len() > 12 {
        return false;
    }
    if !text.chars().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    // Quick affirmative list of typical mistakes.
    matches!(
        text,
        "sec"
            | "secs"
            | "second"
            | "seconds"
            | "min"
            | "mins"
            | "minute"
            | "minutes"
            | "hour"
            | "hours"
            | "day"
            | "days"
            | "msec"
            | "msecs"
            | "usec"
            | "usecs"
            | "nsec"
            | "nsecs"
            | "millis"
            | "millisecond"
            | "milliseconds"
            | "microsecond"
            | "microseconds"
            | "nanosecond"
            | "nanoseconds"
    )
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
