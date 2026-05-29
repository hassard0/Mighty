# deterministic_replay/

v0.20-populated. Cases for the deterministic record/replay subsystem
(spec §28, `dev/history/notes/REPLAY_*` series).

The contract under test: every observable effect a program produces
during a recorded run can be re-played to bit-identical state under
the v0.18+ `ReplayDriver`. The relevant trace event kinds are:

* `ClockRead` — every call into `std.time` (or any host-clock binding)
* `RandomRead` — every call into `std.random` (or any host-random
  binding)
* `Spawn` — every agent spawn
* `MessageSent` / `MessageHandled` — every cross-agent message
* `EnvRead`, `FsRead`, `FsWrite`, `NetGet`, … — every other capability
  call

A *pure* program (one with no effect calls) MUST produce an EMPTY
trace.

Per-case shape:

```
NN_case_name/
  input.mty                       — the source program
  command.txt                     — `check` (parse + typecheck must pass)
  expected_diagnostics.txt        — usually empty for these cases
  expected_exit_code.txt          — 0 (a positive case)
  expected_trace.txt              — descriptive: the trace shape the
                                    case asserts (NN events; kinds; rough
                                    arguments). Read by humans + by the
                                    `ReplayDriver` integration tests in
                                    `crates/mty-runtime/tests/replay_*.rs`,
                                    NOT the conformance_full harness.
  README.md                       — what the case proves
```

The trace itself is captured under v0.18's replay format: a sequence
of `ReplayEvent` records emitted in execution order, each tagged with
the actor id (0 = synthetic external caller; per-agent ids otherwise).
A conforming implementation MUST be able to:

1. Run the program once with `MTY_REPLAY_RECORD=<path>` (or the
   legacy `STARDUST_REPLAY_RECORD`) and produce a trace file.
2. Run the program a second time with `MTY_REPLAY_PLAY=<path>` (or
   the legacy `STARDUST_REPLAY_PLAY`)
   and produce the SAME trace (modulo wall-clock timestamps, which
   the v0.20 ReplayDriver substitutes back from the recorded values).
3. Observe that any post-fact mutation of the trace file (changing a
   recorded clock value or random byte) is detected and reported as
   a divergence at the corresponding replay step.

Conformance for the category passes when all 5 cases' programs
type-check (`check` exits 0) AND the trace-shape descriptions in
`expected_trace.txt` match what the implementation's recorder
produces. The trace-shape comparison is implementation-defined for
v1.0; v1.1 will publish a normative trace serialisation format.

## Cases

| Case | Shape under test |
|------|------------------|
| `01_pure_program` | empty trace (no effect calls) |
| `02_clock_read`   | `[ClockRead]` |
| `03_random_seq`   | `[RandomRead{4}]` |
| `04_send_message` | `[Spawn, MessageSent, MessageHandled]` |
| `05_replay_roundtrip` | record → replay → equal traces |
