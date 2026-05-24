# 05 wall_cancels_mid_turn

v0.3 / A41 closure. The slice-7 case 03 documented that wall
deadlines only fire between turns; v0.3 adds cooperative mid-turn
cancellation via `tokio::task::spawn_blocking` + a per-turn
[`CancellationToken`].

The case still flows through the SIR interp in the conformance
harness (the harness is sdust-driver based and doesn't spin up
`sdust-runtime`), so the deadline-fire path here is tested at the
runtime level in `crates/sdust-runtime/tests/cancellation_mid_turn.rs`.
The shape here documents the spec → implementation mapping;
INTENTIONALLY_IGNORED in the harness with reason
`runtime-driven cancellation tested at runtime-tests level`.
