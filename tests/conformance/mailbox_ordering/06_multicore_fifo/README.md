# 06 multicore_fifo (v0.6)

Spec §25.3 + v0.6 scheduler: under the **multi-worker** runtime, an
agent's mailbox still preserves FIFO order for back-to-back asks from
the same caller. The cross-worker dispatch and work-stealing path
must not reorder messages destined for one agent.

The MtyIR conformance harness exercises the language semantics
(deterministic single-thread). The companion Rust integration test
`crates/mty-runtime/tests/multicore_fifo.rs` runs the same shape
through the real multi-worker scheduler with 4 workers.
