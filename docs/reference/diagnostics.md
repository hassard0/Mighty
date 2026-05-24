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

## Adding a new code

1. Pick the next free number in the appropriate range.
2. Add a `pub const NAME: DiagCode = DiagCode::new(N);` to
   `codes.rs`.
3. Add a row to the table above.
4. Reference it from the producing site via
   `Diagnostic::error(codes::NAME, label)`.

Codes never get renumbered; if one is retired, retire it permanently
and skip the number.
