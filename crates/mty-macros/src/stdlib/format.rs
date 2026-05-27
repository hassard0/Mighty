//! The `format!` builtin macro (v0.24 Track B + v0.25 Track D extensions).
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
//! ## Supported spec subset (v0.25)
//!
//! v0.24 Track B shipped the conversion sigils:
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
//! v0.25 Track D extends this with layout flags:
//!
//! | Spec        | Behaviour                                            |
//! |-------------|------------------------------------------------------|
//! | `{:5}`      | minimum width 5 (right-aligned default for numbers)  |
//! | `{:05}`     | width 5 + zero-padding                               |
//! | `{:<5}`     | left-align to width 5                                |
//! | `{:>5}`     | right-align to width 5                               |
//! | `{:^5}`     | center-align to width 5                              |
//! | `{:*<5}`    | fill char `*` + left-align to width 5                |
//! | `{:.3}`     | precision 3 (floats: decimal places; strings: max)   |
//! | `{:+}`      | always show sign for numbers                         |
//! | `{:#x}`     | alternate hex (prefix `0x`)                          |
//! | `{:#X}`     | alternate HEX (prefix `0x`)                          |
//! | `{:#b}`     | alternate binary (prefix `0b`)                       |
//! | `{:#o}`     | alternate octal (prefix `0o`)                        |
//! | `{:b}`      | binary (no prefix)                                   |
//! | `{:o}`      | octal (no prefix)                                    |
//!
//! Combined specs work in the canonical order
//! `[fill][align][sign][#][0][width][.precision][type]`, matching Rust:
//!
//! | Spec        | Renders                                              |
//! |-------------|------------------------------------------------------|
//! | `{:#05x}`   | `0x0ff` for 0xff (alt + zero + width + hex)          |
//! | `{:+05}`    | `+0001` for 1 (sign + zero + width)                  |
//! | `{:>10.3}`  | `"     3.142"` for 3.14159 (align + width + precision) |
//!
//! Per the Rust convention, `{x}` is a *named-arg passthrough* (refers
//! to in-scope `x`), NOT a positional hex sigil. Use `{:x}` for
//! positional hex, `{x:x}` for "named x rendered as hex".
//!
//! ## Deferred to v0.26
//!
//! - Indexed positional `{0} {1} {0}` (argument reuse by index)
//! - Dynamic width/precision via argument `{:1$}`, `{:.0$}`, `{:.*}`

use crate::token::lex_fragment;
use mty_syntax::SyntaxKind;

/// One concrete piece of a parsed format template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatPiece {
    /// Literal source text (`{{` and `}}` already decoded to `{` / `}`).
    Literal(String),
    /// A placeholder that consumes one of the trailing positional args.
    /// `spec` carries both the conversion sigil and the layout flags.
    Positional { spec: FormatSpec },
    /// A `{name}` placeholder that resolves to an in-scope identifier.
    /// The expander emits `name.<conv>()` directly — no positional arg
    /// is consumed.
    Named { ident: String, spec: FormatSpec },
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
    /// `{:b}` / `{name:b}` — `.to_bin_str()`. v0.25 Track D.
    Binary,
    /// `{:o}` / `{name:o}` — `.to_oct_str()`. v0.25 Track D.
    Octal,
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
            ConvKind::Binary => "to_bin_str",
            ConvKind::Octal => "to_oct_str",
        }
    }
}

/// Alignment within a width-padded field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// `{:<N}` — pad on the right (default for strings).
    Left,
    /// `{:>N}` — pad on the left (default for numbers).
    Right,
    /// `{:^N}` — pad symmetrically.
    Center,
}

impl Alignment {
    /// Canonical name of the variant the runtime helper accepts.
    /// Surfaced as a string the runtime can pattern-match without
    /// needing to import the enum type.
    pub fn as_runtime_str(self) -> &'static str {
        match self {
            Alignment::Left => "left",
            Alignment::Right => "right",
            Alignment::Center => "center",
        }
    }
}

/// Full parsed format spec — captures the v0.24 conversion sigil PLUS
/// the v0.25 layout flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatSpec {
    /// Conversion method (`{:x}`, `{:?}`, …).
    pub kind: ConvKind,
    /// Fill character (default `' '`, or `'0'` when zero-pad flag set).
    pub fill: char,
    /// Alignment, if the spec supplies one explicitly.
    pub align: Option<Alignment>,
    /// `+` flag — always show sign for numbers.
    pub sign_plus: bool,
    /// `#` flag — alternate form (prefix hex/oct/bin).
    pub alternate: bool,
    /// `0` zero-pad flag (implies fill = `'0'` and align = Right when
    /// no explicit fill/align supplied).
    pub zero_pad: bool,
    /// Minimum width.
    pub width: Option<u32>,
    /// Precision (float decimals, string max chars).
    pub precision: Option<u32>,
}

impl FormatSpec {
    /// The bare-`{}` default spec — Display conversion, no layout.
    pub fn display() -> Self {
        FormatSpec {
            kind: ConvKind::Display,
            fill: ' ',
            align: None,
            sign_plus: false,
            alternate: false,
            zero_pad: false,
            width: None,
            precision: None,
        }
    }

    /// True if the spec has no layout flags beyond the conversion
    /// sigil — i.e. the v0.24 baseline path suffices. The expander
    /// uses this to avoid materialising a `to_str_spec`/`pad_str`-style
    /// call chain when a plain `to_str()` is enough.
    pub fn is_bare_conversion(&self) -> bool {
        self.fill == ' '
            && self.align.is_none()
            && !self.sign_plus
            && !self.alternate
            && !self.zero_pad
            && self.width.is_none()
            && self.precision.is_none()
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
    /// Spec like `{0}` (indexed positional) or `{:1$}` (dynamic width
    /// via arg) that the v0.25 parser cannot interpret. Tracked for v0.26.
    UnsupportedSpec { spec: String, position: usize },
    /// Width digit run does not parse as `u32`.
    BadWidth { spec: String, position: usize },
    /// Precision digit run does not parse as `u32` or is missing digits.
    BadPrecision { spec: String, position: usize },
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
                "format spec `{{:{spec}}}` at position {position} is not supported (indexed positional `{{0}}` and dynamic width/precision land in v0.26)"
            ),
            FormatExpandError::BadWidth { spec, position } => write!(
                f,
                "format spec `{{:{spec}}}` at position {position} has a malformed width (must be a 32-bit unsigned integer literal)"
            ),
            FormatExpandError::BadPrecision { spec, position } => write!(
                f,
                "format spec `{{:{spec}}}` at position {position} has a malformed precision (must be a 32-bit unsigned integer literal)"
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
                if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    lit.push('{');
                    i += 2;
                    continue;
                }
                let start = i;
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != b'}' {
                    j += 1;
                }
                if j >= bytes.len() {
                    return Err(FormatExpandError::UnclosedBrace { position: start });
                }
                if !lit.is_empty() {
                    pieces.push(FormatPiece::Literal(std::mem::take(&mut lit)));
                }
                let inner = &template[i + 1..j];
                pieces.push(parse_placeholder(inner, start)?);
                i = j + 1;
            }
            b'}' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                    lit.push('}');
                    i += 2;
                    continue;
                }
                return Err(FormatExpandError::UnexpectedCloseBrace { position: i });
            }
            _ => {
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
/// `"name:x"`, `":>05x"`, `":.3"`). `start_pos` is the byte index of
/// the opening `{` in the outer template, used for diagnostics.
fn parse_placeholder(inner: &str, start_pos: usize) -> Result<FormatPiece, FormatExpandError> {
    let (head, spec_opt) = match inner.find(':') {
        Some(idx) => (&inner[..idx], Some(&inner[idx + 1..])),
        None => (inner, None),
    };

    // Defer: indexed positional `{0}` is intentionally not supported in v0.25.
    if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) {
        return Err(FormatExpandError::UnsupportedSpec {
            spec: head.to_string(),
            position: start_pos,
        });
    }

    let spec = match spec_opt {
        None | Some("") => FormatSpec::display(),
        Some(raw) => parse_spec(raw, start_pos)?,
    };

    if head.is_empty() {
        Ok(FormatPiece::Positional { spec })
    } else {
        if !is_identifier(head) {
            return Err(FormatExpandError::UnsupportedSpec {
                spec: head.to_string(),
                position: start_pos,
            });
        }
        Ok(FormatPiece::Named {
            ident: head.to_string(),
            spec,
        })
    }
}

/// Parse the post-`:` spec body. Implements the canonical Rust spec
/// grammar: `[[fill]align][sign][#][0][width][.precision][type]`.
///
/// Returns [`FormatExpandError::UnsupportedSpec`] for shapes the
/// v0.25 expander cannot lower (dynamic width via arg, etc.).
fn parse_spec(raw: &str, start_pos: usize) -> Result<FormatSpec, FormatExpandError> {
    let mut spec = FormatSpec::display();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0usize;

    // [[fill]align] — fill is any char, align is one of `<>^`.
    if chars.len() >= 2 && is_align_char(chars[1]) {
        spec.fill = chars[0];
        spec.align = Some(align_from_char(chars[1]));
        i += 2;
    } else if !chars.is_empty() && is_align_char(chars[0]) {
        spec.align = Some(align_from_char(chars[0]));
        i += 1;
    }

    // [sign]
    if i < chars.len() && chars[i] == '+' {
        spec.sign_plus = true;
        i += 1;
    }

    // [#] alternate form
    if i < chars.len() && chars[i] == '#' {
        spec.alternate = true;
        i += 1;
    }

    // [0] zero-pad
    if i < chars.len() && chars[i] == '0' {
        spec.zero_pad = true;
        if spec.fill == ' ' && spec.align.is_none() {
            spec.fill = '0';
            spec.align = Some(Alignment::Right);
        }
        i += 1;
    }

    // [width] — run of digits.
    if i < chars.len() && chars[i].is_ascii_digit() {
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        // Dynamic width `5$` is deferred.
        if i < chars.len() && chars[i] == '$' {
            return Err(FormatExpandError::UnsupportedSpec {
                spec: raw.to_string(),
                position: start_pos,
            });
        }
        let width_str: String = chars[start..i].iter().collect();
        let width = width_str
            .parse::<u32>()
            .map_err(|_| FormatExpandError::BadWidth {
                spec: raw.to_string(),
                position: start_pos,
            })?;
        spec.width = Some(width);
    } else if i < chars.len() && chars[i] == '*' {
        // Dynamic width `*`. Deferred.
        return Err(FormatExpandError::UnsupportedSpec {
            spec: raw.to_string(),
            position: start_pos,
        });
    }

    // [.precision] — `.<digits>` or `.*`.
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        if i < chars.len() && chars[i] == '*' {
            return Err(FormatExpandError::UnsupportedSpec {
                spec: raw.to_string(),
                position: start_pos,
            });
        }
        if i >= chars.len() || !chars[i].is_ascii_digit() {
            return Err(FormatExpandError::BadPrecision {
                spec: raw.to_string(),
                position: start_pos,
            });
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let prec_str: String = chars[start..i].iter().collect();
        if i < chars.len() && chars[i] == '$' {
            return Err(FormatExpandError::UnsupportedSpec {
                spec: raw.to_string(),
                position: start_pos,
            });
        }
        let precision = prec_str
            .parse::<u32>()
            .map_err(|_| FormatExpandError::BadPrecision {
                spec: raw.to_string(),
                position: start_pos,
            })?;
        spec.precision = Some(precision);
    }

    // [type]
    if i < chars.len() {
        let ty = chars[i];
        i += 1;
        spec.kind = match ty {
            'x' => ConvKind::HexLower,
            'X' => ConvKind::HexUpper,
            '?' => ConvKind::Debug,
            'b' => ConvKind::Binary,
            'o' => ConvKind::Octal,
            _ => {
                return Err(FormatExpandError::UnsupportedSpec {
                    spec: raw.to_string(),
                    position: start_pos,
                });
            }
        };
    }

    if i < chars.len() {
        return Err(FormatExpandError::UnsupportedSpec {
            spec: raw.to_string(),
            position: start_pos,
        });
    }

    Ok(spec)
}

fn is_align_char(c: char) -> bool {
    matches!(c, '<' | '>' | '^')
}

fn align_from_char(c: char) -> Alignment {
    match c {
        '<' => Alignment::Left,
        '>' => Alignment::Right,
        '^' => Alignment::Center,
        _ => unreachable!("is_align_char guarded"),
    }
}

/// True if `s` is a single Mighty identifier (ASCII letter/underscore
/// followed by letters/digits/underscores). Only plain identifiers
/// participate in named-arg passthrough.
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

/// Render the Mighty snippet for a single non-literal placeholder.
/// The receiver expression `recv_src` is the user's source text
/// (e.g. `score` or `1 + 2`); this fn wraps it in the conversion
/// method call plus, when layout flags are set, follow-on
/// `to_<kind>_spec(...)` / `.pad_str(...)` calls.
fn render_placeholder(recv_src: &str, spec: &FormatSpec) -> String {
    if spec.is_bare_conversion() {
        return format!("({}).{}()", recv_src, spec.kind.method());
    }
    let conv = format!(
        "({}).{}_spec({}, {}, {})",
        recv_src,
        spec.kind.method(),
        bool_lit(spec.sign_plus),
        bool_lit(spec.alternate),
        precision_arg(spec),
    );
    if spec.width.is_none() && spec.align.is_none() && spec.fill == ' ' && !spec.zero_pad {
        return conv;
    }
    let align_str = match spec.align {
        Some(a) => a.as_runtime_str(),
        None => "default",
    };
    format!(
        "({}).pad_str({}, '{}', \"{}\")",
        conv,
        spec.width.unwrap_or(0),
        escape_char_for_mty(spec.fill),
        align_str,
    )
}

fn bool_lit(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

fn precision_arg(spec: &FormatSpec) -> String {
    // `u32::MAX` (4294967295) is the "no precision" sentinel — picked
    // because no real format spec needs that many digits. Keep in sync
    // with `mty_stdlib::fmt::PRECISION_NONE`.
    match spec.precision {
        Some(p) => p.to_string(),
        None => "4294967295".to_string(),
    }
}

/// Escape a fill char for embedding in a Mighty `'x'` char literal.
fn escape_char_for_mty(c: char) -> String {
    match c {
        '\\' => "\\\\".into(),
        '\'' => "\\'".into(),
        '\n' => "\\n".into(),
        '\t' => "\\t".into(),
        '\r' => "\\r".into(),
        _ => c.to_string(),
    }
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
    if !arg_is_string_literal(template_src) {
        return Err(FormatExpandError::NotAStringLiteral);
    }

    let pieces = parse_template(&template)?;

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
            FormatPiece::Positional { spec } => {
                let arg_src = args[pos_idx + 1].trim();
                chunks.push(render_placeholder(arg_src, spec));
                pos_idx += 1;
            }
            FormatPiece::Named { ident, spec } => {
                chunks.push(render_placeholder(ident, spec));
            }
        }
    }

    let body = if chunks.len() == 1 {
        chunks.remove(0)
    } else {
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
                    spec: FormatSpec::display(),
                },
            ]
        );
    }

    #[test]
    fn parse_hex_via_spec() {
        let pieces = parse_template("0x{:x} 0x{:X}").unwrap();
        match &pieces[1] {
            FormatPiece::Positional { spec } => assert_eq!(spec.kind, ConvKind::HexLower),
            other => panic!("expected Positional, got {other:?}"),
        }
        match &pieces[3] {
            FormatPiece::Positional { spec } => assert_eq!(spec.kind, ConvKind::HexUpper),
            other => panic!("expected Positional, got {other:?}"),
        }
    }

    #[test]
    fn parse_bare_named_idents() {
        let pieces = parse_template("{x} and {X}").unwrap();
        match &pieces[0] {
            FormatPiece::Named { ident, spec } => {
                assert_eq!(ident, "x");
                assert_eq!(spec.kind, ConvKind::Display);
            }
            other => panic!("expected Named, got {other:?}"),
        }
        match &pieces[2] {
            FormatPiece::Named { ident, spec } => {
                assert_eq!(ident, "X");
                assert_eq!(spec.kind, ConvKind::Display);
            }
            other => panic!("expected Named, got {other:?}"),
        }
    }

    #[test]
    fn parse_named_with_hex_spec() {
        let pieces = parse_template("cell {x},{y} = {color:x}").unwrap();
        match &pieces[5] {
            FormatPiece::Named { ident, spec } => {
                assert_eq!(ident, "color");
                assert_eq!(spec.kind, ConvKind::HexLower);
            }
            other => panic!("expected Named, got {other:?}"),
        }
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
    fn parse_width_basic() {
        let pieces = parse_template("{:5}").unwrap();
        match &pieces[0] {
            FormatPiece::Positional { spec } => {
                assert_eq!(spec.width, Some(5));
                assert_eq!(spec.kind, ConvKind::Display);
                assert!(!spec.zero_pad);
            }
            other => panic!("expected Positional, got {other:?}"),
        }
    }

    #[test]
    fn parse_zero_pad_width() {
        let pieces = parse_template("{:05}").unwrap();
        match &pieces[0] {
            FormatPiece::Positional { spec } => {
                assert_eq!(spec.width, Some(5));
                assert!(spec.zero_pad);
                assert_eq!(spec.fill, '0');
                assert_eq!(spec.align, Some(Alignment::Right));
            }
            other => panic!("expected Positional, got {other:?}"),
        }
    }

    #[test]
    fn parse_align_only() {
        for (raw, want) in [
            ("<5", Alignment::Left),
            (">5", Alignment::Right),
            ("^5", Alignment::Center),
        ] {
            let template = format!("{{:{raw}}}");
            let pieces = parse_template(&template).unwrap();
            match &pieces[0] {
                FormatPiece::Positional { spec } => {
                    assert_eq!(spec.align, Some(want));
                    assert_eq!(spec.width, Some(5));
                }
                other => panic!("expected Positional, got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_fill_char_with_align() {
        let pieces = parse_template("{:*<5}").unwrap();
        match &pieces[0] {
            FormatPiece::Positional { spec } => {
                assert_eq!(spec.fill, '*');
                assert_eq!(spec.align, Some(Alignment::Left));
                assert_eq!(spec.width, Some(5));
            }
            other => panic!("expected Positional, got {other:?}"),
        }
    }

    #[test]
    fn parse_precision() {
        let pieces = parse_template("{:.3}").unwrap();
        match &pieces[0] {
            FormatPiece::Positional { spec } => {
                assert_eq!(spec.precision, Some(3));
                assert_eq!(spec.kind, ConvKind::Display);
            }
            other => panic!("expected Positional, got {other:?}"),
        }
    }

    #[test]
    fn parse_sign_plus() {
        let pieces = parse_template("{:+}").unwrap();
        match &pieces[0] {
            FormatPiece::Positional { spec } => {
                assert!(spec.sign_plus);
            }
            other => panic!("expected Positional, got {other:?}"),
        }
    }

    #[test]
    fn parse_alternate_hex() {
        let pieces = parse_template("{:#x}").unwrap();
        match &pieces[0] {
            FormatPiece::Positional { spec } => {
                assert!(spec.alternate);
                assert_eq!(spec.kind, ConvKind::HexLower);
            }
            other => panic!("expected Positional, got {other:?}"),
        }
    }

    #[test]
    fn parse_combined_alt_zero_width_hex() {
        let pieces = parse_template("{:#05x}").unwrap();
        match &pieces[0] {
            FormatPiece::Positional { spec } => {
                assert!(spec.alternate);
                assert!(spec.zero_pad);
                assert_eq!(spec.width, Some(5));
                assert_eq!(spec.kind, ConvKind::HexLower);
            }
            other => panic!("expected Positional, got {other:?}"),
        }
    }

    #[test]
    fn parse_indexed_positional_unsupported() {
        let e = parse_template("{0}").unwrap_err();
        assert!(matches!(e, FormatExpandError::UnsupportedSpec { .. }));
    }

    #[test]
    fn parse_dynamic_width_unsupported() {
        let e = parse_template("{:1$}").unwrap_err();
        assert!(matches!(e, FormatExpandError::UnsupportedSpec { .. }));
    }

    #[test]
    fn parse_dynamic_precision_unsupported() {
        let e = parse_template("{:.*}").unwrap_err();
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
        assert!(out.contains("\"{}\""), "got: {out}");
    }

    #[test]
    fn expand_width_emits_pad_call() {
        let out = expand_format_call(&["\"{:5}\"", "n"]).unwrap();
        assert!(out.contains("pad_str(5"), "got: {out}");
    }

    #[test]
    fn expand_zero_pad_emits_zero_fill() {
        let out = expand_format_call(&["\"{:05}\"", "n"]).unwrap();
        assert!(out.contains("pad_str(5"), "got: {out}");
        assert!(out.contains("'0'"), "got: {out}");
        assert!(out.contains("\"right\""), "got: {out}");
    }

    #[test]
    fn expand_alt_hex_emits_alternate_flag() {
        let out = expand_format_call(&["\"{:#x}\"", "255"]).unwrap();
        assert!(out.contains("to_hex_str_spec"), "got: {out}");
        assert!(out.contains("false, true"), "got: {out}");
    }
}
