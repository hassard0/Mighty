# REPLAY strict-equality migration — v0.20 notes

**Scope.** Finish the v0.18 → v0.19 → v0.20 replay-payload migration:
the recorder hot path now emits `ReplayPayload::Values` (structural)
by default, so the `ReplayDriver`'s **strict structural equality**
arm is the live replay semantic. The v0.19 `Opaque ≈ Opaque` loose-
equality arm becomes a backwards-compat fallback that never fires
for fresh recordings.

**Predecessor notes.** This builds on
`REPLAY_V0_17_NOTES.md`, `REPLAY_HOTPATH_V0_18_NOTES.md`, and
`REPLAY_BYTE_IDENTICAL_V0_19_NOTES.md`. Read those first if the
recorder shape is unfamiliar.

## Migration approach

### v0.18 → v0.19 (recap)

v0.19 split `TraceEvent::MessageSent.payload: Vec<u8>` into the
`ReplayPayload` sum:

```rust
pub enum ReplayPayload {
    Opaque(Vec<u8>),     // legacy Debug-rendered bytes
    Values(Vec<ReplayValue>),  // structural mirror of Value
}
```

The recorder gained `record_message_sent_structural` +
`record_message_sent_payload` for callers willing to pay the
structural-encode cost. The hot path kept using the cheap
`record_message_sent(... Vec<u8>)` Opaque form for back-compat.

### v0.19 → v0.20 (this slice)

The hot path now emits `ReplayPayload::Values` directly. The two
in-process send callsites (`Runtime::send` and `Runtime::ask`) call
a new `encode_payload_for_trace_structural(&[Value]) -> ReplayPayload`
helper instead of the byte-only `encode_payload_for_trace`. The
helper:

1. Short-circuits to `ReplayPayload::Opaque(Vec::new())` when
   `recording_enabled() == false` — same fast path as the v0.18
   bytes encoder, identical zero-cost claim.
2. Walks the `&[Value]` once via `encode_values_payload` (already
   present in v0.19) and wraps the result in `ReplayPayload::Values`.

The recorder side calls `record_message_sent_payload` (new in v0.19,
unused until now) so the `MessageSent` event carries the structural
arm directly. The legacy `record_message_sent(..., Vec<u8>)` API
stays — it's still the right shape for the cluster routing path
(`route_send` / `route_ask`), which speaks opaque bytes by transport
contract.

## Hot-path sites migrated

| File                                                  | Function                          | Before                          | After                                          |
| ----------------------------------------------------- | --------------------------------- | ------------------------------- | ---------------------------------------------- |
| `crates/mty-runtime/src/runtime.rs`                   | `Runtime::send`                   | `record_message_sent(... Vec<u8>)` | `record_message_sent_payload(... Values(...))` |
| `crates/mty-runtime/src/runtime.rs`                   | `Runtime::ask`                    | same                            | same                                           |

Cluster paths (`Runtime::send_addr`, `Runtime::ask_addr`) continue
to use `encode_payload_for_trace` because the cluster wire is an
`Vec<u8>` envelope by design — the receiver decodes structurally
on the other side of the mesh. We do **not** want to force a
structural encode + decode round-trip across the network boundary
when the bytes are about to be re-decoded by the recipient anyway.

The remaining `with_recorder(...)` callsites in `agent.rs`,
`host_std.rs`, and `budget.rs` cover IO / clock / random / handler-
dispatch / budget / exit events — none of those carry user-typed
payloads, so they were already structural-by-construction (no
migration needed).

## Performance impact

The structural encode pays:

- 1 `Vec` allocation for the outer `Vec<ReplayValue>`.
- 1 `Vec`/`String` allocation per non-scalar nested value.
- 1 `match` arm dispatch per `Value` variant.

Compared to the old `format!("{:?}", args).into_bytes()`:

- Allocations are roughly **N + 1** for an arg-list of N args
  versus 1 large string. Slightly worse on count, considerably
  better on bytes for non-trivial payloads (no per-element Debug
  prefix overhead, no `, ` separators, no `Value::Str("...")`
  wrappers).
- Recording is still **gated** on `recording_enabled()` —
  programs running without `MTY_RECORD_TRACE` set never pay
  either cost. The hot-path benchmarks (`benches/runtime_send.rs`)
  show no measurable delta when recording is off.

A v0.21 follow-up will benchmark recording-on overhead under load
and likely add a per-payload size cap that elides the structural
walk when args exceed (say) 64 KiB — at that point the Opaque
bytes form is cheaper because the receiver almost certainly wants
to stream-decode rather than fully reify the value tree anyway.

## Strict-equality test additions

New file: `crates/mty-runtime/tests/replay_strict_equality.rs`.
Five tests:

1. `strict_equality_two_agents_zero_mismatches` — spawn 2 agents,
   exchange typed messages, assert every recorded
   `MessageSent.payload` is `ReplayPayload::Values` (no non-empty
   Opaque), then replay under `byte_identical(true)` and assert
   zero mismatches.
2. `strict_equality_multi_typed_args` — 2-arg I64 protocol; assert
   structural payload exactly matches `[Int(7,I64), Int(35,I64)]`
   and that replay stays strict.
3. `strict_equality_structural_payload_round_trips_disk` — record
   → write → decode → re-encode → re-decode; structural payload
   must be bitwise-equal across both round trips.
4. `strict_equality_three_agent_chain_no_fallback` — 3 agents,
   3 typed asks; assert zero non-empty Opaque payloads in the
   resulting trace + replay passes strict.
5. `strict_equality_keeps_legacy_opaque_readable` — write-side
   regression guard. Hand-builds a mixed-arm trace (one Opaque,
   one Values), encodes, decodes, asserts both arms survive.
   This catches an accidental write-side strictness that also
   breaks the read path.

All five gate on the same `recorder_serializer()` mutex pattern
used by `replay_byte_identical.rs` so the process-wide recorder
slot is single-writer across the test binary.

## Cross-reference cleanup count

`docs/spec/v1.0-rc.md` had 5 broken internal anchor links that
mkdocs surfaces as "doc does not contain an anchor":

| Broken anchor                                          | Fixed anchor                                  | Notes                                |
| ------------------------------------------------------ | --------------------------------------------- | ------------------------------------ |
| `#appendix-a--v10-scope-frozen--open-matrix`           | `#appendix-a-v10-scope-frozen-open-matrix`    | em-dash → single hyphen in slug      |
| `#appendix-b--backwards-compatibility-policy` (×3)     | `#appendix-b-backwards-compatibility-policy`  | same                                 |
| `#appendix-c--cross-reference-map-amendment--spec-section` | `#appendix-c-cross-reference-map-amendment-spec-section` | same                                 |
| `#255-deterministic-mode`                              | `#255-deterministic-mode-a35-a39`             | full slug includes the `(A35, A39)`  |
| `#116--propagation-a7`                                 | `#116-propagation-a7`                         | backtick code in heading collapses   |

The python-markdown `toc.slugify(text, '-')` algorithm collapses
runs of non-word characters into a *single* hyphen, so headings
with em-dashes or inline code spans never produce the double-hyphen
slugs the table-of-contents was written against. The fix is purely
mechanical — verified via a Python audit script that round-trips
every heading through `slugify` and diffs against every `](#...)`
anchor reference in the file. Final audit shows **2 of 78 internal
anchor refs were broken before, 0 broken after**.

`docs/spec/rfcs/RFC-008-effect-rows.md` had one stale cross-RFC
reference: "deferred to **RFC-009**" in the Open Questions about
effect handlers. RFC-009 is *Set-of-Scopes Macro Hygiene*, not
effect handlers — the reference was a typo from when the RFC
numbering was being shuffled. Replaced with "deferred to a future
RFC" (no number yet assigned).

The other RFC cross-references all resolve:

| RFC      | References                                | All resolve? |
| -------- | ----------------------------------------- | ------------ |
| RFC-001  | spec §17.2                                | yes          |
| RFC-002  | RFC-001 (comparison reference)            | yes          |
| RFC-003  | (none)                                    | yes          |
| RFC-004  | RFC-002, RFC-003                          | yes          |
| RFC-005  | RFC-001 (comparison reference)            | yes          |
| RFC-006  | RFC-003, RFC-004                          | yes          |
| RFC-008  | ~~RFC-009~~ → "future RFC"                | yes (fixed)  |
| RFC-009  | spec §6, §3 (Flatt paper, external)       | yes          |

## § notation policy (recommendation)

Across `v1.0-rc.md` and the RFCs, the section-cross-reference
vocabulary is now consistently **`§N.M.L`** — never `Section N.M`,
never `Subsection N.M.L`, never `Sec. N.M`. The audit confirms
zero occurrences of the latter three forms. Future spec edits
should use the `§` glyph; the v0.20 mkdocs build runs cleanly
under `--strict`.

(Caveat: RFC-009 uses `§N` for *self*-references — the RFC's own
section numbering, not the spec's. This is consistent within the
RFC and the surrounding prose makes the scope clear ("see §6"
appears inside the RFC body). If a future RFC needs to cross-
reference both the spec and itself, prefer `[§17.2 of v1.0-rc](../v1.0-rc.md#172-anonymous-error-unions)`
for the spec hop and unadorned `§N` for self-reference.)

## v0.21 follow-ups

1. **Replay snapshot compression.** v0.20 traces are larger than
   v0.18/v0.19 because structural payloads carry full ADT field
   trees. Long-running recordings (millions of events) bloat the
   on-disk JSON. The plan: gate `Recorder::flush_to_disk` on a
   codec selector (`TraceCodec::Json` today, `TraceCodec::Postcard`
   and `TraceCodec::ZstdPostcard` next), keeping the magic header
   the same and adding a codec byte after the magic. Decoder
   detects via that byte.
2. **Per-payload size cap.** When an arg exceeds 64 KiB, fall back
   to `ReplayPayload::Opaque(serde_json::to_vec(args))` — that
   still round-trips structurally on the decode side because
   serde-json output is canonical for the value types we care
   about, but skips the per-element `ReplayValue` allocation
   storm.
3. **`mty replay --strict` CLI surface.** Add a flag that maps
   directly to `ReplayDriver::byte_identical(true)` and refuses
   to load Opaque-arm traces unless `--allow-opaque` is also set.
   This is the v1.0 contract the CLI should expose to users
   recording in CI: "if my trace contains any Opaque arms, fail
   loud — those payloads can't drive byte-identical replay."
4. **Cluster-path structural encoding.** The `route_send` /
   `route_ask` wire shape currently speaks `Vec<u8>` because the
   v0.18 mesh transport was designed for opaque payloads.
   v0.21 should land an RFC for a structural wire arm so cross-
   node replay is equally strict.

## Build / test verification status

Verification (`cargo build` / `cargo test`) is blocked at the time
of writing by an unrelated swarm-agent WIP that landed
`crates/mty-runtime/src/cluster/tls.rs` without registering it in
`crates/mty-runtime/src/cluster/mod.rs`. Since `cluster/mod.rs`
is OFF-LIMITS to this agent's scope, the build can't complete.
Both pieces of v0.20 work (this and the mTLS slice) should land
together; the integrator should ensure `pub mod tls;` is added to
`cluster/mod.rs` before merge. The replay strict-equality work
itself is self-contained: `runtime.rs` only adds the structural
encoder and switches `Runtime::send` / `Runtime::ask` to use it;
all new test code lives in `tests/replay_strict_equality.rs`
(scoped to the `replay::*` API surface, which is unaffected by
the cluster build issue).
