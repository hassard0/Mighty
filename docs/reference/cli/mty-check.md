# mty check

Lex, parse, HIR-lower, and type-check a single Mighty source file;
emit diagnostics.

## Synopsis

```
mty check <PATH>
```

## Arguments

| Name | Description |
|---|---|
| `PATH` | Path to a `.mty` file to check. |

## Options

| Flag | Purpose |
|---|---|
| `-h`, `--help` | Print help. |
| `--format <kind>` | `pretty` (default), `json` (NDJSON envelopes), or `json-result` (single structured-result document). |
| `--include-source` | Only meaningful with `--format json`: embed a 3-line snippet around each diagnostic's span in every envelope. |
| `--json` | v0.45 T3 — emit a single structured-result JSON document on stdout (`{ok, path, diagnostics:[{code, severity, message, span:{file, line, col, end_line, end_col}}]}`) and suppress every ariadne byte. Equivalent to `--format json-result`. |

## Behavior

- Reads the file as UTF-8.
- Runs the lexer, parser, AST view, and HIR lowering.
- If lowering produced no hard errors, runs the type checker
  (`SD2xxx`).
- Warnings (severity `Warning`, e.g. `MT2015 non_exhaustive_match`)
  are reported but do not affect the exit status.
- If any errors are present, renders them with `ariadne` to stderr
  (colorized when stderr is a TTY) and exits 1.
- Otherwise prints `ok: <path>` to stdout and exits 0.

As of slice 3, `mty check` performs:

1. Lex (MT0001..MT0004)
2. Parse (MT0010..MT0030)
3. HIR lowering (MT1001..MT1002)
4. Type checking (MT2001..MT2025)

Still deferred to later slices: ownership / affine / borrow checking
(slice 4), effect closure + capability narrowing (slice 5), trait
coherence + dispatch (slice 4+), exhaustiveness as an error (slice 5).

See the [diagnostic codes](../diagnostics.md) page for the registry of
`SDxxxx` codes.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | parsed and lowered without errors |
| `1` | one or more diagnostics emitted, or an I/O error |

## Structured-result JSON (`--json`, v0.45 T3)

With `--json` (or `--format json-result`), `mty check` emits a single
JSON document on stdout instead of the ariadne pretty report. This is
the "agent command surfaces" shape every other v0.45 surface (LSP,
runtime control paths, `mty agent`'s `check` op via `result.result`)
is migrating to. The exit code is preserved (`0` if `ok:true`,
non-zero otherwise) and ariadne text is fully suppressed under
`--json` — agents can `serde_json::from_str(&stdout)` without
pre-stripping.

```json
{
  "ok": false,
  "path": "src/main.mty",
  "diagnostics": [
    {
      "code": "MT2001",
      "severity": "error",
      "message": "type mismatch: expected Str, found I32",
      "span": {
        "file": "src/main.mty",
        "line": 12, "col": 5,
        "end_line": 12, "end_col": 18
      }
    }
  ]
}
```

The richer NDJSON envelope path (`--format json`) is unchanged and
continues to drive `mty fix --apply --from-stdin` and the agent-mode
protocol. See
[`docs/internals/diagnostic-envelopes.md`](../../internals/diagnostic-envelopes.md)
for the envelope schema. The simpler structured-result shape above is
documented in
[`crates/mty-diagnostics/src/fix.rs`](../../../crates/mty-diagnostics/src/fix.rs)
(`CheckResult` / `CheckDiagnostic`).

## Examples

```bash
mty check src/main.mty
mty check examples/07_agent_echo.mty
mty check --json src/main.mty | jq '.diagnostics[].code'
```

In CI:

```bash
for f in examples/*.mty; do mty check "$f" || exit 1; done
```
