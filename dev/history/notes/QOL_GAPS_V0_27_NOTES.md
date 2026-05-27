# v0.27 Track E — QoL gap closure notes

## Scope

Three small but visible gaps surfaced by demo `07_research_agent`'s
v0.26 notes file. Each is independently scoped + independently
testable; the three together close the demo's "v0.27 follow-up" list
without touching the parser, the type-checker, or the wasm codegen
(which sibling tracks own this cycle).

## Gap 1 — `vector.is_empty()` predicate

### Background

`mty_stdlib::memory::VectorStore` already exposed `is_empty(&self) -> bool`
since v0.26 (mirroring `Vec::is_empty`), but:

1. The prelude's permissive method table didn't actively register it
   as a *known* method — only because `is_empty` happened to be in the
   v0.25 Track E list for `String`/`Vec[T]`. It worked by coincidence,
   not contract.
2. The SIR interpreter's `eval_method` dispatch returned `Bool(false)`
   for opaque receivers (which is how a `VectorStore` value reaches
   the interp — as `Value::Unit`), so demo 07's "skip indexing when
   the store is populated" gate would have evaluated to `false` on
   every run, defeating the predicate.

Demo 07 worked around both by re-indexing every run (the local backend
is idempotent on same id — same `upsert` rewrites the same record,
same `vector.json`).

### Fix

- `VectorStore::clear()` added alongside `is_empty()` so callers can
  reset between runs without losing the persisted-disk path. Records
  a `MemoryDelta::Patch { op: "clear", .. }` so v0.19 replay traces
  carry the truncation.
- Interpreter `eval_method` for `is_empty` returns `Bool(true)` on
  `Value::Unit` receivers (the opaque-handle placeholder), so demo
  07's gate evaluates to `true` on first run and the indexing pass
  fires. The real Rust handle reaches `is_empty` through the
  `mty_stdlib` ctor path in v0.28's opaque-handle lift.
- Prelude permissive table picks up `is_empty` + `clear` explicitly
  next to the v0.26 `messages`/`complete`/`complete_stream` lines so
  the contract is documented at the registration site.

### Tests

`crates/mty-stdlib/tests/memory_vector_is_empty.rs`:

- `vector_new_is_empty` — fresh local store reports empty
- `vector_after_upsert_not_empty` — one `upsert` → non-empty
- `vector_after_clear_is_empty` — multi-upsert + `clear()` → empty
- `vector_after_delete_last_is_empty` — `delete()` of last record
- `vector_clear_persists_to_disk` — round-trips through ctor
- `qdrant_constructor_is_empty` — qdrant cached-records reports empty

Total: 6 tests, all green.

## Gap 2 — Source-level streaming surface

### Background

`AnthropicClient::complete_stream(req)` returns a typed
`MessageStream` — a `Stream<Item = Result<MessageDelta, LlmError>>`.
At the Rust API this was complete since v0.26 (the SSE parser tests
pin the per-delta shape), but Mighty source couldn't iterate it
cleanly:

- `MessageStream` wasn't a registered opaque ADT — `mty check` would
  reject `stream: MessageStream` as an unknown type.
- The `next` method wasn't in the permissive table — even after the
  ADT was added, `stream.next()` failed strict-scope MT2021.
- No synchronous adapter — the SIR interp's `eval_method` doesn't
  `await`, so even a permissive registration couldn't drive the
  underlying poll.

### Fix

- `MessageStream::next() -> Option<MessageDelta>` (async, inherent
  method shadowing `StreamExt::next` — the inherent one collapses
  stream errors to `Done { stop_reason: "stream_error: ..." }` so
  Mighty source can drive the loop without rich error handling).
- `MessageStream::next_blocking() -> Option<MessageDelta>` —
  synchronous adapter. Uses `tokio::task::block_in_place` +
  `Handle::block_on` when called from a multi-thread runtime context;
  spins a `current_thread` runtime otherwise. This is the entry
  point the SIR interp's eval_method will dispatch to once the
  opaque-handle lift lands (v0.28).
- `MessageStream` registered in the prelude as a handler-safe opaque
  ADT (alongside `AnthropicClient`/`Message`/`VectorStore`/...).
- `next` + `messages_stream` added to the permissive method table.
- SIR interp `eval_method` arm for `next` on opaque receivers returns
  `Option::None` so Mighty source's `while`-loop terminates rather
  than wedging on an unimplemented call.

### Tests

`crates/mty-stdlib/tests/llm_streaming_source.rs`:

- `messagestream_next_yields_deltas` — async iteration
- `messagestream_next_blocking_drives_from_sync_context` — sync
- `messagestream_handles_tool_use_delta` — tool-use variant
- `messagestream_collapses_errors_to_done` — error path

Total: 4 tests, all green. Sibling tests inside
`crates/mty-stdlib/src/llm/streaming.rs` cover three more cases:
`next_yields_deltas_then_none`, `next_blocking_yields_deltas_then_none`,
`next_collapses_stream_errors_to_done`.

The Mighty-source test is **deferred** until the parser ships `while
let Some(d) = stream.next() { ... }` pattern bindings (v0.28); the
`examples/29_streaming.mty` example pins a bounded-loop variant of the
same iteration shape so `mty check` exercises the typed handle path.

## Gap 3 — `mty run <path> -- <argv>` positional forwarding

### Background

`mty run demos/07_research_agent/src/main.mty -- "What does std.memory do?"`
should make `std.env.args().nth(1)` return `"What does std.memory do?"`.
v0.26 silently discarded the `--` tail; demo 07's smoke.sh notes the
gap and hard-codes the canonical seed question.

### Fix

- `Cmd::Run` in `mty-cli/src/main.rs` gains an `argv: Vec<String>`
  field annotated `#[arg(last = true)]`. Clap collects everything
  after the `--` separator into this vector.
- `mty-cli/src/cmd/run.rs::run` accepts the new `argv` and calls
  `mty_stdlib::env::set_args(argv)` before invoking the runtime.
- `mty-stdlib::env` is the new module that owns the process-wide
  argv channel. `set_args` writes the channel; `args` reads it. The
  channel is an `OnceLock<RwLock<Vec<String>>>` so it's both
  thread-safe and idempotent (last write wins for tests).
- Prelude registers `std.env` as an opaque module (alongside
  `std.io`/`std.fs`/`std.net`/...).
- `std.env.args` registered in the permissive method table.
- `mty_stdlib::host::dispatch` routes `("std.env", "args")` through
  `crate::env::args()` and wraps the result as
  `Value::Array(Vec<Value::Str>)` for the Mighty side.

### Convention

The leading positional (after `--`) lands at index 0. That matches
`std::env::args().skip(1)` semantics from a user perspective —
Mighty source treats `std.env.args()` as "the args this Mighty program
received," not "the OS-level argv-with-binary."

### Tests

`crates/mty-cli/tests/cmd_run_argv.rs`:

- `run_with_positional_argv_reaches_std_env_args` — single positional
- `run_without_argv_returns_empty` — bare `mty run`
- `run_with_multiple_positionals` — three positionals
- `run_argv_preserves_quoted_strings_with_spaces` — spaces survive
- `env_args_channel_round_trips` — direct API assertion

Total: 5 tests, all green.

## Out of scope (deferred to v0.28)

- `MessageStream`'s opaque-handle materialisation. The SIR interp
  still threads it as `Value::Unit`; the real handle reaches the
  interpreter only via the future opaque-value lift that Track A is
  scoping.
- `while let Some(d) = stream.next() { ... }` parser arm. The
  bounded-loop workaround in `examples/29_streaming.mty` keeps the
  example checking until the parser is unlocked.
- `for delta in stream` desugaring on opaque-iterator receivers. Same
  blocker as `while let`; tracked alongside the v0.28 iterator-protocol
  unification.
- `argv` forwarding under `mty build --target wasm32-wasi`. The
  wasi:cli/environment binding is a separate v0.28 wave.

## Files touched

EXTENDED:

- `crates/mty-stdlib/src/lib.rs` — declare `mod env`.
- `crates/mty-stdlib/src/host.rs` — dispatch `("std.env", "args")`.
- `crates/mty-stdlib/src/llm/streaming.rs` — `MessageStream::next` +
  `next_blocking`.
- `crates/mty-stdlib/src/memory/vector.rs` — `VectorStore::clear`.
- `crates/mty-types/src/prelude.rs` — register `std.env`,
  `MessageStream`, `next`, `args`, `messages_stream`.
- `crates/mty-ir/src/interp/run.rs` — `is_empty` on `Unit`, `next`
  arm for opaque receivers.
- `crates/mty-cli/src/main.rs` — `Cmd::Run { argv }`.
- `crates/mty-cli/src/cmd/run.rs` — forward argv to
  `mty_stdlib::env::set_args`.

CREATED:

- `crates/mty-stdlib/src/env.rs`
- `crates/mty-stdlib/tests/memory_vector_is_empty.rs`
- `crates/mty-stdlib/tests/llm_streaming_source.rs`
- `crates/mty-cli/tests/cmd_run_argv.rs`
- `examples/29_streaming.mty`
- `dev/history/notes/QOL_GAPS_V0_27_NOTES.md` (this file)
