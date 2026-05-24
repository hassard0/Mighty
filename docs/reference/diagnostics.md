# Diagnostic codes

Stardust diagnostics carry a stable `SDxxxx` code. Codes are assigned
once and never renumbered. This page is the authoritative registry.

The codes live in
[`crates/sdust-diagnostics/src/codes.rs`](../../crates/sdust-diagnostics/src/codes.rs).

## Discovering explanations

For any assigned code, [`sdust explain <CODE>`](cli/sdust-explain.md)
prints a short paragraph describing the diagnostic and suggested
fixes. Example:

```sh
$ sdust explain MT0001
MT0001: Unexpected token. ...
```

## Ranges

| Range | Category |
|---|---|
| `MT0001`–`MT0999` | Lex and parse |
| `MT1001`–`MT1999` | HIR lowering and name resolution |
| `MT2001`–`MT2999` | Type checking (slice 3) |
| `MT3001`–`MT3999` | Borrow / move / affine (slice 4) |
| `MT4001`–`MT4999` | Effect / capability (slice 5) |

## Slice 1 codes

| Code | Name | Meaning |
|---|---|---|
| `MT0001` | `UNEXPECTED_TOKEN` | The parser encountered a token it could not consume in any production. |
| `MT0002` | `UNTERMINATED_STRING` | A string literal was not terminated before EOF. |
| `MT0003` | `INVALID_ESCAPE` | A string or char literal contained an unrecognized escape sequence. |
| `MT0004` | `UNKNOWN_DURATION_UNIT` | A numeric literal used a duration suffix the lexer does not recognize. |
| `MT0010` | `EXPECTED_ITEM` | Expected a top-level item (function, struct, agent, etc.). |
| `MT0011` | `EXPECTED_EXPR` | Expected an expression. |
| `MT0012` | `MISMATCHED_DELIMITER` | A closing delimiter did not match its opener. |
| `MT0020` | `DUPLICATE_ON_HANDLER` | An agent declared two `on Msg` handlers for the same message. |
| `MT0021` | `PUB_NEEDS_RETURN_TYPE` | A `pub fn` is missing its return type. |
| `MT0030` | `DEPTH_LIMIT_EXCEEDED` | The parser exceeded its nesting depth limit. |
| `MT1001` | `UNRESOLVED_NAME` | A name could not be resolved to any binding. |
| `MT1002` | `USE_RESOLVES_TO_NOTHING` | A `use` import targets a path with no resolution. |

## Slice 3 codes (type checker, MT2001..MT2099)

| Code | Name | Meaning |
|---|---|---|
| `MT2001` | `TYPE_MISMATCH` | Expression's type doesn't match expected type. |
| `MT2002` | `UNRESOLVED_TYPE` | Type name does not name any type in scope. |
| `MT2003` | `CANNOT_INFER_TYPE` | Cannot infer a binding's type. |
| `MT2004` | `WRONG_GENERIC_ARITY` | Wrong number of `[T, ...]` generic args. |
| `MT2005` | `WRONG_ARG_COUNT` | Wrong number of args to a call. |
| `MT2006` | `UNKNOWN_FIELD` | Struct has no such field. |
| `MT2007` | `UNKNOWN_METHOD` | Type has no such method. |
| `MT2008` | `NOT_CALLABLE` | Value is not callable. |
| `MT2009` | `UNKNOWN_VARIANT` | Enum has no such variant. |
| `MT2010` | `QUESTION_OUTSIDE_RESULT` | `?` outside a Result-returning fn. |
| `MT2011` | `QUESTION_ERROR_MISMATCH` | `?` error type doesn't match enclosing fn. |
| `MT2012` | `WRONG_VARIANT_ARITY` | Variant payload count mismatch. |
| `MT2013` | `MISSING_STRUCT_FIELD` | Struct literal omits a required field. |
| `MT2014` | `DUPLICATE_STRUCT_FIELD` | Struct literal lists a field twice. |
| `MT2015` | `NON_EXHAUSTIVE_MATCH` | Match doesn't cover all cases (slice 4 promoted to error). |
| `MT2016` | `UNREACHABLE_MATCH_ARM` | (warning) arm shadowed by earlier arm. |
| `MT2017` | `BINOP_TYPE_MISMATCH` | Operator not defined on operand types. |
| `MT2018` | `IF_BRANCH_MISMATCH` | If/else branches have incompatible types. |
| `MT2019` | `RETURN_TYPE_MISMATCH` | Fn body produces wrong type for return. |
| `MT2020` | `PUB_PARAM_NEEDS_TYPE` | `pub fn` parameter needs explicit type. |
| `MT2021` | `UNRESOLVED_VALUE` | Name does not refer to any value. |
| `MT2022` | `NOT_A_STRUCT` | Value cannot be struct-initialized. |
| `MT2023` | `GENERIC_ARG_MISMATCH` | Generic arg kind mismatch. |
| `MT2024` | `LAMBDA_ARITY_MISMATCH` | Lambda has wrong param count. |
| `MT2025` | `CANNOT_TAKE_REF` | Cannot take reference to non-place. |
| `MT2026` | `PROTOCOL_MSG_UNKNOWN` | (warning) agent handler msg not in any implemented protocol. |

## Slice 4 codes (borrow checker, MT3001..MT3099)

| Code | Name | Meaning |
|---|---|---|
| `MT3001` | `USE_AFTER_MOVE` | Use of a value after it was moved. |
| `MT3002` | `MOVE_OUT_OF_BORROW` | (reserved; slice 4 uses MT3008) Move while borrowed. |
| `MT3003` | `BORROW_AFTER_MOVE` | Borrow created after the value was moved. |
| `MT3004` | `MUT_BORROW_WHILE_SHARED` | `&mut` created while shared borrows exist. |
| `MT3005` | `SHARED_BORROW_WHILE_MUT` | `&` created while a `&mut` is live. |
| `MT3006` | `TWO_MUT_BORROWS` | Second `&mut` to the same value. |
| `MT3007` | `BORROW_OUTLIVES_OWNER` | (reserved) Borrow lifetime exceeds the owner's. |
| `MT3008` | `CANNOT_MOVE_BORROWED` | Moved a value while it was borrowed. |
| `MT3009` | `MOVE_OUT_OF_REF` | (reserved) Tried to move out of a reference. |
| `MT3010` | `ARENA_ESCAPE` | Arena-local value escapes its arena scope. |
| `MT3011` | `NON_SENDABLE_MESSAGE_ARG` | Cross-agent message arg is not Sendable. |
| `MT3012` | `DROP_IN_CONST_CONTEXT` | (reserved) Drop-requiring value in const context. |
| `MT3013` | `MUT_BORROW_OF_IMMUT_LOCAL` | `&mut` of a local declared without `mut`. |
| `MT3014` | `ASSIGN_TO_IMMUT_LOCAL` | Assigned to a local declared without `mut`. |
| `MT3015` | `USE_OF_UNINITIALIZED` | Read of a binding never assigned. |

## Adding a new code

1. Pick the next free number in the appropriate range.
2. Add a `pub const NAME: DiagCode = DiagCode::new(N);` to
   `codes.rs`.
3. Add a row to the table above.
4. Reference it from the producing site via
   `Diagnostic::error(codes::NAME, label)`.

Codes never get renumbered; if one is retired, retire it permanently
and skip the number.
