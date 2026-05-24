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
$ sdust explain SD0001
SD0001: Unexpected token. ...
```

## Ranges

| Range | Category |
|---|---|
| `SD0001`–`SD0999` | Lex and parse |
| `SD1001`–`SD1999` | HIR lowering and name resolution |
| `SD2001`–`SD2999` | Type checking (slice 3) |
| `SD3001`–`SD3999` | Borrow / move / affine (slice 4) |
| `SD4001`–`SD4999` | Effect / capability (slice 5) |

## Slice 1 codes

| Code | Name | Meaning |
|---|---|---|
| `SD0001` | `UNEXPECTED_TOKEN` | The parser encountered a token it could not consume in any production. |
| `SD0002` | `UNTERMINATED_STRING` | A string literal was not terminated before EOF. |
| `SD0003` | `INVALID_ESCAPE` | A string or char literal contained an unrecognized escape sequence. |
| `SD0004` | `UNKNOWN_DURATION_UNIT` | A numeric literal used a duration suffix the lexer does not recognize. |
| `SD0010` | `EXPECTED_ITEM` | Expected a top-level item (function, struct, agent, etc.). |
| `SD0011` | `EXPECTED_EXPR` | Expected an expression. |
| `SD0012` | `MISMATCHED_DELIMITER` | A closing delimiter did not match its opener. |
| `SD0020` | `DUPLICATE_ON_HANDLER` | An agent declared two `on Msg` handlers for the same message. |
| `SD0021` | `PUB_NEEDS_RETURN_TYPE` | A `pub fn` is missing its return type. |
| `SD0030` | `DEPTH_LIMIT_EXCEEDED` | The parser exceeded its nesting depth limit. |
| `SD1001` | `UNRESOLVED_NAME` | A name could not be resolved to any binding. |
| `SD1002` | `USE_RESOLVES_TO_NOTHING` | A `use` import targets a path with no resolution. |

## Slice 3 codes (type checker, SD2001..SD2099)

| Code | Name | Meaning |
|---|---|---|
| `SD2001` | `TYPE_MISMATCH` | Expression's type doesn't match expected type. |
| `SD2002` | `UNRESOLVED_TYPE` | Type name does not name any type in scope. |
| `SD2003` | `CANNOT_INFER_TYPE` | Cannot infer a binding's type. |
| `SD2004` | `WRONG_GENERIC_ARITY` | Wrong number of `[T, ...]` generic args. |
| `SD2005` | `WRONG_ARG_COUNT` | Wrong number of args to a call. |
| `SD2006` | `UNKNOWN_FIELD` | Struct has no such field. |
| `SD2007` | `UNKNOWN_METHOD` | Type has no such method. |
| `SD2008` | `NOT_CALLABLE` | Value is not callable. |
| `SD2009` | `UNKNOWN_VARIANT` | Enum has no such variant. |
| `SD2010` | `QUESTION_OUTSIDE_RESULT` | `?` outside a Result-returning fn. |
| `SD2011` | `QUESTION_ERROR_MISMATCH` | `?` error type doesn't match enclosing fn. |
| `SD2012` | `WRONG_VARIANT_ARITY` | Variant payload count mismatch. |
| `SD2013` | `MISSING_STRUCT_FIELD` | Struct literal omits a required field. |
| `SD2014` | `DUPLICATE_STRUCT_FIELD` | Struct literal lists a field twice. |
| `SD2015` | `NON_EXHAUSTIVE_MATCH` | (warning) match doesn't cover all cases. |
| `SD2016` | `UNREACHABLE_MATCH_ARM` | (warning) arm shadowed by earlier arm. |
| `SD2017` | `BINOP_TYPE_MISMATCH` | Operator not defined on operand types. |
| `SD2018` | `IF_BRANCH_MISMATCH` | If/else branches have incompatible types. |
| `SD2019` | `RETURN_TYPE_MISMATCH` | Fn body produces wrong type for return. |
| `SD2020` | `PUB_PARAM_NEEDS_TYPE` | `pub fn` parameter needs explicit type. |
| `SD2021` | `UNRESOLVED_VALUE` | Name does not refer to any value. |
| `SD2022` | `NOT_A_STRUCT` | Value cannot be struct-initialized. |
| `SD2023` | `GENERIC_ARG_MISMATCH` | Generic arg kind mismatch. |
| `SD2024` | `LAMBDA_ARITY_MISMATCH` | Lambda has wrong param count. |
| `SD2025` | `CANNOT_TAKE_REF` | Cannot take reference to non-place. |

## Adding a new code

1. Pick the next free number in the appropriate range.
2. Add a `pub const NAME: DiagCode = DiagCode::new(N);` to
   `codes.rs`.
3. Add a row to the table above.
4. Reference it from the producing site via
   `Diagnostic::error(codes::NAME, label)`.

Codes never get renumbered; if one is retired, retire it permanently
and skip the number.
