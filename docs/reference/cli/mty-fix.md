# mty fix

Bulk-apply structured fix proposals from `mty check`'s diagnostic
envelopes. Ships in v0.35 T3 — closes the loop on agent first-shot →
zero-shot by letting the CLI (or an LLM agent) take a `mty check`
report and roll forward without an editor in the loop.

## Synopsis

```
mty fix --apply <PATH> [OPTIONS]
mty check --format json <PATH> | mty fix --apply --from-stdin [OPTIONS]
```

## Arguments

| Name | Description |
|---|---|
| `PATH` | Path to a `.mty` file to fix. Optional when `--from-stdin` is set and at least one envelope on stdin carries a `span.file`. |

## Options

| Flag | Purpose |
|---|---|
| `--apply` | Required. Without it, `mty fix` is a no-op (reserved for future read-only commands). |
| `--code <CODE>` | Apply only fixes whose code matches (e.g. `MT4099`). |
| `--alternative <N>` | Always pick this 0-indexed alternative instead of the highest-confidence one. Falls back to the best surviving alternative when index `N` is below the confidence threshold. |
| `--threshold <FLOAT>` | Confidence floor. Default `0.85` (matches the LSP `preferred_threshold`). |
| `--dry-run` | Print the resulting diff to stdout; do not write back. |
| `--interactive` | Prompt `y/N` before each fix. Incompatible with `--from-stdin`. |
| `--from-stdin` | Read NDJSON envelopes from stdin (pipe from `mty check --format json`). |
| `-h`, `--help` | Print help. |

## Policy

For every diagnostic that ships a fix envelope:

1. If `--code` is set and doesn't match, skip.
2. Filter alternatives to those with `confidence >= --threshold`
   (default `0.85`).
3. Pick:
   - `--alternative N` → that 0-indexed alternative (when it survives
     filtering); else
   - Highest-confidence surviving alternative.
4. Apply via the shared `mty_diagnostics::apply::apply_unified_diff`
   helper.
5. After every successful application, re-validate the diff for the
   next envelope. If a later envelope's diff no longer matches the
   current source (the file shifted under it), `mty fix` silently
   drops that envelope and counts it as `unapplied`.

### Conflict resolution

When two fixes touch overlapping lines, `mty fix --apply`:

1. Sorts applicable picks **highest line number first** so earlier-
   in-file edits remain anchored as we splice.
2. Within a line, innermost-span first (higher column wins).
3. After each splice, the next pick's diff is matched against the
   updated buffer. Picks whose OLD lines no longer appear are
   skipped — they're tallied as `unapplied` in the summary.

The same shape powers the LSP `source.fixAll.mighty` bulk-apply
action, so behavior matches between the CLI and the editor.

## Examples

### Apply all high-confidence fixes to a single file

```bash
mty fix --apply src/main.mty
# Applied 3 fixes (MT4099 ×2, MT1001 ×1), 1 unapplied
```

### Only apply MT4099 (taint) fixes

```bash
mty fix --apply src/main.mty --code MT4099
```

### Override the default alternative

```bash
# Always pick the second alt (e.g. sanitizer over regex for MT4099).
mty fix --apply src/main.mty --alternative 1
```

### Lower the confidence floor

```bash
# Apply borderline 0.7-confidence fixes too.
mty fix --apply src/main.mty --threshold 0.7
```

### Preview the diff without writing

```bash
mty fix --apply src/main.mty --dry-run
# --- a/src/main.mty
# +++ b/src/main.mty
# ...
```

### Confirm each fix before applying

```bash
mty fix --apply src/main.mty --interactive
# Apply fix? [MT4099 — Constrain via a known-safe regex] (y/N): y
# applied MT4099 — Constrain via a known-safe regex
# ...
```

### Pipe from `mty check --format json`

```bash
mty check --format json src/main.mty | mty fix --apply --from-stdin
```

This is the canonical zero-shot agent loop: an LLM agent issues
`mty check --format json`, parses the envelopes, and (when it wants
to "let the compiler drive") pipes them back through `mty fix`
without re-reading the file.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | At least one fix applied successfully, OR no diagnostics with fixes (clean noop). |
| `1` | File read / write error, or malformed stdin NDJSON. |
| `2` | Usage error (e.g. `--apply` without `<PATH>` and without `--from-stdin`). |

## Output

- **stderr** — one line per applied fix (`applied MT4099 — <label>`)
  plus the summary (`Applied N fixes (...), M unapplied`).
- **stdout** — unchanged file in non-`--dry-run` mode; the unified
  diff in `--dry-run` mode.

## Related

- [`mty check`](mty-check.md) — emit envelopes via `--format json`.
- [`mty explain <code>`](mty-explain.md) — print the human-facing
  explanation behind a diagnostic code.
- [Diagnostic envelopes](../../internals/diagnostic-envelopes.md) —
  the wire shape `mty fix` consumes.
- LSP `source.fixAll.mighty` — the editor-side counterpart documented
  in [`docs/internals/lsp.md`](../../internals/lsp.md).
