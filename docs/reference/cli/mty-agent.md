# mty agent

Run Mighty's agent-mode CLI: a structured JSON-over-stdio protocol
that lets an LLM agent (or any non-human caller) drive every other
`mty` subcommand without scraping human-rendered output.

The wire format is documented in
[`docs/internals/agent-mode-protocol.md`](../../internals/agent-mode-protocol.md).
This page covers the human-facing CLI knobs.

## Synopsis

```
mty agent                                    # interactive stdio loop
mty agent --single-shot < req.json           # one request, exit
mty agent --transport stdio                  # explicit stdio
mty agent --transport http --port 8888       # v0.34 stub (errors out cleanly)
mty agent --transport unix --socket /tmp/x   # v0.34 stub (errors out cleanly)
```

## Arguments

None. All work is driven by NDJSON requests on stdin.

## Options

| Flag | Default | Purpose |
|---|---|---|
| `--single-shot` | off | Read exactly one JSON object from stdin, run it, exit with that op's exit code. |
| `--transport <KIND>` | `stdio` | One of `stdio`, `http`, `unix`. http + unix are v0.34 stubs. |
| `--port <N>` | 8889 | Used with `--transport http`. |
| `--socket <PATH>` | — | Used with `--transport unix`. |

## Behavior

* **stdio (default)** — Reads NDJSON requests, dispatches each to the
  corresponding subcommand handler, captures its stdout + stderr,
  emits structured `kind:"log"` / `kind:"envelope"` / `kind:"result"`
  / `kind:"done"` lines back on stdout. Loops until an `{"op":"halt"}`
  request, an EOF, or a fatal protocol error.
* **single-shot** — Reads the entire stdin body as one JSON object,
  runs it, exits with the wrapped op's exit code.
* **http / unix (v0.34 stubs)** — Print a one-line `kind:"error"`
  envelope explaining the transport is reserved for v0.34, then exit 2.

## Ops at a glance

| op        | wraps            |
|-----------|------------------|
| `check`   | `mty check --format json` |
| `run`     | `mty run`        |
| `test`    | `mty test [--eval]` |
| `inspect` | `mty inspect [--cost]` |
| `find`    | substring search across `*.mty` + `docs/*.md` (RAG-backed in v0.34) |
| `explain` | `mty explain`    |
| `fmt`     | `mty fmt`        |
| `fix`     | check-then-patch using a T4 fix alternative |
| `halt`    | clean shutdown of the interactive loop |

See the protocol document for full request / response schemas.

## Examples

### One-shot from a shell script

```bash
echo '{"op":"check","path":"src/main.mty","include_source":true}' \
  | mty agent --single-shot
```

Output (one JSON object per line):

```json
{"kind":"envelope","code":"MT4099","severity":"error",...}
{"kind":"result","op":"check","ok":false,"diagnostics_count":1,"fix_count":1}
{"kind":"done","exit_code":1}
```

### Interactive session

```bash
mty agent
# stdin (one line per request):
{"op":"check","path":"src/main.mty"}
{"op":"fix","path":"src/main.mty","code":"MT4099","alternative":0,"write":true}
{"op":"halt"}
```

The session loops until `halt` (or stdin closes). Between requests,
the loop remembers the last `check` result so `fix` can look up the
diagnostic to patch without re-reading the file.

### Explain a code

```bash
echo '{"op":"explain","code":"MT4099"}' | mty agent --single-shot
```

### Fix preview (no write)

```bash
echo '{"op":"fix","path":"src/main.mty","code":"MT1001"}' \
  | mty agent --single-shot
```

Returns a `kind:"patch"` line with the candidate diff but does not
mutate the file.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | All requests in the session succeeded. |
| 1 | At least one wrapped op exited nonzero (e.g. diagnostics, failed test). |
| 2 | Protocol error (bad JSON, unknown op, unsupported transport). |

## Use this when

* You're an LLM agent and want machine-readable Mighty output.
* You're scripting CI workflows that need fix-application
  (`check` → `fix --write` → re-`check`).
* You're building tooling on top of Mighty (a VS Code panel, an LSP
  client extension, an internal dashboard) and want one stable JSON
  surface instead of N subprocess parsers.

## Don't use this when

* You're a human at a TTY — use `mty check`, `mty run`, etc. directly.
* You need DAP semantics (breakpoints, stepping) — use `mty dap`.
* You need streaming LSP completions — use `mty lsp`.
