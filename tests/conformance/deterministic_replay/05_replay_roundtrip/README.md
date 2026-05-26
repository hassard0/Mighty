# deterministic_replay/05_replay_roundtrip

Closes the loop: record a trace, then replay it, and the second
run's trace MUST be byte-identical to the first.

This is the **end-to-end determinism contract**: an implementation
passes when its recorder produces a stable serialisation AND its
replay driver consumes that serialisation losslessly.

Spec §28.5 (record/replay roundtrip).
