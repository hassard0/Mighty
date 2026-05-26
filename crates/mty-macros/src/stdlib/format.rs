//! The `format!` builtin macro (v0.24 Track B).
//!
//! Unlike `assert!`/`debug!`/`unreachable!`, `format!` is not a plain
//! declarative macro that can be expressed as `pub macro format(...)
//! => { ... }` — its first argument is a *format string* whose body
//! has its own grammar (`{}`, `{x}`, `{name}`, `{{`, `}}`, …) that
//! interleaves with the trailing argument list. Token substitution
//! alone cannot drive that walk.
//!
//! Instead, the macro is *expanded by code*: callers hand the source
//! slice of the call's parenthesised argument list to [`expand_format_call`],
//! which returns either a Mighty source snippet that the HIR
//! preprocessor splices back in, or a structured error the call site
//! reports as a diagnostic.
//!
//! ## Supported spec subset (v0.24)
//!
//! | Spec        | Behaviour                                            |
//! |-------------|------------------------------------------------------|
//! | `{}`        | positional, `.to_str()`                              |
//! | `{:x}`      | positional, `.to_hex_str()` (lowercase hex)          |
//! | `{:X}`      | positional, `.to_hex_upper_str()` (uppercase hex)    |
//! | `{:?}`      | positional, `.to_debug_str()` (debug)                |
//! | `{name}`    | named-arg passthrough, `.to_str()` on in-scope `name`|
//! | `{name:x}`  | named-arg passthrough, `.to_hex_str()` on `name`     |
//! | `{name:X}`  | named-arg passthrough, `.to_hex_upper_str()` on `name`|
//! | `{name:?}`  | named-arg passthrough, `.to_debug_str()` on `name`   |
//! | `{{` / `}}` | literal `{` / `}`                                    |
//!
//! Per the Rust convention, `{x}` is a *named-arg passthrough* (refers
//! to in-scope `x`), NOT a positional hex sigil. Use `{:x}` for
//! positional hex, `{x:x}` for "named x rendered as hex".
//!
//! ## Deferred to v0.25
//!
//! Width (`{:05}`), precision (`{:.3}`), alignment/fill (`{:>10}`,
//! `{:*<5}`), sign flags (`{:+}`), and `0` padding modifiers all
//! return [`FormatExpandError::UnsupportedSpec`]. See the notes file
//! `dev/history/notes/FORMAT_MACRO_V0_24_NOTES.md` for the roadmap.

use crate::token::lex_fragment;
use mty_syntax::SyntaxKind;

/// One concrete piece of a parsed format template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatPiece {
    /// Literal source text (`{{` and `}}` already decoded to `{` / `}`).
    Literal(String),
    /// A placeholder that consumes one of the trailing positional args.
    /// `kind` selects which conversion method the arg is funneled through.
    Positional { kind: ConvKind },
    /// A `{name}` placeholder that resolves to an in-scope identifier.
    /// The expander emits `name.<conv>()` directly — no positional arg
    /// is consumed.
    Named { ident: String, kind: ConvKind },
}

/// How a placeholder converts its argument to a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvKind {
    /// `{}` / `{name}` — `.to_str()`.
    Display,
    /// `{:x}` / `{name:x}` — `.to_hex_str()`.
    HexLower,
    /// `{:X}` / `{name:X}` — `.to_hex_upper_str()`.
    HexUpper,
    /// `{:?}` / `{name:?}` — `.to_debug_str()`.
    Debug,
}

impl ConvKind {
    /// The Mighty method name the expander emits for this conversion.
    /// The mty-stdlib `fmt` module documents these as the runtime
    /// contract; the SIR interpreter implements them in `mty-ir`.
    pub fn method(self) -> &'static str {
        match self {
            ConvKind::Display => "to_str",
            ConvKind::HexLower => "to_hex_str",
            ConvKind::HexUpper => "to_hex_upper_str",
            ConvKind::Debug => "to_debug_str",
        }
    }
}

/// Errors the format-template walker can report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatExpandError {
    /// First argument was missing, or not a string literal.
    NotAStringLiteral,
    /// Unbalanced `{` (no matching `}`).
    UnclosedBrace { position: usize },
    /// Lone `}` outside a placeholder.
    UnexpectedCloseBrace { position: usize },
    /// Spec like `{:05}` or `{:.3}` that v0.24 doesn't implement.
    UnsupportedSpec { spec: String, position: usize },
    /// `format!("{} {}", 1)` — three placeholders, two args.
    NotEnoughArgs { expected: usize, given: usize },
    /// `format!("{}", 1, 2)` — extra args supplied. Warning-ish but we
    /// reject for clarity; the caller surfaces it as MT6002 arity.
    TooManyArgs { expected: usize, given: usize },
    /// `format!(...)` was passed zero arguments.
    EmptyArgList,
}

impl std::fmt::Display for FormatExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatExpandError::NotAStringLiteral => write!(
                f,
                "first argument to `format!` must be a string literal"
            ),
            FormatExpandError::UnclosedBrace { position } => {
                write!(f, "unclosed `{{` in format template at position {position}")
            }
            FormatExpandError::UnexpectedCloseBrace { position } => write!(
                f,
                "unexpected `}}` in format template at position {position} (escape as `}}}}` to emit a literal `}}`)"
            ),
            FormatExpandError::UnsupportedSpec { spec, position } => write!(
                f,
                "format spec `{{:{spec}}}` at position {position} is not supported in v0.24 (only `{{}}`, `{{x}}`, `{{X}}`, `{{?}}`, and named-arg passthrough ship; width / precision / alignment land in v0.25)"
            ),
            FormatExpandError::NotEnoughArgs { expected, given } => write!(
                f,
                "format template requires {expected} positional argument(s), but {given} supplied"
            ),
            FormatExpandError::TooManyArgs { expected, given } => write!(
                f,
                "format template uses {expected} positional argument(s); {given} supplied"
            ),
            FormatExpandError::EmptyArgList => {
                write!(f, "`format!` requires at least the template argument")
            }
        }
    }
}

/// Parse the template into pieces. The template is the *decoded*
/// string body (escape sequences already turned into their characters),
/// not the source slice with surrounding `"`. Use [`decode_string_literal`]
/// first if you've got a raw source slice.
pub fn parse_template(template: &str) -> Result<Vec<FormatPiece>, FormatExpandError> {
    let bytes = template.as_bytes();
    let mut pieces: Vec<FormatPiece> = vec![];
    let mut lit = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'{' => {
                // `{{` is a literal `{`.
                if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    lit.push('{');
                    i += 2;
                    continue;
                }
                // Start of a placeholder. Find the matching `}`. We do
                // NOT support nested braces in v0.24.
                let start = i;
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != b'}' {
                    j += 1;
                }
                if j >= bytes.len() {
                    return Err(FormatExpandError::UnclosedBrace { position: start });
                }
                // Flush any pending literal before consuming the placeholder.
                if !lit.is_empty() {
                    pieces.push(FormatPiece::Literal(std::mem::take(&mut lit)));
                }
                let inner = &template[i + 1..j];
                pieces.push(parse_placeholder(inner, start)?);
                i = j + 1;
            }
            b'}' => {
                // `}}` is a literal `}`.
                if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                    lit.push('}');
                    i += 2;
                    continue;
                }
                return Err(FormatExpandError::UnexpectedCloseBrace { position: i });
            }
            _ => {
                // UTF-8 multi-byte chars: copy as-is. Since we only
                // pattern-match on ASCII `{` / `}`, a raw byte index
                // never lands inside a multi-byte boundary.
                lit.push(c as char);
                i += 1;
            }
        }
    }
    if !lit.is_empty() {
        pieces.push(FormatPiece::Literal(lit));
    }
    Ok(pieces)
}

/// Parse the body of a single `{ ... }` placeholder. `inner` is the
/// text between the braces (e.g. `""`, `"x"`, `"name"`, `":x"`,
/// `"name:x"`). `start_pos` is the byte index of the opening `{` in
/// the outer template, used for diagnostics.
fn parse_placeholder(inner: &str, start_pos: usize) -> Result<FormatPiece, FormatExpandError> {
    // Split on the *first* `:` — that separates the optional argument
    // selector (positional/named/sigil-conv) from the optional format spec.
    let (head, spec_opt) = match inner.find(':') {
        Some(idx) => (&inner[..idx], Some(&inner[idx + 1..])),
        None => (inner, None),
    };

    // Resolve the format spec (after the `:`). This is what selects
    // the conversion method for positional sites AND for named-arg
    // passthrough sites.
    let spec_kind = match spec_opt {
        None => None,
        Some("") => None,
        Some("x") => Some(ConvKind::HexLower),
        Some("X") => Some(ConvKind::HexUpper),
        Some("?") => Some(ConvKind::Debug),
        Some(other) => {
            return Err(FormatExpandError::UnsupportedSpec {
                spec: other.to_string(),
                position: start_pos,
            });
        }
    };
    let kind = spec_kind.unwrap_or(ConvKind::Display);

    // The head is either:
    //   ""    — positional placeholder (`{}` / `{:x}`)
    //   IDENT — named-arg passthrough (`{name}` / `{name:x}`)
    //
    // Per the Rust convention, `{x}` is a *named* arg referring to the
    // in-scope `x`; the hex conversion sigil is the spec form `{:x}`
    // (or `{name:x}` for a named arg). No positional shorthand sigils.
    if head.is_empty() {
        Ok(FormatPiece::Positional { kind })
    } else {
        // Treat as a Mighty identifier. Validate quickly so a typo like
        // `{1+2}` falls through to the lexer error path instead of
        // silently emitting `(1+2).to_str()`.
        if !is_identifier(head) {
            return Err(FormatExpandError::UnsupportedSpec {
                spec: head.to_string(),
                position: start_pos,
            });
        }
        Ok(FormatPiece::Named {
            ident: head.to_string(),
            kind,
        })
    }
}

/// True if `s` is a single Mighty identifier (ASCII letter/underscore
/// followed by letters/digits/underscores). v0.24 only supports plain
/// identifiers in named-arg passthrough.
fn is_identifier(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    it.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Decode a Mighty STRING_LITERAL source slice (including surrounding
/// quotes) into the runtime string the user typed. Mirrors
/// `mty-hir`'s `decode_str_escapes` for the subset of escapes the
/// front-end accepts. Returns `None` if the slice does not start and
/// end with `"`.
pub fn decode_string_literal(src: &str) -> Option<String> {
    let trimmed = src.trim();
    if !trimmed.starts_with('"') || !trimmed.ends_with('"') || trimmed.len() < 2 {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('0') => out.push('\0'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Format a Mighty string literal: escape `\` `"` `\n` `\t` `\r`.
fn escape_for_mty_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// Quick check: does `src` look like a single STRING_LITERAL token?
/// Used by the integration path to validate the first argument before
/// invoking the template parser.
pub fn arg_is_string_literal(src: &str) -> bool {
    let Some(tokens) = lex_fragment(src.trim()) else {
        return false;
    };
    let non_trivia: Vec<_> = tokens.iter().filter(|t| !t.is_trivia()).collect();
    non_trivia.len() == 1 && non_trivia[0].kind == SyntaxKind::STRING_LITERAL
}

/// Expand a `format!(args)` call into a Mighty source snippet.
///
/// `args` is the list of comma-split source slices the caller already
/// produced for the `MACRO_CALL` (see `parse_macro_call_token_tree`
/// in `mty-hir/src/lower/macros.rs`). The first slice must lex to a
/// single STRING_LITERAL.
///
/// On success returns a parenthesised block expression that evaluates
/// to a string. The block is wrapped so it can be used as a sub-expr
/// (e.g. `log(format!(...))`) without grammar surprises.
pub fn expand_format_call(args: &[&str]) -> Result<String, FormatExpandError> {
    if args.is_empty() {
        return Err(FormatExpandError::EmptyArgList);
    }
    let template_src = args[0].trim();
    let template =
        decode_string_literal(template_src).ok_or(FormatExpandError::NotAStringLiteral)?;
    // Defense in depth: confirm the source slice actually lexes as one
    // STRING_LITERAL token; catches a glued case like `"a" + "b"` which
    // `decode_string_literal` happens to accept (it strips outer quotes
    // off the trimmed slice).
    if !arg_is_string_literal(template_src) {
        return Err(FormatExpandError::NotAStringLiteral);
    }

    let pieces = parse_template(&template)?;

    // Count positional placeholders so we can arity-check the call.
    let pos_needed = pieces
        .iter()
        .filter(|p| matches!(p, FormatPiece::Positional { .. }))
        .count();
    let pos_given = args.len().saturating_sub(1);
    if pos_given < pos_needed {
        return Err(FormatExpandError::NotEnoughArgs {
            expected: pos_needed,
            given: pos_given,
        });
    }
    if pos_given > pos_needed {
        return Err(FormatExpandError::TooManyArgs {
            expected: pos_needed,
            given: pos_given,
        });
    }

    // Walk pieces, emitting a chain of `+` between string fragments.
    // Empty template short-circuits to `""`.
    if pieces.is_empty() {
        return Ok("\"\"".to_string());
    }

    let mut chunks: Vec<String> = Vec::with_capacity(pieces.len());
    let mut pos_idx = 0usize;
    for piece in &pieces {
        match piece {
            FormatPiece::Literal(s) => {
                chunks.push(format!("\"{}\"", escape_for_mty_literal(s)));
            }
            FormatPiece::Positional { kind } => {
                // args[0] is the template; positional args start at index 1.
                let arg_src = args[pos_idx + 1].trim();
                chunks.push(format!("({}).{}()", arg_src, kind.method()));
                pos_idx += 1;
            }
            FormatPiece::Named { ident, kind } => {
                chunks.push(format!("({}).{}()", ident, kind.method()));
            }
        }
    }

    // A single chunk needs no `+`. Multiple chunks: glue with `+`. We
    // start with `""` so the result type is always String even when the
    // first chunk is a method call that the inferencer might want to
    // double-check (`("x").to_str() + ...` already strings-out, but the
    // explicit `"" + ...` head also helps when the template starts with
    // a placeholder and the user wraps the call in `log(...)`).
    let body = if chunks.len() == 1 {
        chunks.remove(0)
    } else {
        // Build (((""+a)+b)+c) shape — Mighty's `+` on strings is
        // left-associative and concat semantics live in the runtime.
        let mut out = String::from("\"\"");
        for chunk in chunks {
            out = format!("({} + {})", out, chunk);
        }
        out
    };

    Ok(body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_template_is_empty() {
        let pieces = parse_template("").unwrap();
        assert!(pieces.is_empty());
    }

    #[test]
    fn parse_pure_literal() {
        let pieces = parse_template("hello, world").unwrap();
        assert_eq!(pieces, vec![FormatPiece::Literal("hello, world".into())]);
    }

    #[test]
    fn parse_single_positional() {
        let pieces = parse_template("x={}").unwrap();
        assert_eq!(
            pieces,
            vec![
                FormatPiece::Literal("x=".into()),
                FormatPiece::Positional {
                    kind: ConvKind::Display
                },
            ]
        );
    }

    #[test]
    fn parse_hex_via_spec() {
        // Hex conversion is the `:x` / `:X` spec, not a placeholder
        // shorthand. `{x}` / `{X}` are named-arg passthroughs, NOT
        // positional hex placeholders (Rust convention).
        let pieces = parse_template("0x{:x} 0x{:X}").unwrap();
        assert_eq!(
            pieces,
            vec![
                FormatPiece::Literal("0x".into()),
                FormatPiece::Positional {
                    kind: ConvKind::HexLower
                },
                FormatPiece::Literal(" 0x".into()),
                FormatPiece::Positional {
                    kind: ConvKind::HexUpper
                },
            ]
        );
    }

    #[test]
    fn parse_bare_named_idents() {
        // `{x}` and `{X}` are NAMED-arg passthroughs, not hex.
        let pieces = parse_template("{x} and {X}").unwrap();
        assert_eq!(
            pieces,
            vec![
                FormatPiece::Named {
                    ident: "x".into(),
                    kind: ConvKind::Display
                },
                FormatPiece::Literal(" and ".into()),
                FormatPiece::Named {
                    ident: "X".into(),
                    kind: ConvKind::Display
                },
            ]
        );
    }

    #[test]
    fn parse_named_with_hex_spec() {
        let pieces = parse_template("cell {x},{y} = {color:x}").unwrap();
        assert_eq!(
            pieces,
            vec![
                FormatPiece::Literal("cell ".into()),
                FormatPiece::Named {
                    ident: "x".into(),
                    kind: ConvKind::Display
                },
                FormatPiece::Literal(",".into()),
                FormatPiece::Named {
                    ident: "y".into(),
                    kind: ConvKind::Display
                },
                FormatPiece::Literal(" = ".into()),
                FormatPiece::Named {
                    ident: "color".into(),
                    kind: ConvKind::HexLower
                },
            ]
        );
    }

    #[test]
    fn parse_double_braces_escape() {
        let pieces = parse_template("{{}}").unwrap();
        assert_eq!(pieces, vec![FormatPiece::Literal("{}".into())]);
    }

    #[test]
    fn parse_unclosed_brace_errors() {
        let e = parse_template("hi {").unwrap_err();
        assert!(matches!(e, FormatExpandError::UnclosedBrace { .. }));
    }

    #[test]
    fn parse_lone_close_brace_errors() {
        let e = parse_template("hi }").unwrap_err();
        assert!(matches!(e, FormatExpandError::UnexpectedCloseBrace { .. }));
    }

    #[test]
    fn parse_width_spec_is_unsupported() {
        let e = parse_template("{:05}").unwrap_err();
        assert!(matches!(e, FormatExpandError::UnsupportedSpec { .. }));
    }

    #[test]
    fn parse_precision_spec_is_unsupported() {
        let e = parse_template("{:.3}").unwrap_err();
        assert!(matches!(e, FormatExpandError::UnsupportedSpec { .. }));
    }

    #[test]
    fn parse_align_spec_is_unsupported() {
        let e = parse_template("{:>10}").unwrap_err();
        assert!(matches!(e, FormatExpandError::UnsupportedSpec { .. }));
    }

    #[test]
    fn decode_string_literal_basic() {
        assert_eq!(
            decode_string_literal("\"hi\\nthere\"").as_deref(),
            Some("hi\nthere")
        );
    }

    #[test]
    fn arg_is_string_literal_accepts_simple_string() {
        assert!(arg_is_string_literal("\"hi\""));
    }

    #[test]
    fn arg_is_string_literal_rejects_concat() {
        assert!(!arg_is_string_literal("\"a\" + \"b\""));
    }

    #[test]
    fn expand_empty_template_is_empty_string() {
        let out = expand_format_call(&["\"\""]).unwrap();
        assert_eq!(out, "\"\"");
    }

    #[test]
    fn expand_no_args_errors() {
        let e = expand_format_call(&[]).unwrap_err();
        assert!(matches!(e, FormatExpandError::EmptyArgList));
    }

    #[test]
    fn expand_first_arg_must_be_literal() {
        let e = expand_format_call(&["x"]).unwrap_err();
        assert!(matches!(e, FormatExpandError::NotAStringLiteral));
    }

    #[test]
    fn expand_single_positional_arg() {
        let out = expand_format_call(&["\"score: {}\"", "score"]).unwrap();
        assert!(out.contains("(score).to_str()"), "got: {out}");
        assert!(out.contains("\"score: \""), "got: {out}");
    }

    #[test]
    fn expand_named_arg_passthrough() {
        let out = expand_format_call(&["\"score: {score}\""]).unwrap();
        assert!(out.contains("(score).to_str()"), "got: {out}");
    }

    #[test]
    fn expand_hex_lower_calls_to_hex_str() {
        let out = expand_format_call(&["\"{:x}\"", "255"]).unwrap();
        assert!(out.contains("to_hex_str"), "got: {out}");
        assert!(out.contains("(255).to_hex_str()"), "got: {out}");
    }

    #[test]
    fn expand_hex_upper_calls_to_hex_upper_str() {
        let out = expand_format_call(&["\"{:X}\"", "255"]).unwrap();
        assert!(out.contains("to_hex_upper_str"), "got: {out}");
    }

    #[test]
    fn expand_bare_x_is_named_arg() {
        // `{x}` is a named-arg passthrough — the expander emits
        // `(x).to_str()`, NOT a positional `(arg).to_hex_str()`.
        let out = expand_format_call(&["\"{x}\""]).unwrap();
        assert!(out.contains("(x).to_str()"), "got: {out}");
        assert!(!out.contains("to_hex_str"), "got: {out}");
    }

    #[test]
    fn expand_arity_too_few() {
        let e = expand_format_call(&["\"{} {}\"", "1"]).unwrap_err();
        assert!(
            matches!(
                e,
                FormatExpandError::NotEnoughArgs {
                    expected: 2,
                    given: 1
                }
            ),
            "got: {e:?}"
        );
    }

    #[test]
    fn expand_arity_too_many() {
        let e = expand_format_call(&["\"{}\"", "1", "2"]).unwrap_err();
        assert!(matches!(
            e,
            FormatExpandError::TooManyArgs {
                expected: 1,
                given: 2
            }
        ));
    }

    #[test]
    fn expand_double_brace_literal() {
        let out = expand_format_call(&["\"{{}}\""]).unwrap();
        // The expansion contains a string literal `{}`. Because we
        // escape on emit, the raw chars in the output source are `\"{}\"`.
        assert!(out.contains("\"{}\""), "got: {out}");
    }
}
