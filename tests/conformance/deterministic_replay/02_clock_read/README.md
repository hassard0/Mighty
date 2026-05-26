# deterministic_replay/02_clock_read

A call into the host clock (`clock.now()`) MUST appear as a
`ClockRead` event in the recorded trace, and the same call during
replay MUST consume that event and return the recorded value.

This pins the clock as a **replay observable**: implementations that
read the wall clock directly (bypassing the recorder) cannot pass
this case because the second run will report a different `ts`.

Spec §28.2 (clock determinism).
