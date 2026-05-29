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
mty agent --transport http --listen 0.0.0.0:9090
mty agent --transport http --listen 127.0.0.1:9090 --auth-token s3cret
mty agent --transport unix --listen /tmp/mty-agent.sock
mty agent --record session.ndjson            # capture every (req, resp)
mty agent --replay session.ndjson            # re-run + assert byte-match
```

## Arguments

None. All work is driven by NDJSON requests on stdin.

## Options

| Flag | Default | Purpose |
|---|---|---|
| `--single-shot` | off | Read exactly one JSON object from stdin, run it, exit with that op's exit code. (stdio only) |
| `--transport <KIND>` | `stdio` | One of `stdio`, `http`, `unix`. All three are fully implemented since v0.35 T2. |
| `--port <N>` | 8889 | HTTP transport convenience shortcut for `--listen 127.0.0.1:<N>`. |
| `--socket <PATH>` | — | Unix transport convenience shortcut for `--listen <PATH>`. |
| `--listen <ADDR>` | — | `host:port` for HTTP, or a path for unix. Wins over `--port` / `--socket` when both are set. |
| `--auth-token <T>` | off | Bearer token required on every HTTP request. Returns 401 on missing / wrong token. Ignored under stdio + unix transports. |
| `--record <PATH>` | off | Append every `(request, response)` pair to this NDJSON file. Works under every transport. |
| `--replay <PATH>` | off | Read a previously recorded file, re-run every request, assert each live response byte-matches the recorded one. Exits 0 on match, 1 on drift, 2 on read/parse errors. |

## Behavior

* **stdio (default)** — Reads NDJSON requests, dispatches each to the
  corresponding subcommand handler, captures its stdout + stderr,
  emits structured `kind:"log"` / `kind:"envelope"` / `kind:"result"`
  / `kind:"done"` lines back on stdout. Loops until an `{"op":"halt"}`
  request, an EOF, or a fatal protocol error.
* **single-shot** — Reads the entire stdin body as one JSON object,
  runs it, exits with the wrapped op's exit code.
* **http** — Binds a TCP listener. Exposes `POST /v1/agent`,
  `POST /v1/agent/batch`, and `GET /v1/agent/version`. With
  `--auth-token`, every request must carry `Authorization: Bearer
  <token>`. See `docs/internals/agent-mode-protocol.md` for the wire
  format.
* **unix** — Binds a `tokio::net::UnixListener` at the given path.
  Same line-delimited JSON protocol as stdio, one independent
  session per connection. Pre-existing socket files at the path are
  unlinked. On Windows the agent prints a one-line `kind:"error"`
  envelope and exits 2 (Unix sockets aren't supported by this
  binary today).
* **record / replay** — `--record` is a session-trace recorder
  usable under any transport. `--replay` is its inverse: read a
  recorded file, re-run every request against a fresh session, and
  assert each response byte-matches.

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

### HTTP transport

```bash
# Terminal A — start the server
mty agent --transport http --listen 127.0.0.1:9090 --auth-token s3cret

# Terminal B — drive it
curl -sS -H 'Authorization: Bearer s3cret' \
  http://127.0.0.1:9090/v1/agent/version
# {"agent_protocol":"1.0","mty_version":"0.1.0"}

curl -sS -H 'Authorization: Bearer s3cret' \
  -d '{"op":"explain","code":"MT0001"}' \
  http://127.0.0.1:9090/v1/agent
# NDJSON response stream, ending with {"kind":"done","exit_code":0}

# Batch mode — multiple NDJSON requests, share session state
printf '%s\n%s\n' \
  '{"op":"check","path":"src/main.mty"}' \
  '{"op":"fix","code":"MT4099","write":true}' | \
  curl -sS -H 'Authorization: Bearer s3cret' \
    --data-binary @- \
    http://127.0.0.1:9090/v1/agent/batch
```

### Unix socket transport

```bash
# Server
mty agent --transport unix --listen /tmp/mty-agent.sock &

# Client — bash + ncat / socat
echo '{"op":"explain","code":"MT0001"}' | nc -U /tmp/mty-agent.sock
```

### Recorded sessions

```bash
# Record one session — works under any transport.
mty agent --single-shot --record /tmp/sess.ndjson < req.json

# Replay it later — exits 0 if every response still byte-matches.
mty agent --replay /tmp/sess.ndjson
```

The replay path is the easiest way to catch unintentional changes to
the agent protocol's wire output: check a `sess.ndjson` into git and
fail CI when it drifts.

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
