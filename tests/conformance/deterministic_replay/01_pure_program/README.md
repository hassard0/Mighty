# deterministic_replay/01_pure_program

A program with no effect calls (no `log`, no `std.time.now`, no
`std.random.*`, no `send`/`ask`, no `fs.*`/`net.*`) MUST produce an
empty replay trace.

This pins the **trace-emission discipline** at the bottom: an
implementation that records *anything* for a pure program is leaking
state into the trace and would not survive byte-identical replay.

Spec §28.1 (recorder invariant).
