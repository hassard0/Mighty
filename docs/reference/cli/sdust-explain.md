# sdust explain

Print a human-readable explanation of a Stardust diagnostic code.

## Synopsis

```
sdust explain <CODE>
```

## Arguments

| Argument | Required | Description |
|---|---|---|
| `<CODE>` | yes | Diagnostic code in any of these forms: `MT0001`, `sd0001`, `0001`, `1`. |

## Examples

```sh
$ sdust explain MT0001
MT0001: Unexpected token. The lexer or parser found a token that
doesn't fit the current grammar context. Check for typos, missing
punctuation, or a misplaced keyword.

$ sdust explain 1001
MT1001: Unresolved name. The HIR lowerer could not resolve a name
reference to any binding in scope. Check the spelling and ensure
the binding's `use` or declaration is visible.

$ sdust explain MT9999
error: unknown diagnostic code MT9999
$ echo $?
1
```

## Exit codes

| Code | Meaning |
|------|---------|
| `0`  | The code was recognized; explanation printed to stdout. |
| `1`  | The code is well-formed but not a known Stardust diagnostic. |
| `2`  | The argument is not a valid diagnostic-code string. |

## See also

- [`docs/reference/diagnostics.md`](../diagnostics.md) — full list of assigned codes
- `crates/sdust-diagnostics/src/codes.rs` — source of truth
