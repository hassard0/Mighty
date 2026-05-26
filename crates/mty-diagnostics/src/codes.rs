//! Stable diagnostic codes. Once assigned, NEVER renumber.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiagCode(pub u16);

impl DiagCode {
    pub const fn new(n: u16) -> Self {
        DiagCode(n)
    }
    pub fn as_str(&self) -> String {
        format!("MT{:04}", self.0)
    }
}

// Lex/parse: MT0001..MT0999
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

// HIR: MT1001..MT1999
pub const UNRESOLVED_NAME: DiagCode = DiagCode::new(1001);
pub const USE_RESOLVES_TO_NOTHING: DiagCode = DiagCode::new(1002);

// Type checker: MT2001..MT2099
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

// Effects + capabilities + traits + protocol strict: MT4001..MT4099
pub const EFFECT_UNDECLARED: DiagCode = DiagCode::new(4001);
pub const ALLOC_IN_CORE: DiagCode = DiagCode::new(4002);
pub const CAPABILITY_TOO_BROAD: DiagCode = DiagCode::new(4010);
pub const METHOD_AMBIGUOUS: DiagCode = DiagCode::new(4020);
pub const METHOD_NOT_FOUND: DiagCode = DiagCode::new(4021);
pub const TRAIT_COHERENCE_VIOLATION: DiagCode = DiagCode::new(4022);
pub const DYN_REQUIRES_OBJECT_SAFE: DiagCode = DiagCode::new(4023);
pub const PROTOCOL_ARITY_MISMATCH: DiagCode = DiagCode::new(4030);
pub const PROTOCOL_PARAM_TYPE_MISMATCH: DiagCode = DiagCode::new(4031);
pub const PROTOCOL_MISSING_HANDLER: DiagCode = DiagCode::new(4032);
pub const PROTOCOL_EXTRA_HANDLER: DiagCode = DiagCode::new(4033);
pub const DERIVE_COPY_FIELD_NOT_COPY: DiagCode = DiagCode::new(4040);
pub const DERIVE_UNKNOWN: DiagCode = DiagCode::new(4041);

// v0.15 — RFC-008 row-machinery diagnostics for stdlib HOF dispatch.
//
// RFC-008 reserved MT4020..MT4025 in `dev/history/notes/RFC-008-…` but
// those codes were already claimed by the v0.6 trait/method codes shown
// above. The row-machinery codes therefore land in the unused 4050-block
// (same `MT40xx` family / same severity tier; `mty explain` text below
// notes the RFC reservation for searchability).
//
// Wire-by-design: the v0.15 dispatcher only emits MT4050 on a closed-row
// rejection. MT4051..MT4054 are *reserved* in this file so future v0.16
// inference work doesn't renumber them when adding more emit-sites.
pub const ROW_SUBSUMPTION_FAIL: DiagCode = DiagCode::new(4050);
/// MT4051: row var bound to a row containing itself (RFC-008 MT4020 slot).
pub const ROW_OCCURS_CHECK: DiagCode = DiagCode::new(4051);
/// MT4052: row var on a struct field (RFC-008 MT4021 slot).
pub const ROW_VAR_IN_STRUCT: DiagCode = DiagCode::new(4052);
/// MT4053: row var never bound by any argument (RFC-008 MT4022 slot).
pub const ROW_VAR_UNBOUND: DiagCode = DiagCode::new(4053);
/// MT4054: closed-row mismatch — two closed rows differ (RFC-008 MT4024 slot).
pub const ROW_EFFECT_MISMATCH: DiagCode = DiagCode::new(4054);

// v0.16 RFC-008 — user-authored row-poly fn signature validation. The
// MT4055..MT4059 block houses the v0.16 emit-sites that the
// surface-syntax wiring drives. The 4050 base reservation note above
// still applies (RFC-008 numbered them MT4021..MT4025 but those slots
// were taken by v0.6 trait codes); the v0.16 wiring picks up at 4055
// to leave MT4051..MT4054 for the reserved future inference work.

/// MT4055: row variable declared but never bound by any parameter
/// (e.g. `fn read[E](path: String) -> String !E` — no closure param
/// to carry effects through `E`). The fn's open-row signature is
/// structurally degenerate. RFC-008 §"v0.16" — corresponds to the
/// RFC's `row_var_unused` slot, narrowed: v0.16 emits this when the
/// row var appears in the effect clause but no fn-typed parameter
/// exists.
pub const ROW_VAR_UNUSED: DiagCode = DiagCode::new(4055);

/// MT4056: open-row signature where the row var only contributes
/// concrete effects (`!{a, b | E}` but no closure param). Reserved
/// for v0.16 — currently superseded by MT4055/MT4057 which fire on
/// the structural variant.
pub const ROW_VAR_IN_CONCRETE_ONLY: DiagCode = DiagCode::new(4056);

/// MT4057: row variable mentioned in the return effect row but no
/// parameter accepts a fn type from which the row could be bound.
/// RFC-008 §"v0.16" return-position specialisation of MT4055 —
/// surfaced separately so the diagnostic note can point the author
/// at the "add a closure parameter" fix rather than the "drop `E`"
/// alternative.
pub const ROW_VAR_RETURNED_UNBOUND: DiagCode = DiagCode::new(4057);

/// MT4058: row variable arity mismatch — the fn declares multiple
/// distinct row variables (e.g. `fn observed[E, F]` with `!E` and
/// `!F` on different parameters), but v0.16 SHIPPED-SUBSET only
/// supports a single row variable per signature. Reserved for v0.17
/// multi-row-var extension.
pub const ROW_VAR_ARITY_MISMATCH: DiagCode = DiagCode::new(4058);

/// MT4059: row subsumption failure for a user-authored row-poly
/// fn — the caller's row constraint cannot accept the callee's
/// instantiated row (analogous to MT4050 but on a user fn). RFC-008
/// §"v0.16" — caller-side variant of the stdlib MT4050.
pub const ROW_VAR_SUBSUMPTION_FAIL: DiagCode = DiagCode::new(4059);

// Runtime / interpreter (slice 6): MT5001..MT5099
pub const RUNTIME_PANIC: DiagCode = DiagCode::new(5001);
pub const USE_AFTER_DROP: DiagCode = DiagCode::new(5002);
pub const DIVISION_BY_ZERO: DiagCode = DiagCode::new(5003);
pub const INTEGER_OVERFLOW: DiagCode = DiagCode::new(5004);
pub const UNREACHABLE_MATCH: DiagCode = DiagCode::new(5005);
pub const UNHANDLED_ERROR_RESULT: DiagCode = DiagCode::new(5006);
pub const ARENA_ESCAPE_RUNTIME: DiagCode = DiagCode::new(5007);
pub const UNCALLABLE_BUILTIN: DiagCode = DiagCode::new(5008);
pub const BUDGET_EXCEEDED: DiagCode = DiagCode::new(5009);
pub const SANDBOX_VIOLATION: DiagCode = DiagCode::new(5010);
pub const DEADLINE_EXCEEDED: DiagCode = DiagCode::new(5011);
pub const MAILBOX_FULL: DiagCode = DiagCode::new(5012);
pub const SUPERVISOR_ESCALATED: DiagCode = DiagCode::new(5013);
pub const RESTART_LIMIT_EXCEEDED: DiagCode = DiagCode::new(5014);
pub const CAPABILITY_OUTSIDE_SANDBOX: DiagCode = DiagCode::new(5015);
pub const AGENT_HANDLER_MISSING: DiagCode = DiagCode::new(5020);
pub const SEND_TO_DEAD_AGENT: DiagCode = DiagCode::new(5021);
pub const EXTERN_FN_UNIMPL: DiagCode = DiagCode::new(5050);

// Codegen traps (slice 8): MT8001..MT8010
pub const CODEGEN_DIV_BY_ZERO: DiagCode = DiagCode::new(8001);
pub const CODEGEN_OOB_INDEX: DiagCode = DiagCode::new(8002);
pub const CODEGEN_INT_OVERFLOW: DiagCode = DiagCode::new(8003);
pub const CODEGEN_NULL_DEREF: DiagCode = DiagCode::new(8004);
pub const CODEGEN_EXTERN_UNRESOLVED: DiagCode = DiagCode::new(8005);
pub const CODEGEN_UNREACHABLE: DiagCode = DiagCode::new(8006);
pub const CODEGEN_UNSUPPORTED_SHAPE: DiagCode = DiagCode::new(8007);
pub const CODEGEN_LINKER_MISSING: DiagCode = DiagCode::new(8008);
pub const CODEGEN_WASM_VALIDATION: DiagCode = DiagCode::new(8009);
pub const CODEGEN_MONO_FAILED: DiagCode = DiagCode::new(8010);

// Macros: MT6001..MT6099 (moved from mty_macros::diag in v0.6
// integrator pass — see SLICE_V0_6.md easy-win 2).
pub const UNKNOWN_MACRO: DiagCode = DiagCode::new(6001);
pub const MACRO_ARITY_MISMATCH: DiagCode = DiagCode::new(6002);
pub const MACRO_BODY_PARSE_FAILED: DiagCode = DiagCode::new(6003);
pub const RECURSIVE_MACRO_TOO_DEEP: DiagCode = DiagCode::new(6004);
pub const PROC_MACRO_IMPURE: DiagCode = DiagCode::new(6005);
pub const PROC_MACRO_UNSUPPORTED_V0_5: DiagCode = DiagCode::new(6006);
/// MT6007 — proc-macro body invoked an effect at runtime that purity
/// analysis missed (e.g. via an aliased binding the static check can't
/// see). Sandboxed execution observed the call and aborted.
pub const PROC_MACRO_IMPURE_AT_RUNTIME: DiagCode = DiagCode::new(6007);
/// MT6008 — sandboxed proc-macro execution exceeded one of its three
/// resource bounds (wall-clock, step count, or memory). The expansion
/// is aborted and the call site is left as an inert sentinel.
pub const PROC_MACRO_RESOURCE_EXCEEDED: DiagCode = DiagCode::new(6008);

// Borrow checker: MT3001..MT3099
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
/// code, suitable for `mty explain MTxxxx`. Returns None for codes
/// outside the assigned ranges.
pub fn explain(code: DiagCode) -> Option<&'static str> {
    Some(match code.0 {
        1 => {
            "MT0001: Unexpected token.\n\
             \n\
             Cause:   The parser found a token that doesn't fit the current \
             grammar context (typo, missing punctuation, misplaced keyword).\n\
             Example: `fn main() { let x = ; }`   // `=` followed by `;`\n\
             Fix:     Supply the missing expression, fix the typo, or remove \
             the stray token.\n\
             Spec:    \u{a7}5 (lexical grammar) of v1.0-RC2."
        }
        2 => {
            "MT0002: Unterminated string literal. A string starts with \" \
              but never closes before end-of-line or end-of-file. Add the \
              closing quote, or escape any embedded \" as \\\"."
        }
        3 => {
            "MT0003: Invalid escape sequence. The character after \\ in a \
              string or char literal is not a recognized escape. Valid \
              escapes include \\n, \\t, \\r, \\\\, \\\", \\', \\x{HH}, and \
              \\u{HHHH}."
        }
        4 => {
            "MT0004: Unknown duration unit. Mighty duration literals use \
              one of `ns`, `us`, `ms`, `s`, `m`, `h` as the trailing unit. \
              For size literals see MT0001 (`KiB`/`MiB`/`GiB` binary, `k`/`M` \
              decimal)."
        }
        10 => {
            "MT0010: Expected an item.\n\
             \n\
             Cause:   At the top level (or inside a `mod`) the parser \
             expected one of: fn, struct, enum, type, use, mod, package, \
             agent, protocol, supervisor, extern, export, impl, trait, \
             const, macro \u{2014} and found something else.\n\
             Example: `mod m { let x = 1 }`        // `let` is not an item\n\
             Fix:     Wrap the binding in a `fn`, or use `const X = 1` if \
             you really want a module-level constant.\n\
             Spec:    \u{a7}4.2 (items) of v1.0-RC2."
        }
        11 => {
            "MT0011: Expected an expression.\n\
             \n\
             Cause:   The parser reached a position where an expression must \
             appear but found a closing delimiter or a statement keyword.\n\
             Example: `let x = + 1`                // unary `+` is not valid\n\
             Fix:     Supply the missing operand, or use a unary operator the \
             grammar accepts (`-`, `!`, `*`, `&`).\n\
             Spec:    \u{a7}5.4 (expressions) of v1.0-RC2."
        }
        12 => {
            "MT0012: Mismatched delimiter. An opening `(`, `[`, or `{` was \
               not paired with the matching closing delimiter, or they were \
               crossed."
        }
        20 => {
            "MT0020: Duplicate `on` handler. An agent body declared two \
               handlers for the same protocol message. Each protocol message \
               may have at most one `on Message` handler per agent."
        }
        21 => {
            "MT0021: `pub` function needs a return type. Public functions \
               must declare an explicit return type (`-> T`) so callers in \
               other modules can rely on the signature. Add `-> Unit` if the \
               function returns nothing."
        }
        30 => {
            "MT0030: Recursion depth limit exceeded. The parser nested \
               deeper than the configured limit. This usually indicates \
               adversarial or accidentally pathological input; refactor the \
               source to reduce nesting."
        }
        1001 => {
            "MT1001: Unresolved name.\n\
             \n\
             Cause:   The HIR lowerer could not resolve a name reference to \
             any binding in scope.\n\
             Example: `fn main() { log(grting) }`  // `grting` not declared\n\
             Fix:     Check the spelling, add a `let` binding, or bring the \
             name into scope with `use pkg.module.name`. Most type-level \
             unresolved names surface as MT2021 instead.\n\
             Spec:    \u{a7}4.5 (resolution) of v1.0-RC2."
        }
        1002 => {
            "MT1002: `use` resolves to nothing. The path on the right of \
                 `use` does not name any importable item. Verify the package \
                 and module path; remember that paths use `.` as the \
                 separator."
        }
        2001 => {
            "MT2001: Type mismatch.\n\
             \n\
             Cause:   An expression's type does not match the type required \
             by context (parameter, annotation, branch unification, or \
             return type).\n\
             Example: `fn f() -> I32 { \"hello\" }`  // returns Str, not I32\n\
             Fix:     Convert the value (`.to_string()`, `.parse()`, an \
             explicit constructor), or change the annotation. The diagnostic \
             also prints the EXPECTED and FOUND types verbatim \u{2014} use \
             them to locate the conversion site.\n\
             Spec:    \u{a7}7.2 (unification) of v1.0-RC2."
        }
        2002 => {
            "MT2002: Unresolved type.\n\
             \n\
             Cause:   The named type does not exist in scope.\n\
             Example: `fn f(x: Stng) -> Unit {}`   // typo for `Str`\n\
             Fix:     Check the spelling, add a `use pkg.mod.Type`, or use \
             the fully qualified path (`pkg.mod.Type`). Note paths use `.` \
             as the module separator, not `::` or `/`.\n\
             Spec:    \u{a7}7.3 (type resolution) of v1.0-RC2."
        }
        2003 => {
            "MT2003: Cannot infer type. The type checker could not determine \
                 a binding's type from context. Add an explicit annotation \
                 (`let x: T = ...`) or provide more usage."
        }
        2004 => {
            "MT2004: Wrong number of generic arguments. The type or function \
                 expects a specific number of `[T, ...]` arguments. Add or \
                 remove arguments to match the declaration."
        }
        2005 => {
            "MT2005: Wrong number of arguments. The function expects a specific \
                 arity; the call site provides a different number. Check the \
                 function's declaration."
        }
        2006 => {
            "MT2006: Unknown field. The named field does not exist on the \
                 struct. Check spelling and the struct declaration."
        }
        2007 => {
            "MT2007: Unknown method.\n\
             \n\
             Cause:   The named method does not exist on the receiver type. \
             Method resolution searches inherent `impl` blocks, trait \
             `impl`s in scope, and a small built-in table; nothing matched.\n\
             Example: `let s = \"hi\"; s.lengt()`   // typo for `.len()`\n\
             Fix:     Check the spelling. If the method lives on a trait, \
             import the trait with `use pkg.mod.Trait`. If ambiguous between \
             two traits in scope you'll see MT4020 instead.\n\
             Spec:    \u{a7}11.2 (method dispatch) of v1.0-RC2."
        }
        2008 => {
            "MT2008: Not callable. The value being applied does not have a \
                 function type. Check that it refers to a function or a \
                 callable agent constructor."
        }
        2009 => {
            "MT2009: Unknown variant. The named variant does not exist on the \
                 enum. Verify the spelling and the enum declaration."
        }
        2010 => {
            "MT2010: `?` outside Result-returning function. The `?` operator \
                 requires the enclosing function's return type to be \
                 `Result[_, _]`. Change the signature, or replace `?` with an \
                 explicit `match`."
        }
        2011 => {
            "MT2011: `?` error-type mismatch. The error type of `?`'s operand \
                 must match (or coerce to) the enclosing function's error \
                 type. Slice 3 requires an exact match."
        }
        2012 => {
            "MT2012: Wrong variant arity. The enum variant expects a specific \
                 number of payload values; the call site provides a different \
                 count."
        }
        2013 => {
            "MT2013: Missing struct field. The struct literal omits a \
                 required field. Add the field, or (if the field has a \
                 default) use shorthand notation."
        }
        2014 => {
            "MT2014: Duplicate struct field. The struct literal lists the same \
                 field twice. Remove the duplicate."
        }
        2015 => {
            "MT2015: Non-exhaustive match. The match does not cover every \
                 possible value of the scrutinee. Slice 4 made this an \
                 error; add the missing arm(s) or a wildcard `_ => ...`."
        }
        2016 => {
            "MT2016: Unreachable match arm (warning). A later arm cannot be \
                 reached because an earlier arm always matches first."
        }
        2017 => {
            "MT2017: Binary operator type mismatch. The operator is not defined \
                 for the given operand types. Slice 3 supports the standard \
                 numeric/boolean operators only."
        }
        2018 => {
            "MT2018: If/else branch type mismatch. The two branches of an `if` \
                 produce incompatible types. Unify them, or remove the value \
                 use of the `if`."
        }
        2019 => {
            "MT2019: Return-type mismatch. The function declares one return \
                 type but the body produces a different one."
        }
        2020 => {
            "MT2020: Public function parameter requires explicit type. `pub` \
                 functions must declare every parameter's type so callers in \
                 other modules can rely on the signature."
        }
        2021 => {
            "MT2021: Unresolved value.\n\
             \n\
             Cause:   The named identifier does not refer to any value in \
             scope. Since v0.3 (A65) agent / handler / supervisor / \
             cap-narrow bodies are STRICT \u{2014} unknown names are an \
             error here instead of falling through to fresh-var inference.\n\
             Example: `agent A: P { on Tick() -> { counter = 1 } }`\n\
             // `counter` not declared as agent state\n\
             Fix:     Bind the name as agent state (`counter = 0` line), \
             ctor-param, prelude entry, or `use`-imported value. If you want \
             permissive inference, lift the body into a top-level `fn`.\n\
             Spec:    \u{a7}7.3 + amendment A65 of v1.0-RC2."
        }
        2022 => {
            "MT2022: Not a struct. The value cannot be initialized with a \
                 struct literal because it is not a struct type."
        }
        2023 => {
            "MT2023: Generic argument-kind mismatch. The type argument's kind \
                 does not match the expected parameter kind (e.g. supplied a \
                 lifetime where a type was expected)."
        }
        2024 => {
            "MT2024: Lambda arity mismatch. The lambda's parameter count \
                 differs from the expected function type."
        }
        2025 => {
            "MT2025: Cannot take reference. The expression is not a place \
                 (l-value), so `&` cannot apply."
        }
        2026 => {
            "MT2026: Protocol message unknown (warning). An `on Msg(...)` \
                 handler refers to a message that no implemented protocol \
                 declares. Handler params will be typed as fresh inference \
                 variables; declare the message in a protocol or use \
                 protocol composition to bring it in."
        }
        3001 => {
            "MT3001: Use after move.\n\
             \n\
             Cause:   The value was moved earlier (assignment, fn argument, \
             or return) and cannot be used again.\n\
             Example: `let s = String(\"hi\"); let t = move s; log(s.len())`\n\
             // `s` invalid after `move s`\n\
             Fix:     Add `.clone()` before the move if you need both copies, \
             pass a borrow (`&s`) instead of moving, or restructure so each \
             owner has a single move.\n\
             Spec:    \u{a7}7.1 (ownership) of v1.0-RC2."
        }
        3002 => {
            "MT3002: Move out of borrowed value. A reference (`&` or \
                 `&mut`) to the value is still live, so the value cannot be \
                 moved. Wait for the borrow's scope to end, or copy the \
                 value."
        }
        3003 => {
            "MT3003: Borrow after move. The value was already moved and no \
                 longer owns its storage; a borrow is not permitted."
        }
        3004 => {
            "MT3004: Mutable borrow while shared borrows exist.\n\
             \n\
             Cause:   You created a `&mut T` while at least one `&T` to the \
             same value is still live (its last-use point hasn't been \
             reached yet).\n\
             Example: `let r = &v; let m = &mut v; log_len(r)`\n\
             // `r` still used after `&mut v`\n\
             Fix:     Move the `&mut` creation after the last use of every \
             shared borrow, or scope the shared borrow tighter. v0.3 uses \
             NLL (last-use) regions, not lexical scope.\n\
             Spec:    \u{a7}7.4 (borrow regions) + amendment A55 of v1.0-RC2."
        }
        3005 => {
            "MT3005: Shared borrow while a mutable borrow exists.\n\
             \n\
             Cause:   You created a `&T` while a `&mut T` to the same value \
             is still live.\n\
             Example: `let m = &mut v; let r = &v; push(m, x)`\n\
             // `r` made while `m` still live\n\
             Fix:     Drop or last-use the mutable borrow before taking the \
             shared borrow. Mighty enforces exclusive-XOR-shared at all \
             times.\n\
             Spec:    \u{a7}7.4 (borrow regions) of v1.0-RC2."
        }
        3006 => {
            "MT3006: Two mutable borrows of the same value.\n\
             \n\
             Cause:   Only one `&mut T` may exist at a time; you tried to \
             create a second one before the first ended.\n\
             Example: `let m1 = &mut v; let m2 = &mut v; push(m1, x)`\n\
             // m1 still live\n\
             Fix:     Sequence the two mutations \u{2014} finish using `m1` \
             before taking `m2`. If both mutations are field-disjoint, \
             borrow `&mut v.field_a` and `&mut v.field_b` (allowed since \
             v0.3 A54).\n\
             Spec:    \u{a7}7.4 + amendment A54 of v1.0-RC2."
        }
        3007 => {
            "MT3007: Borrow outlives its owner. The borrowed value's \
                 owner goes out of scope before the borrow's lexical \
                 region ends."
        }
        3008 => {
            "MT3008: Cannot move a borrowed value. A reference to the \
                 value is still live."
        }
        3009 => {
            "MT3009: Cannot move out of a reference. Dereferencing `&T` \
                 or `&mut T` does not transfer ownership; you may only \
                 read or write through it."
        }
        3010 => {
            "MT3010: Value escapes its arena. A value bound inside an \
                 `arena name { ... }` cannot leave the arena's scope unless \
                 explicitly promoted via `move` to an ancestor scope."
        }
        3011 => {
            "MT3011: Non-Sendable cross-agent message argument. Every \
                 argument to a `!Msg(...)` or `?Msg(...)` call must be \
                 Sendable. v0.3 (A65) gives Sendable a formal definition: \
                 (a) Copy types are Sendable; (b) owned Sized values with \
                 no internal references are Sendable; (c) references \
                 (`&T`/`&mut T`), capability handles (`Net`/`Fs`/...), \
                 and any type that transitively contains either are NOT \
                 Sendable. User structs can opt in via #[derive(Sendable)] \
                 and the check is enforced at the !/? call site."
        }
        3012 => {
            "MT3012: Drop in const context. A value requiring deterministic \
                 cleanup cannot live in a `const` slot."
        }
        3013 => {
            "MT3013: Cannot mutably borrow an immutable local. The binding \
                 was declared without `mut`."
        }
        3014 => {
            "MT3014: Assignment to immutable local. The binding was \
                 declared without `mut`."
        }
        3015 => {
            "MT3015: Use of uninitialised binding. The binding was \
                 declared but never assigned before its first read."
        }
        4001 => {
            "MT4001: Public function effect set is incomplete.\n\
             \n\
             Cause:   The body calls (transitively) something that produces \
             effects not listed in the function's `effect ...` clause. \
             Effect closure is checked across the whole call graph reachable \
             from the public fn.\n\
             Example: `pub fn save(buf: Bytes) -> Unit { fs.write(\"/x\", buf) }`\n\
             // missing `effect io`\n\
             Fix:     Add the missing effect to the signature \
             (`effect io`), or pass the offending capability as a parameter \
             so the effect is local to the caller. Effects are a contract \
             with downstream packages \u{2014} they cannot be hidden.\n\
             Spec:    \u{a7}9 (effects) of v1.0-RC2."
        }
        4002 => {
            "MT4002: Heap allocation in `core` profile. The strict `core` \
                 profile bans the `alloc` effect; the body uses an \
                 allocator (arena, growable container, html template, \
                 etc.). Switch to a stack-only design or change the \
                 profile."
        }
        4010 => {
            "MT4010: Capability too broad. The argument's capability \
                 constraint is wider than the parameter declares. \
                 Narrow at the call site (e.g. `fs.ro(\"/data\")`) \
                 before passing."
        }
        4020 => {
            "MT4020: Ambiguous method call. Two or more traits in scope \
                 each provide a method of this name on the receiver type. \
                 Disambiguate by importing fewer traits or by an explicit \
                 trait-qualified call."
        }
        4021 => {
            "MT4021: Method not found. No inherent `impl` and no trait \
                 impl in scope provides this method for the receiver \
                 type. Add an `impl T { fn m(...) }` or import the \
                 trait."
        }
        4022 => {
            "MT4022: Trait coherence violation. The same trait is \
                 implemented twice for the same self type. Remove one \
                 of the conflicting `impl Trait for T` blocks."
        }
        4023 => {
            "MT4023: `dyn Trait` requires an object-safe trait and an \
                 implementing concrete type. Slice 5 bans `Self` in \
                 method signatures and bans generic methods inside \
                 traits used through `dyn`."
        }
        4030 => {
            "MT4030: Protocol handler arity mismatch. The `on Msg(...)` \
                 handler declares a different number of parameters than \
                 the protocol's message signature."
        }
        4031 => {
            "MT4031: Protocol handler parameter type mismatch. The handler \
                 uses a parameter at a type incompatible with the \
                 protocol's declared parameter type. v0.3 (A65) fires this \
                 only when the protocol is defined in the current package \
                 (or prelude). External protocols continue to issue \
                 MT2026 warnings — once the external module is in scope, \
                 the strict check will activate automatically. Fix by \
                 adjusting either the handler body's usage or the protocol \
                 declaration so the two agree."
        }
        4032 => {
            "MT4032: Protocol handler missing. The agent implements a \
                 protocol that declares this message, but no `on Msg(...)` \
                 handler is provided. Either implement the handler or \
                 remove the protocol from the agent's declaration."
        }
        4033 => {
            "MT4033: Protocol handler unknown. The `on Msg(...)` handler \
                 refers to a message that no implemented protocol declares. \
                 Either declare the message in a protocol or remove the \
                 handler."
        }
        4040 => {
            "MT4040: `derive(Copy)` requires every field to be Copy. \
                 At least one field's type is not Copy (e.g. `String`, \
                 `Bytes`, or another user ADT that is not itself Copy)."
        }
        4041 => {
            "MT4041: Unknown derive. v0.3 supports `Copy`, `Hash`, `Eq`, \
                 and `Sendable`. Other derive names are reserved for later \
                 slices."
        }
        4050 => {
            "MT4050: Closure effects rejected by row constraint. The \
                 closure passed to a row-polymorphic stdlib HOF (`map`, \
                 `filter`, `fold`, `and_then`, ...) carries effects the \
                 caller's declared effect clause does not allow. The \
                 row-poly signature instantiates `Var(0)` to the \
                 closure's row, and the caller's closed declared row \
                 fails subsumption against it. Fix: add the missing \
                 effect to the caller's `effect ...` clause, or use a \
                 pure closure. (RFC-008 row_subsumption_fail.)"
        }
        4051 => {
            "MT4051: Row variable occurs-check failure. A row variable \
                 would be bound to a row that mentions itself (directly \
                 or via the substitution chain). Reserved for the v0.16 \
                 surface-syntax row-clause inference pass. \
                 (RFC-008 row_occurs_check.)"
        }
        4052 => {
            "MT4052: Row variable on a struct field. Row polymorphism is \
                 a fn-signature feature; struct fields must use a closed \
                 effect set or no effect clause at all. Reserved for \
                 the v0.16 surface-syntax row-clause check. (RFC-008 \
                 row_var_in_struct.)"
        }
        4053 => {
            "MT4053: Unbound row variable. A row variable was declared \
                 in a fn's effect clause but never appears in any \
                 parameter position from which the inference could bind \
                 it. Reserved for the v0.16 surface-syntax row-clause \
                 check. (RFC-008 row_var_unbound.)"
        }
        4054 => {
            "MT4054: Closed-row mismatch. Two closed effect rows differ \
                 in their concrete effects (neither is a sub-row of the \
                 other). Reserved for the v0.16 closed-row equality \
                 paths. (RFC-008 row_effect_mismatch.)"
        }
        4055 => {
            "MT4055: Row variable declared but never bound. The fn's \
                 effect clause references a row variable (`!E`, \
                 `!{... | E}`, or `effect ... | E`) but no parameter \
                 has a fn type that could carry effects through `E`. \
                 Without a closure parameter the row variable cannot be \
                 bound at any call site, so the open-row signature is \
                 structurally degenerate. Fix: add a `fn(...) -> _` \
                 parameter, or drop the row variable from the effect \
                 clause and write a concrete closed row. (RFC-008 \
                 row_var_unused.)"
        }
        4056 => {
            "MT4056: Row variable in concrete-only position. The fn's \
                 effect clause is `!{a, b | E}` but the row variable is \
                 not used by any parameter, so the concrete `{a, b}` \
                 part is the only effective component. Reserved for \
                 v0.16 — most cases currently surface as MT4055/MT4057. \
                 (RFC-008 row_var_in_concrete_only.)"
        }
        4057 => {
            "MT4057: Row variable in return effect row but unbound. \
                 The fn declares a row variable on the return side but \
                 has no fn-typed parameter from which the row could be \
                 inferred at the call site. Add a closure parameter \
                 (`f: fn(...) -> T`) to give the row var a binding \
                 site, or convert the return row to a concrete closed \
                 set. (RFC-008 row_var_returned_but_unbound.)"
        }
        4058 => {
            "MT4058: Row variable arity mismatch. The fn declares \
                 multiple distinct row variables, but v0.16 supports a \
                 single row variable per signature. Use one row name \
                 across all closure parameters, or wait for the v0.17 \
                 multi-row-var extension. (RFC-008 \
                 row_var_arity_mismatch.)"
        }
        4059 => {
            "MT4059: Closure effects rejected by user-fn row \
                 constraint. The closure passed to a user-authored \
                 row-polymorphic fn carries effects the caller's \
                 declared effect clause does not allow. This is the \
                 user-fn analogue of MT4050 (stdlib HOFs). Add the \
                 missing effect to the enclosing fn's `effect ...` \
                 clause, or replace the closure with a pure one. \
                 (RFC-008 row_var_subsumption_fail.)"
        }
        5001 => {
            "MT5001: Runtime panic. The program executed `panic(msg)` \
                 (or an unreachable terminator). The interpreter unwinds \
                 to the top of `main` and exits with code 1."
        }
        5002 => {
            "MT5002: Use after drop. A reference outlived the local it \
                 pointed into. The borrow checker proves this statically \
                 in well-typed programs; this trap fires only on programs \
                 that bypassed type-checking."
        }
        5003 => {
            "MT5003: Division by zero. The interpreter trapped on `a / 0` \
                 or `a % 0`. The static checker does not currently flag \
                 divisions by literal zero; that is post-v0.1 work."
        }
        5004 => {
            "MT5004: Integer overflow. Arithmetic exceeded the target \
                 integer's range. In debug builds (and the slice-6 \
                 interpreter) this traps. Release-mode wrap is post-v0.1."
        }
        5005 => {
            "MT5005: Unreachable match. The interpreter fell off the end \
                 of a `match` whose arms did not cover the scrutinee. \
                 Make the match exhaustive or add `_ => ...`."
        }
        5006 => {
            "MT5006: Unhandled error result. Slice 6 reports this when \
                 `main` returns a `Result::Err(...)` value: the process \
                 exits 1 and prints the err payload."
        }
        5007 => {
            "MT5007: Arena escape at runtime. The interpreter caught a \
                 value escaping its arena scope. Borrow check MT3010 \
                 covers the static case; this is a defense in depth."
        }
        5008 => {
            "MT5008: Uncallable builtin. The program tried to invoke a \
                 built-in fn whose interpreter implementation is not yet \
                 wired up. File an issue with the fn name."
        }
        5009 => {
            "MT5009: Budget exceeded. The interpreter's step budget \
                 (default 1 000 000 ops) was exhausted, or an async \
                 suspension point was reached that slice 6 cannot honor."
        }
        5010 => {
            "MT5010: Sandbox violation. The runtime denied a capability \
                 call because the active sandbox's allowlist (fs.read, \
                 fs.write, or net) does not cover the requested target."
        }
        5011 => {
            "MT5011: Deadline exceeded. An `?Msg(args) @duration` ask \
                 did not receive a reply within the requested duration. \
                 The runtime cancels the reply oneshot and the caller \
                 observes Result::Err(DeadlineExceeded) — or a typed-error \
                 variant when the protocol declares one."
        }
        5012 => {
            "MT5012: Mailbox full. An agent's mailbox is at its declared \
                 `mb` depth and the budget policy is `drop` or `fail`. \
                 Under the default `block` policy the sender back-pressures \
                 instead of trapping."
        }
        5013 => {
            "MT5013: Supervisor escalated. A supervisor's `escalate` \
                 strategy propagated a child failure to its parent. At \
                 the top of the supervisor tree this terminates the run."
        }
        5014 => {
            "MT5014: Restart limit exceeded. A child agent exceeded its \
                 `restart up_to N in DUR` budget. The supervisor escalates \
                 per its strategy."
        }
        5015 => {
            "MT5015: Capability outside sandbox. A capability call \
                 attempted to reach a path or host not on the active \
                 sandbox allowlist. The runtime denies the call."
        }
        5020 => {
            "MT5020: Agent handler missing. A `send` or `ask` referenced \
                 a message the target agent does not handle. The static \
                 checker covers most cases (MT4032/MT4033); this code \
                 covers `dyn`-dispatch holes."
        }
        5021 => {
            "MT5021: Send to dead agent. The target agent handle no \
                 longer resolves (e.g. the agent panicked and was not \
                 supervised). Slice 7 will integrate supervisor restart."
        }
        5050 => {
            "MT5050: Extern fn unimplemented. The interpreter has no \
                 host binding for the named `extern { fn ... }` symbol. \
                 Register a host stub or run under a target that \
                 supplies the symbol."
        }
        8001 => {
            "MT8001: Divide by zero in compiled code. The native or \
                 wasm backend lowered an integer division whose RHS \
                 was zero at runtime. Add a guard before the division \
                 or migrate to a checked-arithmetic helper."
        }
        8002 => {
            "MT8002: Out-of-bounds index in compiled code. An array \
                 index escaped its declared length. Slice-8 codegen \
                 emits bounds-checked loads; ensure the index is in \
                 `0..len`."
        }
        8003 => {
            "MT8003: Integer overflow in checked arithmetic. A checked \
                 add/sub/mul wrapped past its representable range. \
                 Use the wrapping_* helpers if wrap-around is intended."
        }
        8004 => {
            "MT8004: Null pointer dereference in compiled code. A \
                 raw pointer (`*T`) was dereferenced while null. \
                 Mighty safe code never produces null `*T`; this \
                 trap fires only from `unsafe` blocks."
        }
        8005 => {
            "MT8005: Extern symbol unresolved. The runtime's libloading \
                 lookup failed for an `extern { fn ... }` declaration. \
                 Ensure the library is in your loader path or override \
                 it via `mighty.toml [extern]`."
        }
        8006 => {
            "MT8006: Unreachable code executed. A SIR block marked \
                 unreachable by the lowerer was reached at runtime — \
                 usually a sign of a typeck bug or a hand-edited SIR \
                 program."
        }
        8007 => {
            "MT8007: Codegen rejected SIR shape. The selected backend \
                 (Cranelift native or Wasm) cannot lower this fn yet. \
                 The driver normally falls back to the interpreter for \
                 `mty run`; `mty build` reports it as an error."
        }
        8008 => {
            "MT8008: Native linker missing. `mty build --target native` \
                 emitted a `.o` but could not find a system linker \
                 (cc / gcc / clang on unix, link.exe on windows). Set \
                 `STARDUST_LINKER` to point at one, or install a C \
                 toolchain."
        }
        8009 => {
            "MT8009: Emitted Wasm failed validation. The wasm backend \
                 produced a module that `wasmparser` rejected. This is \
                 a codegen bug — please report with a reduced test case."
        }
        8010 => {
            "MT8010: Monomorphization failed. A generic fn with \
                 unresolved type parameters reached the codegen. The \
                 monomorphizer should have specialized or rejected it; \
                 if you see this, file a bug."
        }
        6001 => {
            "MT6001: Unknown macro.\n\
             \n\
             Cause:   The call site `name!(...)` refers to a name that is \
             not a registered declarative or procedural macro.\n\
             Example: `fn main() { dbg!(42) }`     // `dbg!` not declared\n\
             Fix:     Declare it with `macro Name(...) => { ... }` above the \
             call site, or import a cross-file macro with `use otherpkg.name` \
             (the exporting file must say `pub macro`). Check for a typo \
             too \u{2014} macro names are case-sensitive.\n\
             Spec:    \u{a7}20.3 (macros) of v1.0-RC2."
        }
        6002 => {
            "MT6002: Macro arity mismatch. The macro was declared with a fixed \
             number of parameters; the call site supplied a different count. \
             v0.6 macros do not support variadic parameters."
        }
        6003 => {
            "MT6003: Macro body did not parse after expansion. Substituting the \
             call-site arguments into the body produced tokens that no longer \
             form a valid expression or statement. Check for missing punctuation \
             in the macro body, or for arguments that need parentheses to remain \
             a single sub-expression after substitution."
        }
        6004 => {
            "MT6004: Recursive macro expansion exceeded the depth cap (32).\n\
             \n\
             Cause:   The macro called itself (directly or via another macro) \
             more times than v0.6 permits. The cap is hard-coded at 32 \
             expansion frames per call site.\n\
             Example: `macro Fwd(x) => { Fwd!(x) }; fn main() { Fwd!(1) }`\n\
             Fix:     Rewrite the macro non-recursively (most patterns can be \
             expressed as iterated substitution), or split into a base case + \
             one recursive step so the depth bound is met. Bounded recursion \
             with explicit fuel parameters is on the RFC backlog (RFC-005).\n\
             Spec:    \u{a7}20.3.4 (macro recursion) of v1.0-RC2."
        }
        6005 => {
            "MT6005: Procedural macro impurity. The proc-macro body contains a \
             call that looks like an effect (I/O, time, env, model, rand). \
             Procedural macros must be pure functions over TokenStream; effects \
             are forbidden because expansion happens at compile time, inside a \
             sandbox, with no access to the runtime environment."
        }
        6006 => {
            "MT6006: Procedural macro execution is not supported yet. The \
             declaration parses and is stored in the registry, but the body \
             cannot run until the sandboxed compile-time interpreter ships. \
             Replace the call with a hand-expanded equivalent, or wait for the \
             proc-macro sandbox slice."
        }
        6007 => {
            "MT6007: Procedural macro impurity at runtime. The proc-macro body \
             tried to invoke a runtime effect (I/O, time, env, model, rand) \
             during sandboxed execution. The static purity check (MT6005) may \
             have been bypassed by aliasing the impure name through a `let` \
             binding. Proc macros must be pure — remove the effect call or \
             move the side effect into runtime code."
        }
        6008 => {
            "MT6008: Procedural macro resource bound exceeded. The sandboxed \
             expansion ran for more than 100 ms wall-clock, used more than \
             100,000 interpreter steps, or allocated more than 16 MiB. Reduce \
             the macro's complexity, or split the work between several smaller \
             macros."
        }
        _ => return None,
    })
}
