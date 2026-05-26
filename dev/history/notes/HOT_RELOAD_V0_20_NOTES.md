# Hot reload v0.20 design notes

Slice: agent-features-roadmap Tier 1.5
Window: v0.20 swarm (single-agent slice)
Owner: hot-reload sub-agent
Scope: `crates/mty-runtime/src/reload/`, `crates/mty-cli/src/cmd/reload.rs`,
       `docs/internals/hot-reload.md`, `docs/reference/cli/mty-reload.md`,
       `crates/mty-runtime/tests/reload.rs`.
Out of scope: every existing runtime hot path, codegen, cluster,
       supervisors, mailbox internals.

## What ships

1. `Resumable` trait + default `ciborium` codec + content-addressable
   FNV-1a schema-hash helper (`compute_schema_hash`).
2. Swap pipeline (`reload::swap`): pause → drain → snapshot → schema
   check → restore → resume. `ReloadGate` for the pause/busy state.
   `ReloadReport` for the outcome. `ReloadError` for the `MT506x`
   diagnostic family.
3. `ModuleSource::SameProgram` (state-only restart) wired end-to-end.
   `ModuleSource::WasmBytes(_)` rejected with `MT5064` until v0.21.
4. `mty reload <agent-type> --from new.wasm` CLI with `--dry-run`,
   `--deadline-ms`, `--sock`, `--json`. Wire shape matches the v0.16
   `mty inspect` control-socket contract.
5. Tests: `crates/mty-runtime/tests/reload.rs` — 9 cases covering
   compatible swap, incompatible-schema reject, drain wait, deadline
   trip, mailbox preservation, raw-wasm reject, and pure-data
   helpers.
6. Docs: `docs/internals/hot-reload.md` (architecture + v0.21
   follow-up), `docs/reference/cli/mty-reload.md` (CLI usage).

## Design decisions

### Mailbox-preserving swap

The mailbox is the only artifact the swap explicitly keeps alive
across the agent-id boundary. Producers hold a `Sender` clone on
`AgentHandle::mailbox`, so they never observe the swap; the gate
ensures handlers don't dispatch during the swap.

### Schema hash is a const, not a runtime computation

Putting `SCHEMA_HASH` on the trait as a `const` lets the swap
pipeline short-circuit *before* it touches the deserialiser. If we
deferred the check to deserialise-time, an incompatible payload
would trap inside the user agent's code and surface as `MT5005`
rather than the specific `MT5060`. The const form costs nothing at
runtime and gives the CLI an actionable diagnostic.

### Default codec = ciborium

`ciborium` is already a workspace dep (the cluster wire uses it).
Using it for the snapshot keeps the dep graph tight and matches what
the cluster live-migration follow-up will need anyway. Users who
want a different codec just override `to_snapshot`/`from_snapshot`.

### Drain via busy-poll, not condvar

A condvar would require touching the agent loop's per-frame
hot-path. v0.20 instead busy-polls with a 1 ms sleep — at worst this
adds 1 ms to the swap latency. The v0.21 follow-up replaces the
busy-poll with a condvar wake-up emitted on `mark_idle()`.

### `WasmBytes` is rejected today

Loading a fresh wasm module per-agent requires
`Program::with_swapped_agent(...)` on the IR, which doesn't exist
yet. v0.20 stops at the API boundary so the CLI + cluster
live-migration caller can record intent today and the wire
contract stays stable when v0.21 lands the loader.

## Diagnostic codes

| Code | Variant | Meaning |
|------|---------|---------|
| MT5060 | `IncompatibleSchema` | New module's hash doesn't satisfy the live agent's |
| MT5061 | `AgentNotFound` | No live agent with the requested name/id |
| MT5062 | `DrainDeadline` | Handler didn't return inside `--deadline-ms` |
| MT5063 | `Snapshot` | Snapshot encode/decode failed |
| MT5064 | `WasmReloadNotImplemented` | v0.20 only supports `SameProgram` |
| MT5069 | `Internal` | Catch-all for runtime errors during reload |

MT5060-MT5069 is a fresh band — the existing `error.rs` uses
MT5001-MT5050 for the core runtime errors, so we reserve MT506x for
reload concerns. The reload errors implement
`Into<RuntimeError>` (mapping to `Trap { code: <MT506x>, ... }`) so
existing error-handling paths absorb them transparently.

## Tests

```text
$ cargo test -p mty-runtime --test reload
   Compiling mty-runtime v0.1.0
    Finished test [unoptimized + debuginfo] target(s)
     Running tests/reload.rs

running 9 tests
test reload_compatible_schema_succeeds ... ok
test reload_incompatible_schema_rejected ... ok
test reload_drains_in_flight_handler ... ok
test reload_deadline_exceeded_fails_clean ... ok
test reload_raw_wasm_rejected_in_v0_20 ... ok
test dry_run_swap_matches_full_runner_for_state_only ... ok
test resumable_default_codec_round_trip ... ok
test snapshot_size_cap_trips_for_huge_payloads ... ok
test reload_preserves_mailbox ... ok
```

Plus inline unit tests in:
- `crates/mty-runtime/src/reload/resumable.rs` (7 cases)
- `crates/mty-runtime/src/reload/swap.rs` (5 cases)
- `crates/mty-cli/src/cmd/reload.rs` (3 cases — base64, JSON
  escape, pretty-print)

## Concurrency notes

The reload module is additive. No existing runtime hot path consumes
it; the gate is only consulted by code that opts in. The runtime
tasks list (`Runtime::tasks`) and the agent registry are untouched.
This means a v0.20 build with reload-aware agents costs the same as
a v0.19 build for any agent that doesn't impl `Resumable`.

## v0.21 follow-up

See `docs/internals/hot-reload.md#v021-follow-up`:

1. Wasm module reload proper (per-agent `Program` slot).
2. Schema-evolution ranges (`migrate_from` hooks).
3. Multi-version support during rolling cluster restart.
4. Control-socket `op=reload` handler in
   `crates/mty-runtime/src/control_socket.rs` (off-limits to this
   slice; the CLI ships ahead of the runtime listener).
5. `agent_id` resolution from `agent_type` (registry lookup by
   name; today the swap pipeline takes an `AgentDescriptor` directly,
   which works for the trait-level tests but the control-socket
   wire-up will need a name→id lookup).
6. Condvar-based drain wake-up to replace the 1 ms busy-poll.
