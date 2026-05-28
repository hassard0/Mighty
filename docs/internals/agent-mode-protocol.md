# `mty agent` — Agent-Mode CLI Protocol

> v0.33 T5 — structured JSON-over-stdio protocol that lets LLM agents
> drive Mighty natively, without scraping human-rendered console output.

## Overview

`mty agent` wraps every existing `mty` subcommand in a single
NDJSON-over-stdio protocol. The wire format is line-delimited JSON:

* One JSON object per line on stdin: the **request**.
* Zero or more JSON objects per line on stdout: the **response stream**.
* A single terminator line per request: `{"kind":"done", ...}`.

This contract is consumed by:

1. LLM-agent frameworks (Claude Code, GPT/codex, Mighty's own swarm),
   which spawn `mty agent` as a long-lived subprocess and route JSON.
2. Mighty's CI rigging (`mty agent --single-shot < req.json`) for
   scripted one-shots.
3. The v0.34 HTTP and Unix-domain-socket transports (stubbed in T5).

The protocol consumes the structured `DiagnosticEnvelope` shape T4
shipped — see `docs/internals/diagnostic-envelopes.md`.

## Transport

Default transport is **stdio** (line-delimited JSON over stdin/stdout).

```
mty agent                          # interactive; loops until {"op":"halt"} or EOF
mty agent --single-shot < req.json # one request, one response, exit
```

Optional transports (v0.34 stubs):

```
mty agent --transport http --port 8888   # POST /v1/agent, NDJSON body
mty agent --transport unix --socket /tmp/mty-agent.sock
```

These two stubs reject with a `kind:"error"` envelope explaining v0.34.

## Wire format

### Requests

Every request is a single-line JSON object. Required key: `"op"`.

```json
{"op": "check",  "path": "src/main.mty", "include_source": true}
{"op": "run",    "path": "src/main.mty", "args": ["--", "alpha"]}
{"op": "test",   "eval": true, "replay_only": true, "manifest_dir": "tests/"}
{"op": "inspect","view": "cost", "since": "24h", "format": "json"}
{"op": "find",   "query": "write files", "top": 5}
{"op": "explain","code": "MT4099"}
{"op": "fmt",    "path": "src/main.mty", "check": false}
{"op": "fix",    "path": "src/main.mty", "code": "MT4099", "alternative": 0, "write": false}
{"op": "halt"}
```

Unknown keys are ignored. Type mismatches surface as `kind:"error"`
envelopes (parse error returned to the client; the loop continues).

### Responses

Every response message is a single-line JSON object with a `"kind"`
discriminator:

| `kind`     | Meaning |
|------------|---------|
| `envelope` | One `DiagnosticEnvelope` (T4 shape). See diagnostic-envelopes.md. |
| `log`      | One captured `stream` line (`stderr` or `stdout`). |
| `progress` | Optional progress notification: `phase` (str), `pct` (0..1). |
| `result`   | Per-request summary: `op`, `ok`, and op-specific counters. |
| `patch`    | (For `fix`) the candidate diff + optionally the resulting file content. |
| `error`    | Protocol-level error (bad JSON, unknown op, missing file). |
| `done`     | Terminator with `exit_code` (`i32`). Always last. |

#### `envelope`

Exactly the T4 `DiagnosticEnvelope` shape with `"kind":"envelope"`
prepended:

```json
{"kind":"envelope","code":"MT4099","severity":"error",
 "span":{"file":"src/main.mty","line":18,"col":27,"len":12,
         "byte_start":420,"byte_end":432},
 "title":"tainted value flows to fs.write",
 "prose":"...",
 "fix":{"kind":"untaint","confidence":0.92,
        "alternatives":[{"label":"regex untaint","diff":"--- a/...","rationale":"...","confidence":0.92}]}}
```

#### `log`

```json
{"kind":"log","stream":"stderr","text":"compiling mty-stdlib v0.1.0..."}
```

`text` is one line of captured output. Multi-line bodies are split on
`\n` before emission.

#### `progress`

```json
{"kind":"progress","phase":"typecheck","pct":0.42}
```

Emitted opportunistically; agents may ignore.

#### `result`

```json
{"kind":"result","op":"check","ok":false,
 "diagnostics_count":3,"fix_count":2}
```

Per-op fields:

* `check`: `ok`, `diagnostics_count`, `fix_count`.
* `run`: `ok`, `exit_code`.
* `test`: `ok`, `passed`, `failed`.
* `inspect`: `ok` (raw payload also streamed as `log` lines).
* `find`: `ok`, `hits` (list of objects).
* `explain`: `ok`, `code`, `text`.
* `fmt`: `ok`, `would_reformat` (bool).
* `fix`: `ok`, `applied` (bool), `diff_len`.
* `halt`: `ok=true`.

#### `patch`

Emitted by the `fix` op. Always includes the chosen alternative's
diff; `new_content` is populated only when the request set `write`
or `dry_run` (the agent gets to see the resulting file body either
way before deciding to commit).

```json
{"kind":"patch","applied":false,"diff":"--- a/src/main.mty\n+++ b/...\n@@ -18,1 +18,1 @@\n-...\n+...\n",
 "new_content":"...full file body..."}
```

#### `error`

```json
{"kind":"error","message":"unknown op: foo"}
{"kind":"error","message":"malformed JSON: expected `:` at column 14"}
{"kind":"error","message":"path not found: src/main.mty"}
```

#### `done`

```json
{"kind":"done","exit_code":0}
```

Always the last line emitted for a request. `exit_code` mirrors the
exit code that the equivalent `mty <subcommand>` invocation would
return.

## Ops in detail

### `check`

Wraps `mty check --format json`. Emits one `envelope` per diagnostic
(T4 shape), then a `result` summary, then `done`.

Request keys:

| key              | type   | default | meaning |
|------------------|--------|---------|---------|
| `path`           | str    | required | source file |
| `include_source` | bool   | false   | embed 3-line snippet in each envelope |

### `run`

Wraps `mty run`. Captures stdout + stderr as `log` lines, then a
`result` with the program's exit code.

Request keys:

| key      | type      | default | meaning |
|----------|-----------|---------|---------|
| `path`   | str       | required | source file |
| `args`   | list[str] | []      | forwarded to `std.env.args()` |
| `legacy_interp` | bool | false | use slice-6 tree-walker |

### `test`

Wraps `mty test [--eval]`. Streams per-suite status as `log` lines and
emits a `result` summary at the end.

Request keys:

| key            | type | default | meaning |
|----------------|------|---------|---------|
| `manifest_dir` | str  | cwd     | discovery root |
| `eval`         | bool | false   | run `*.eval.mty` instead |
| `replay_only`  | bool | false   | only score cached traces |
| `ci`           | bool | false   | use `[eval.ci]` block from `mighty.toml` |
| `no_strict`    | bool | false   | don't fail on errored cells |

### `inspect`

Wraps `mty inspect [--cost]`. Default `view` is `"agents"`; `"cost"`
flips to the cost-snapshot path.

Request keys:

| key      | type | default | meaning |
|----------|------|---------|---------|
| `view`   | str  | "agents" | "agents" or "cost" |
| `sock`   | str  | env     | control-socket path |
| `agent`  | u64  | none    | single-agent snapshot |
| `since`  | str  | "24h"   | cost window |
| `by`     | str  | "provider" | cost group key |
| `top`    | usize| none    | top-N expensive calls |
| `db`     | str  | env     | observations sqlite |

### `find`

Lightweight free-text search over the current package's source
files (and Markdown docs under `docs/`). v0.33 T5 ships a literal
substring matcher; the v0.34 follow-up wires this to the v0.33 T1
RAG index.

Request keys:

| key     | type   | default | meaning |
|---------|--------|---------|---------|
| `query` | str    | required | substring to search for |
| `top`   | usize  | 10      | max number of hits |
| `root`  | str    | cwd     | search root |

Response: `result.hits` is a list of `{file, line, text}` records.

### `explain`

Wraps `mty explain`. Returns the long-form prose for one `MTxxxx`
code in `result.text`.

Request keys:

| key    | type | default | meaning |
|--------|------|---------|---------|
| `code` | str  | required | e.g. `"MT4099"` |

### `fmt`

Wraps `mty fmt`. With `check: true`, just reports whether the file
would reformat; otherwise rewrites in place.

Request keys:

| key     | type | default | meaning |
|---------|------|---------|---------|
| `path`  | str  | required | source file |
| `check` | bool | false   | check-only mode |

### `fix`

The flagship agent-mode op. Re-runs `check` on `path`, finds the
envelope whose `code` matches, picks alternative #`alternative` (0
by default), and either previews or applies the diff.

Request keys:

| key           | type  | default | meaning |
|---------------|-------|---------|---------|
| `path`        | str   | required | source file |
| `code`        | str   | required | diagnostic code to match (e.g. `"MT4099"`) |
| `alternative` | usize | 0       | which fix alternative |
| `write`       | bool  | false   | persist the diff to disk |

Response: a single `patch` envelope with `applied` (bool), `diff`,
and (when `write` is true) `new_content` (the post-patch file body).

### `halt`

Cleanly exits the interactive loop. Emits a single
`{"kind":"done","exit_code":0}` and stops reading stdin.

## Single-shot mode

`mty agent --single-shot < req.json` reads exactly one request from
stdin (the whole stdin body, parsed as one JSON object), runs it,
and exits with the same exit code the equivalent `mty <subcommand>`
would have used. Multiple requests in a single-shot payload are an
error (the second-and-later objects are surfaced as `kind:"error"`
and ignored).

## Interactive state

Between requests, the loop remembers:

* `last_path` — last `path` argument the agent operated on.
* `last_envelopes` — the most recent `check` envelopes (used by `fix`
  to look up the diagnostic by code).
* `last_source` — the source text the last `check` was run against.

The state is per-process; closing stdin (or sending `halt`) drops it.
This lets an agent run `check` then `fix` without re-sending the path.

## Exit codes

Same convention as the underlying `mty <subcommand>`:

| code | meaning |
|------|---------|
| 0    | success |
| 1    | normal error (diagnostics, failed test, missing file) |
| 2    | protocol error (bad JSON, unknown op, transport failure) |

## Forward compatibility

* New op kinds: agents that don't understand `"op":"foo"` MUST still
  read the response stream and look for `kind:"error"` or `kind:"done"`.
* New response kinds: agents MUST ignore unknown `kind` values rather
  than failing.
* New fields on existing kinds: forward-compatible; old agents ignore.

## Reference implementation

The reference implementation lives at
`crates/mty-cli/src/cmd/agent.rs`. It is ~700 LOC plus 25+ tests
under `crates/mty-cli/tests/agent_mode.rs`.

The CLI wrapper lives in `crates/mty-cli/src/main.rs` (look for
`Cmd::Agent`).
