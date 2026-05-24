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

// Type checker: SD2001..SD2099
pub const TYPE_MISMATCH: DiagCode = DiagCode::new(2001);
pub const UNRESOLVED_TYPE: DiagCode = DiagCode::new(2002);
pub const CANNOT_INFER_TYPE: DiagCode = DiagCode::new(2003);
pub const WRONG_GENERIC_ARITY: DiagCode = DiagCode::new(2004);
pub const WRONG_ARG_COUNT: DiagCode = DiagCode::new(2005);
pub const UNKNOWN_FIELD: DiagCode = DiagCode::new(2006);
pub const UNKNOWN_METHOD: DiagCode = DiagCode::new(2007);
pub const NOT_CALLABLE: DiagCode = DiagCode::new(2008);
pub const UNKNOWN_VARIANT: DiagCode = DiagCode::new(2009);
pub const QUESTION_OUTSIDE_RESULT: DiagCode = DiagCode::new(2010);
pub const QUESTION_ERROR_MISMATCH: DiagCode = DiagCode::new(2011);
pub const WRONG_VARIANT_ARITY: DiagCode = DiagCode::new(2012);
pub const MISSING_STRUCT_FIELD: DiagCode = DiagCode::new(2013);
pub const DUPLICATE_STRUCT_FIELD: DiagCode = DiagCode::new(2014);
pub const NON_EXHAUSTIVE_MATCH: DiagCode = DiagCode::new(2015);
pub const UNREACHABLE_MATCH_ARM: DiagCode = DiagCode::new(2016);
pub const BINOP_TYPE_MISMATCH: DiagCode = DiagCode::new(2017);
pub const IF_BRANCH_MISMATCH: DiagCode = DiagCode::new(2018);
pub const RETURN_TYPE_MISMATCH: DiagCode = DiagCode::new(2019);
pub const PUB_PARAM_NEEDS_TYPE: DiagCode = DiagCode::new(2020);
pub const UNRESOLVED_VALUE: DiagCode = DiagCode::new(2021);
pub const NOT_A_STRUCT: DiagCode = DiagCode::new(2022);
pub const GENERIC_ARG_MISMATCH: DiagCode = DiagCode::new(2023);
pub const LAMBDA_ARITY_MISMATCH: DiagCode = DiagCode::new(2024);
pub const CANNOT_TAKE_REF: DiagCode = DiagCode::new(2025);
pub const PROTOCOL_MSG_UNKNOWN: DiagCode = DiagCode::new(2026);

// Borrow checker: SD3001..SD3099
pub const USE_AFTER_MOVE: DiagCode = DiagCode::new(3001);
pub const MOVE_OUT_OF_BORROW: DiagCode = DiagCode::new(3002);
pub const BORROW_AFTER_MOVE: DiagCode = DiagCode::new(3003);
pub const MUT_BORROW_WHILE_SHARED: DiagCode = DiagCode::new(3004);
pub const SHARED_BORROW_WHILE_MUT: DiagCode = DiagCode::new(3005);
pub const TWO_MUT_BORROWS: DiagCode = DiagCode::new(3006);
pub const BORROW_OUTLIVES_OWNER: DiagCode = DiagCode::new(3007);
pub const CANNOT_MOVE_BORROWED: DiagCode = DiagCode::new(3008);
pub const MOVE_OUT_OF_REF: DiagCode = DiagCode::new(3009);
pub const ARENA_ESCAPE: DiagCode = DiagCode::new(3010);
pub const NON_SENDABLE_MESSAGE_ARG: DiagCode = DiagCode::new(3011);
pub const DROP_IN_CONST_CONTEXT: DiagCode = DiagCode::new(3012);
pub const MUT_BORROW_OF_IMMUT_LOCAL: DiagCode = DiagCode::new(3013);
pub const ASSIGN_TO_IMMUT_LOCAL: DiagCode = DiagCode::new(3014);
pub const USE_OF_UNINITIALIZED: DiagCode = DiagCode::new(3015);

/// Returns a 2-4 sentence human-readable explanation for a diagnostic
/// code, suitable for `sdust explain SDxxxx`. Returns None for codes
/// outside the assigned ranges.
pub fn explain(code: DiagCode) -> Option<&'static str> {
    Some(match code.0 {
        1 => {
            "SD0001: Unexpected token. The lexer or parser found a token \
              that doesn't fit the current grammar context. Check for typos, \
              missing punctuation, or a misplaced keyword."
        }
        2 => {
            "SD0002: Unterminated string literal. A string starts with \" \
              but never closes before end-of-line or end-of-file. Add the \
              closing quote, or escape any embedded \" as \\\"."
        }
        3 => {
            "SD0003: Invalid escape sequence. The character after \\ in a \
              string or char literal is not a recognized escape. Valid \
              escapes include \\n, \\t, \\r, \\\\, \\\", \\', \\x{HH}, and \
              \\u{HHHH}."
        }
        4 => {
            "SD0004: Unknown duration unit. Stardust duration literals use \
              one of `ns`, `us`, `ms`, `s`, `m`, `h` as the trailing unit. \
              For size literals see SD0001 (`KiB`/`MiB`/`GiB` binary, `k`/`M` \
              decimal)."
        }
        10 => {
            "SD0010: Expected an item. At the top level (or inside a mod), \
               the parser expected one of: fn, struct, enum, type, use, mod, \
               package, agent, protocol, supervisor, extern, export, impl, \
               trait, const, macro."
        }
        11 => {
            "SD0011: Expected an expression. The parser reached a position \
               where an expression must appear but found something else \
               (such as a closing delimiter or a statement keyword)."
        }
        12 => {
            "SD0012: Mismatched delimiter. An opening `(`, `[`, or `{` was \
               not paired with the matching closing delimiter, or they were \
               crossed."
        }
        20 => {
            "SD0020: Duplicate `on` handler. An agent body declared two \
               handlers for the same protocol message. Each protocol message \
               may have at most one `on Message` handler per agent."
        }
        21 => {
            "SD0021: `pub` function needs a return type. Public functions \
               must declare an explicit return type (`-> T`) so callers in \
               other modules can rely on the signature. Add `-> Unit` if the \
               function returns nothing."
        }
        30 => {
            "SD0030: Recursion depth limit exceeded. The parser nested \
               deeper than the configured limit. This usually indicates \
               adversarial or accidentally pathological input; refactor the \
               source to reduce nesting."
        }
        1001 => {
            "SD1001: Unresolved name. The HIR lowerer could not resolve \
                 a name reference to any binding in scope. Check the spelling \
                 and ensure the binding's `use` or declaration is visible."
        }
        1002 => {
            "SD1002: `use` resolves to nothing. The path on the right of \
                 `use` does not name any importable item. Verify the package \
                 and module path; remember that paths use `.` as the \
                 separator."
        }
        2001 => {
            "SD2001: Type mismatch. An expression's type does not match the \
                 type required by context. The diagnostic shows the expected \
                 type and the actual type; check the call site or annotation."
        }
        2002 => {
            "SD2002: Unresolved type. The named type does not exist in scope. \
                 Verify the spelling, the relevant `use` declaration, and \
                 whether the type lives inside a module path (`foo.bar.Type`)."
        }
        2003 => {
            "SD2003: Cannot infer type. The type checker could not determine \
                 a binding's type from context. Add an explicit annotation \
                 (`let x: T = ...`) or provide more usage."
        }
        2004 => {
            "SD2004: Wrong number of generic arguments. The type or function \
                 expects a specific number of `[T, ...]` arguments. Add or \
                 remove arguments to match the declaration."
        }
        2005 => {
            "SD2005: Wrong number of arguments. The function expects a specific \
                 arity; the call site provides a different number. Check the \
                 function's declaration."
        }
        2006 => {
            "SD2006: Unknown field. The named field does not exist on the \
                 struct. Check spelling and the struct declaration."
        }
        2007 => {
            "SD2007: Unknown method. The named method does not exist on the \
                 receiver type. Method resolution searches `impl` blocks and \
                 a small built-in table; nothing matched."
        }
        2008 => {
            "SD2008: Not callable. The value being applied does not have a \
                 function type. Check that it refers to a function or a \
                 callable agent constructor."
        }
        2009 => {
            "SD2009: Unknown variant. The named variant does not exist on the \
                 enum. Verify the spelling and the enum declaration."
        }
        2010 => {
            "SD2010: `?` outside Result-returning function. The `?` operator \
                 requires the enclosing function's return type to be \
                 `Result[_, _]`. Change the signature, or replace `?` with an \
                 explicit `match`."
        }
        2011 => {
            "SD2011: `?` error-type mismatch. The error type of `?`'s operand \
                 must match (or coerce to) the enclosing function's error \
                 type. Slice 3 requires an exact match."
        }
        2012 => {
            "SD2012: Wrong variant arity. The enum variant expects a specific \
                 number of payload values; the call site provides a different \
                 count."
        }
        2013 => {
            "SD2013: Missing struct field. The struct literal omits a \
                 required field. Add the field, or (if the field has a \
                 default) use shorthand notation."
        }
        2014 => {
            "SD2014: Duplicate struct field. The struct literal lists the same \
                 field twice. Remove the duplicate."
        }
        2015 => {
            "SD2015: Non-exhaustive match. The match does not cover every \
                 possible value of the scrutinee. Slice 4 made this an \
                 error; add the missing arm(s) or a wildcard `_ => ...`."
        }
        2016 => {
            "SD2016: Unreachable match arm (warning). A later arm cannot be \
                 reached because an earlier arm always matches first."
        }
        2017 => {
            "SD2017: Binary operator type mismatch. The operator is not defined \
                 for the given operand types. Slice 3 supports the standard \
                 numeric/boolean operators only."
        }
        2018 => {
            "SD2018: If/else branch type mismatch. The two branches of an `if` \
                 produce incompatible types. Unify them, or remove the value \
                 use of the `if`."
        }
        2019 => {
            "SD2019: Return-type mismatch. The function declares one return \
                 type but the body produces a different one."
        }
        2020 => {
            "SD2020: Public function parameter requires explicit type. `pub` \
                 functions must declare every parameter's type so callers in \
                 other modules can rely on the signature."
        }
        2021 => {
            "SD2021: Unresolved value. The named identifier does not refer to \
                 any value in scope. Check the spelling and visibility."
        }
        2022 => {
            "SD2022: Not a struct. The value cannot be initialized with a \
                 struct literal because it is not a struct type."
        }
        2023 => {
            "SD2023: Generic argument-kind mismatch. The type argument's kind \
                 does not match the expected parameter kind (e.g. supplied a \
                 lifetime where a type was expected)."
        }
        2024 => {
            "SD2024: Lambda arity mismatch. The lambda's parameter count \
                 differs from the expected function type."
        }
        2025 => {
            "SD2025: Cannot take reference. The expression is not a place \
                 (l-value), so `&` cannot apply."
        }
        2026 => {
            "SD2026: Protocol message unknown (warning). An `on Msg(...)` \
                 handler refers to a message that no implemented protocol \
                 declares. Handler params will be typed as fresh inference \
                 variables; declare the message in a protocol or use \
                 protocol composition to bring it in."
        }
        3001 => {
            "SD3001: Use after move. The value was moved earlier and cannot \
                 be used again. Add `.clone()` before the move site if you \
                 need both, or restructure the code so each owner has a \
                 single move."
        }
        3002 => {
            "SD3002: Move out of borrowed value. A reference (`&` or \
                 `&mut`) to the value is still live, so the value cannot be \
                 moved. Wait for the borrow's scope to end, or copy the \
                 value."
        }
        3003 => {
            "SD3003: Borrow after move. The value was already moved and no \
                 longer owns its storage; a borrow is not permitted."
        }
        3004 => {
            "SD3004: Mutable borrow while shared borrows exist. Stardust \
                 forbids creating a `&mut T` while any `&T` to the same \
                 value is live."
        }
        3005 => {
            "SD3005: Shared borrow while a mutable borrow exists. Stardust \
                 forbids creating a `&T` while a `&mut T` to the same \
                 value is live."
        }
        3006 => {
            "SD3006: Two mutable borrows of the same value. Only one \
                 `&mut T` may exist at a time."
        }
        3007 => {
            "SD3007: Borrow outlives its owner. The borrowed value's \
                 owner goes out of scope before the borrow's lexical \
                 region ends."
        }
        3008 => {
            "SD3008: Cannot move a borrowed value. A reference to the \
                 value is still live."
        }
        3009 => {
            "SD3009: Cannot move out of a reference. Dereferencing `&T` \
                 or `&mut T` does not transfer ownership; you may only \
                 read or write through it."
        }
        3010 => {
            "SD3010: Value escapes its arena. A value bound inside an \
                 `arena name { ... }` cannot leave the arena's scope unless \
                 explicitly promoted via `move` to an ancestor scope."
        }
        3011 => {
            "SD3011: Non-Sendable cross-agent message argument. Every \
                 argument to a `!Msg(...)` or `?Msg(...)` call must be \
                 Sendable: a Copy type or an owned value (references and \
                 managed handles cannot cross agent boundaries)."
        }
        3012 => {
            "SD3012: Drop in const context. A value requiring deterministic \
                 cleanup cannot live in a `const` slot."
        }
        3013 => {
            "SD3013: Cannot mutably borrow an immutable local. The binding \
                 was declared without `mut`."
        }
        3014 => {
            "SD3014: Assignment to immutable local. The binding was \
                 declared without `mut`."
        }
        3015 => {
            "SD3015: Use of uninitialised binding. The binding was \
                 declared but never assigned before its first read."
        }
        _ => return None,
    })
}
