# 05 backpressure_block

v0.3 / A40 closure. Block policy sender backpressure shape. The
MtyIR-interp-driven harness sees synchronous returns; runtime-level
backpressure is asserted in
`crates/mty-runtime/tests/mailbox_slab_pool.rs::block_policy_backpressures_until_drained`.
