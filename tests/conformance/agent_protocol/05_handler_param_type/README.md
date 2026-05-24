# 05_handler_param_type

v0.3 (A65) conformance: SD4031 protocol handler parameter-type mismatch.
The agent implements the local `Counter` protocol whose `Add(n: I32)`
message types `n` as `I32`. The handler body uses `n` at a Str-shaped
type via `n.starts_with("0")`, so the inferred-vs-declared unification
fails and SD4031 fires. External (cross-package) protocols continue to
fall back to SD2026.
