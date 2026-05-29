# Diagnostic envelopes — v0.33 T4 / v0.34 T4

> **Status:** Stable contract since v0.33; v0.34 T4 locks the wire shape
> with an explicit `schema_version` field on every envelope. The
> contract supersedes per-diagnostic string parsing for every
> downstream consumer.

Every Mighty diagnostic carries an `MTxxxx` code, a primary span, a
human-readable message, and (where the diagnostic kind admits one) a
proposed fix. v0.33 Track 4 adds a structured JSON envelope on top of
the existing human renderer so an agent (Mighty's first-class
consumer) can read the diagnostic, understand the precise span and
cause, and apply a proposed fix without re-parsing prose.

This document is the agent-mode protocol contract for v0.33 T5
(`mty agent-mode`). The schema is enforced by tests in
`crates/mty-diagnostics/src/fix.rs` and
`crates/mty-diagnostics/src/codes_fix.rs`.

## Invocation

```
mty check <path> --format json [--include-source]
```

- `--format json` emits one envelope per diagnostic on **stdout**,
  in NDJSON (one JSON object per line, no array wrapper). Clean
  results produce zero output (the absence of any envelope is the
  success signal).
- `--include-source` adds a `source` field to each envelope with a
  three-line snippet centered on the primary span.
- The default `--format pretty` output is unchanged from v0.32.

Pretty output goes to **stderr**; JSON output goes to **stdout**.
This means an agent can pipe `mty check foo.mty --format json | jq`
without colliding with `ariadne`'s diagnostic stream.

## Envelope shape

```json
{
  "schema_version": "1.0",
  "code": "MT4099",
  "severity": "error",
  "span": {
    "file": "src/main.mty",
    "line": 18,
    "col": 27,
    "len": 12,
    "byte_start": 412,
    "byte_end": 424
  },
  "title": "tainted value flows to fs.write",
  "prose": "The value `user_input` originates from agent.ask() which returns Tainted[Str]. The std.fs.write sink requires untainted input.",
  "fix": {
    "kind": "untaint",
    "confidence": 0.92,
    "alternatives": [
      {
        "label": "Constrain via a known-safe regex",
        "diff": "--- a/src/main.mty\n+++ b/src/main.mty\n@@ -18,1 +18,3 @@\n-  fs.write(\"log.txt\", user_input)\n+  if let Some(safe) = user_input.matches_regex(r\"^[a-zA-Z0-9 ]+$\") {\n+    fs.write(\"log.txt\", safe)\n+  }\n",
        "rationale": "Restricts the value to a known-safe character set.",
        "confidence": 0.92
      },
      {
        "label": "Apply a provably-correct sanitizer",
        "diff": "...",
        "rationale": "Routes the value through a typed sanitizer.",
        "confidence": 0.9
      },
      {
        "label": "Parse against an enum allowlist",
        "diff": "...",
        "rationale": "Narrows the value to enum variants.",
        "confidence": 0.85
      }
    ]
  },
  "see_also": ["MT4001", "docs/internals/taint-types.md"]
}
```

### Field reference

| Field            | Type                | Notes |
|------------------|---------------------|-------|
| `schema_version` | string              | v0.34 T4. Currently `"1.0"`. See **Versioning** below. |
| `code`           | string `MTxxxx`     | Stable diagnostic code. |
| `severity`       | string              | `"error"`, `"warning"`, `"note"`, `"help"`. |
| `span`           | object              | See **Span** below. |
| `title`          | string              | The primary label message. |
| `prose`          | string              | Multi-sentence explanation. Merges `mty explain <code>` text with any per-site notes/helps the diagnostic carried. |
| `fix`            | object (optional)   | Absent when no fix passes the 0.5 confidence floor. |
| `see_also`       | string[] (optional) | Related `MTxxxx` codes and doc paths. |
| `source`         | object (optional)   | Populated only with `--include-source`. |

### Versioning

The envelope wire shape is identified by the top-level
`schema_version` string. The compiler always stamps the current
constant; consumers should read it BEFORE inspecting the rest of the
envelope.

**Bump policy:**

- **Major bump (e.g. `"1.0"` → `"2.0"`)** — *breaking* change. The
  shape removed a field, renamed a field, changed a field's type, or
  repurposed an existing field. Consumers MUST check the major
  version and adapt (or refuse to parse, falling back to the pretty
  renderer). A major bump is announced in a `RELEASE-vX.Y.md` note
  and accompanied by a one-release window where the previous version
  is still produced under an opt-in flag.
- **Minor bump (e.g. `"1.0"` → `"1.1"`)** — *additive only*. A new
  optional field was added. Existing consumers can ignore the new
  field and keep working. Removing or renaming any pre-existing field
  is NEVER a minor bump.

**Forward-compatibility rule for consumers:** *accept unknown fields*.
The envelope deliberately does not use `#[serde(deny_unknown_fields)]`
so the v1.x line can ship additive minor versions without breaking
older agents. The compiler also defaults the field to `"1.0"` when
parsing legacy envelopes that omit it.

**Per-version changelog:**

| Version | Date       | Change |
|---------|------------|--------|
| `1.0`   | v0.33 T4   | Initial shape (code, severity, span, title, prose, fix, see_also, source). v0.34 T4 promotes it to an explicit field. |


### Span

```json
{ "file": "...", "line": 18, "col": 27, "len": 12, "byte_start": 412, "byte_end": 424 }
```

- `file` — the source-id given to the diagnostic pipeline (usually a
  path string).
- `line`, `col` — 1-indexed, matching the human MTxxxx output.
- `len` — byte length of the primary span.
- `byte_start`, `byte_end` — half-open byte offsets into the source.

### Fix

```json
{ "kind": "untaint", "confidence": 0.92, "alternatives": [...] }
```

- `kind` — discriminator for fix mechanism. See **FixKind** below.
- `confidence` — `max(alternatives[*].confidence)` — agents can
  short-circuit on the parent field without scanning every
  alternative.
- `alternatives` — non-empty list. Each carries `label`,
  `rationale`, `diff` (unified diff, LF-only), and per-alternative
  `confidence`.

### FixKind

JSON tag (lowercase snake-case):

| Tag                          | Typical codes              |
|------------------------------|----------------------------|
| `untaint`                    | MT4099                     |
| `missing_import`             | MT1002, MT2002             |
| `add_effect`                 | MT4001, MT4050, MT4055     |
| `add_capability`             | MT4010, MT4060             |
| `rename_to_match_decl`       | MT1001, MT2006, MT2007     |
| `wrap_in_some`               | MT2013 (field is Option)   |
| `type_conversion`            | MT2001, MT2018             |
| `declare_protocol_message`   | MT2026                     |
| `add_clone`                  | MT3001                     |
| `take_reference`             | MT3008                     |
| `add_mutability`             | MT3013, MT3014             |
| `balance_delimiters`         | MT0012                     |
| `add_match_arm`              | MT2015                     |
| `remove_unreachable`         | MT2016                     |
| `correct_macro_attr`         | MT6001, MT6017             |
| `add_return_type`            | MT0021, MT2019             |
| `add_type_annotation`        | MT2003, MT2020             |
| `add_struct_field`           | MT2013                     |
| `unpack_question`            | MT2010                     |
| `initialize_binding`         | MT3015                     |
| `other`                      | fall-back                  |

### Confidence scale

Confidence is a `f32` in `[0.0, 1.0]`. The compiler refuses to emit
fixes below `0.5`; in that case the envelope ships without a `fix`
field and the agent must rely on `prose` and `see_also` alone.

| Range       | Meaning                                                                 |
|-------------|-------------------------------------------------------------------------|
| `1.0`       | Mechanical — fix is structurally complete (e.g. an annotation insert).  |
| `0.9 – 0.99`| High — well-known idiom (taint untaint via regex/sanitizer/allowlist).  |
| `0.7 – 0.89`| Medium — likely correct (closest-spelling rename, single-line patch).   |
| `0.5 – 0.69`| Low — might apply; agent should weigh `see_also` and re-prompt.         |
| `< 0.5`     | Suppressed.                                                             |

### Source snippet

With `--include-source`:

```json
"source": {
  "start_line": 17,
  "lines": ["  let user_input = agent.ask(\"hi\")", "  fs.write(\"log.txt\", user_input)", "}"]
}
```

`start_line` is the 1-indexed line number of `lines[0]`. The window
defaults to one line of context on each side.

## Coverage matrix

v0.33 T4 ships per-code fix engines for **31 `MTxxxx` codes**:

| Tier | Codes |
|------|-------|
| Lex / parse | MT0012, MT0021 |
| HIR resolution | MT1001, MT1002 |
| Type-check | MT2001, MT2002, MT2003, MT2005, MT2006, MT2007, MT2010, MT2013, MT2015, MT2018, MT2019, MT2020, MT2021, MT2026 |
| Borrow | MT3001, MT3004, MT3005, MT3006, MT3013, MT3014, MT3015 |
| Effects / caps / taint | MT4001, MT4010, MT4032, MT4050, MT4055, MT4057, MT4059, MT4060, **MT4099 (marquee)** |
| Macros | MT6001, MT6017 |

All remaining codes still produce envelopes (sans `fix` field) so
agents can rely on `code` + `span` + `prose` for every diagnostic.

## NDJSON contract for downstream tools

- One JSON object per line.
- Lines are LF-terminated; **no** trailing blank line, **no**
  comments, **no** YAML/TOML preamble.
- Lines are valid UTF-8 + valid JSON; an agent may safely call
  `serde_json::from_str` (or equivalent) on each line independently.
- The exit code follows the existing `mty check` convention: 1 on
  any `error`-severity envelope, 0 otherwise.

## Versioning + back-compat

The envelope shape is stable for the entire v0.33 line. v0.34 will:

1. Add a top-level `schema_version` field defaulting to `"1.0"`.
2. Promote agent-mode (`mty agent-mode --transport stdio`) to a
   first-class CLI verb that streams envelopes the way the LSP server
   streams notifications today.
3. Wire the existing `mty explain <code>` text directly into the
   `prose` field so the two surfaces never drift.

Until then, agents should treat the schema as additive — unknown fields
must be ignored, present fields must keep their documented shape.

## Implementation pointers

- Envelope types: `crates/mty-diagnostics/src/fix.rs`.
- Per-code fix engines: `crates/mty-diagnostics/src/codes_fix.rs`.
- CLI integration: `crates/mty-cli/src/cmd/check.rs` (`--format json`).
- Example with the marquee MT4099 envelope:
  `examples/38_diag_envelopes.mty`.

## See also

- `docs/internals/taint-types.md` — MT4099 background.
- `docs/internals/diagnostics.md` — the human-renderer architecture.
- `docs/reference/cli/mty-check.md` — CLI reference.
