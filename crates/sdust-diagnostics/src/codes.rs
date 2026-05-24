//! Stable diagnostic codes. Once assigned, NEVER renumber.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiagCode(pub u16);

impl DiagCode {
    pub const fn new(n: u16) -> Self {
        DiagCode(n)
    }
    pub fn as_str(&self) -> String {
        format!("SD{:04}", self.0)
    }
}

// Lex/parse: SD0001..SD0999
pub const UNEXPECTED_TOKEN: DiagCode = DiagCode::new(1);
pub const UNTERMINATED_STRING: DiagCode = DiagCode::new(2);
pub const INVALID_ESCAPE: DiagCode = DiagCode::new(3);
pub const UNKNOWN_DURATION_UNIT: DiagCode = DiagCode::new(4);
pub const EXPECTED_ITEM: DiagCode = DiagCode::new(10);
pub const EXPECTED_EXPR: DiagCode = DiagCode::new(11);
pub const MISMATCHED_DELIMITER: DiagCode = DiagCode::new(12);
pub const DUPLICATE_ON_HANDLER: DiagCode = DiagCode::new(20);
pub const PUB_NEEDS_RETURN_TYPE: DiagCode = DiagCode::new(21);
pub const DEPTH_LIMIT_EXCEEDED: DiagCode = DiagCode::new(30);

// HIR: SD1001..SD1999
pub const UNRESOLVED_NAME: DiagCode = DiagCode::new(1001);
pub const USE_RESOLVES_TO_NOTHING: DiagCode = DiagCode::new(1002);

/// Returns a 2-4 sentence human-readable explanation for a diagnostic
/// code, suitable for `sdust explain SDxxxx`. Returns None for codes
/// outside the assigned ranges.
pub fn explain(code: DiagCode) -> Option<&'static str> {
    Some(match code.0 {
        1 => "SD0001: Unexpected token. The lexer or parser found a token \
              that doesn't fit the current grammar context. Check for typos, \
              missing punctuation, or a misplaced keyword.",
        2 => "SD0002: Unterminated string literal. A string starts with \" \
              but never closes before end-of-line or end-of-file. Add the \
              closing quote, or escape any embedded \" as \\\".",
        3 => "SD0003: Invalid escape sequence. The character after \\ in a \
              string or char literal is not a recognized escape. Valid \
              escapes include \\n, \\t, \\r, \\\\, \\\", \\', \\x{HH}, and \
              \\u{HHHH}.",
        4 => "SD0004: Unknown duration unit. Stardust duration literals use \
              one of `ns`, `us`, `ms`, `s`, `m`, `h` as the trailing unit. \
              For size literals see SD0001 (`KiB`/`MiB`/`GiB` binary, `k`/`M` \
              decimal).",
        10 => "SD0010: Expected an item. At the top level (or inside a mod), \
               the parser expected one of: fn, struct, enum, type, use, mod, \
               package, agent, protocol, supervisor, extern, export, impl, \
               trait, const, macro.",
        11 => "SD0011: Expected an expression. The parser reached a position \
               where an expression must appear but found something else \
               (such as a closing delimiter or a statement keyword).",
        12 => "SD0012: Mismatched delimiter. An opening `(`, `[`, or `{` was \
               not paired with the matching closing delimiter, or they were \
               crossed.",
        20 => "SD0020: Duplicate `on` handler. An agent body declared two \
               handlers for the same protocol message. Each protocol message \
               may have at most one `on Message` handler per agent.",
        21 => "SD0021: `pub` function needs a return type. Public functions \
               must declare an explicit return type (`-> T`) so callers in \
               other modules can rely on the signature. Add `-> Unit` if the \
               function returns nothing.",
        30 => "SD0030: Recursion depth limit exceeded. The parser nested \
               deeper than the configured limit. This usually indicates \
               adversarial or accidentally pathological input; refactor the \
               source to reduce nesting.",
        1001 => "SD1001: Unresolved name. The HIR lowerer could not resolve \
                 a name reference to any binding in scope. Check the spelling \
                 and ensure the binding's `use` or declaration is visible.",
        1002 => "SD1002: `use` resolves to nothing. The path on the right of \
                 `use` does not name any importable item. Verify the package \
                 and module path; remember that paths use `.` as the \
                 separator.",
        _ => return None,
    })
}
