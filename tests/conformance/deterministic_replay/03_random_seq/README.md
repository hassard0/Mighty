# deterministic_replay/03_random_seq

A call into the host RNG MUST appear as a `RandomRead` event in the
recorded trace, and the same call during replay MUST consume that
event and return the recorded bytes.

This pins the RNG as a **replay observable**: implementations that
read `/dev/urandom` directly (bypassing the recorder) cannot pass
this case because the second run will see different bytes.

Spec §28.3 (RNG determinism).
