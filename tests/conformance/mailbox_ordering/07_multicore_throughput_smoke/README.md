# 07 multicore_throughput_smoke (v0.6)

Spec §25.4 + v0.6 scheduler: throughput smoke test for the multi-
worker runtime. The MtyIR case here just validates the loop+ask shape
deterministically. The companion Rust integration test
`crates/mty-runtime/tests/multicore_throughput_smoke.rs` runs **4
workers x 4 agents x 10k messages each**, asserting the run completes
in < 10s and every reply is the expected sum.

This is a smoke test, not a perf gate — perf gating lives in the
`mty-bench` crate (criterion benches).
