# Hot reload (Tier 1.5)

v0.20 ships the state-preserving hot-reload pipeline described in
`docs/internals/agent-features-roadmap.md` Tier 1.5. This document
covers the architecture, schema-hash design, mailbox-preservation
guarantees, and the v0.21 follow-up items.

## TL;DR

1. CLI issues `mty reload <agent-type> --from new.wasm` to the
   runtime's control socket.
2. Runtime locates the live agent by type and pauses dispatch.
3. The in-flight handler (if any) is allowed to finish, bounded by a
   wall deadline.
4. State is serialised through the agent's `Resumable` impl into
   opaque bytes.
5. The new wasm module is loaded; a fresh agent of the same type is
   spawned alongside the old one.
6. State is decoded back via the new module's `Resumable::from_snapshot`.
7. The mailbox is re-attached to the new agent.
8. The old agent is unregistered; the new one resumes.

Steps 1-4 + 6-8 land in v0.20. Step 5 (the wasm load itself) gates
on the runtime's per-agent module surface, which lands in v0.21.

## Architecture diagram

```text
   producer       producer
   (HTTP)         (other agent)
       \             /
        \           /
   ┌────────────────────┐
   │  Arc<Mailbox>      │  ← shared across the swap
   │  (Sender side)     │
   └────────┬───────────┘
            │  receiver
            ▼
   ┌──────────────────────────────────────────────────────┐
   │  Agent #N (old wasm module)                          │
   │   • ReloadGate.busy   ← set while handler runs       │
   │   • ReloadGate.paused ← set during the swap          │
   │   • State<T> (typed cell, snapshot via Resumable)    │
   └──────────────────────────────────────────────────────┘
                            │  swap pipeline (sync)
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │  Agent #N (NEW wasm module, same id)                 │
   │   • State<T> restored from snapshot bytes            │
   │   • Receiver reattached to the same mailbox          │
   └──────────────────────────────────────────────────────┘
```

Crucially, **the `Arc<Mailbox>` is never replaced**. Producers
holding a `Sender` clone (via `AgentHandle::mailbox`) keep sending
into the same channel; the gate ensures handlers don't dispatch
during the swap, and the new agent picks up the next frame as soon
as the gate clears.

## `Resumable` trait

```rust
pub trait Resumable: Sized + Serialize + DeserializeOwned {
    const SCHEMA_HASH: u64;
    fn from_snapshot(bytes: &[u8]) -> ResumableResult<Self> { ... }
    fn to_snapshot(&self) -> ResumableResult<Vec<u8>> { ... }
    fn schema_compatible_with(other_hash: u64) -> bool {
        Self::SCHEMA_HASH == other_hash
    }
}
```

The default codec is `ciborium` — the same wire the cluster
transport uses, already in the workspace dep tree. Implementors can
override `to_snapshot` / `from_snapshot` to use any other
serde-compatible format (postcard, bincode) without changing the
swap pipeline's contract.

### Schema hash design

The hash is a **content address** of the state shape. Two structs
with identical fields produce the same hash, regardless of:

- Source order of the fields
- Crate or module the type was defined in
- Whether the type was renamed (only the field set matters)

The reference helper `compute_schema_hash(&[("count", "u64"),
("label", "String")])` runs FNV-1a 64-bit over the
lexicographically-sorted `(field_name, type_tag)` pairs, with
explicit separators between elements so e.g. `("ab", "c")` doesn't
collide with `("a", "bc")`.

The trait declares the hash as `const SCHEMA_HASH` so the swap
pipeline can short-circuit *before* it deserialises a snapshot —
otherwise an incompatible payload would trap inside the user's
deserialiser, surfacing as a generic `MT5005` trap rather than the
specific `MT5060 incompatible schema`.

#### Why not just check `Serialize + DeserializeOwned` equality?

Two types can round-trip through serde without being shape-compatible
(e.g. a `Vec<u8>` payload deserialising into a `String` if the bytes
happen to be valid UTF-8). The schema hash is the contract; serde is
the transport. Splitting the two means we can reject incompatible
swaps with an actionable diagnostic.

## Why we don't just restart

A naïve "kill + respawn" workflow loses three things:

1. **Mailbox contents.** Any messages enqueued between drain and
   respawn would be dropped on the floor. With the gate-based swap,
   producers keep enqueuing into the same `Arc<Mailbox>` and the new
   agent picks them up.
2. **Agent identity.** The supervisor's restart counters, the
   `agent_id` consumers reference, and the introspection ring buffer
   all keep their continuity.
3. **In-flight reply channels.** An `ask()` caller waiting on a
   `oneshot::Receiver` keeps its receiver alive across the swap. The
   handler that produces the reply is the *new* code, but the
   channel binding is preserved.

## `ModuleSource`

```rust
pub enum ModuleSource<'a> {
    SameProgram,            // v0.20: state-only restart
    WasmBytes(&'a [u8]),    // v0.21: per-agent wasm-module loading
}
```

v0.20 only wires `SameProgram`. The `WasmBytes` variant is part of
the public API today so the CLI + future cluster live-migration
caller can record intent; the swap pipeline rejects it cleanly with
`MT5064` until v0.21 lands the per-agent module surface.

## Drain semantics

The pipeline busy-polls the gate's `is_busy` flag with a 1 ms sleep
between checks. The default deadline is **5 s** (matches the
roadmap), overridable per-call via `ReloadOptions::deadline`.

When the deadline trips:

- `ReloadError::DrainDeadline(d)` is returned (`MT5062`)
- The gate's `paused` flag is **not** set (the swap aborted before
  phase 3)
- The agent continues running its in-flight handler — the caller can
  choose to back off, increase the deadline, or in v0.21 forcibly
  cancel the handler with `--force`

The 1 ms poll interval is a deliberate tradeoff: tight enough that a
5 ms handler doesn't dominate the swap latency, slack enough that a
spin loop doesn't pin a CPU. v0.21 will replace the busy-poll with a
condvar wake-up emitted by the agent loop on `mark_idle()`.

## Mailbox preservation guarantee

The mailbox is preserved end-to-end across a successful swap. The
test suite (`crates/mty-runtime/tests/reload.rs::reload_preserves_mailbox`)
verifies this empirically: messages enqueued before, during, and
after the swap are all delivered in FIFO order.

The guarantee depends on **not** dropping the `Arc<Mailbox>`. The
swap pipeline operates on the descriptor in place; no
descriptor-level allocation is replaced. The new wasm code reads
from the same channel.

## v0.21 follow-up

The v0.20 slice deliberately stops short of three items so the
runtime-side surface can stabilise:

1. **Wasm module reload proper.** Plumb `ModuleSource::WasmBytes`
   into the runtime via a per-agent program slot. Likely requires
   `Program::with_swapped_agent(...)` on the IR.
2. **Schema-evolution ranges.** Today `schema_compatible_with` is
   bit-equality. v0.21 will widen this so the derived impl can opt
   into a *forward-compatible* range — fields added at the tail stay
   compatible with old snapshots, fields removed from the tail stay
   compatible with new snapshots, and explicit
   `migrate_from(old: V1) -> V2` hooks bridge anything that isn't
   tail-only.
3. **Multi-version support during rolling restart.** During a
   cluster-wide rolling restart, two versions of an agent type may
   coexist briefly. The router must be aware of the hash range each
   peer accepts; the cluster mesh already carries per-peer metadata,
   so this is a wire-format extension, not a transport rewrite.

## Diagnostics summary

| Code | Cause |
|------|-------|
| `MT5060` | Incompatible `SCHEMA_HASH` |
| `MT5061` | Agent not found |
| `MT5062` | Drain deadline exceeded |
| `MT5063` | Snapshot encode/decode failure |
| `MT5064` | Raw-wasm reload not yet wired (v0.20) |
| `MT5069` | Internal runtime error during reload |

## See also

- `docs/internals/agent-features-roadmap.md` Tier 1.5 — the original
  spec.
- `docs/reference/cli/mty-reload.md` — user-facing CLI doc.
- `dev/history/notes/HOT_RELOAD_V0_20_NOTES.md` — design log.
- `crates/mty-runtime/src/reload/` — implementation.
- `crates/mty-runtime/tests/reload.rs` — integration tests.
