# 04 slab_reuse_fifo

v0.3 / A40 closure. Eight `?Say` asks against an `Echoer` agent
exercise slab-slot reuse (default v0.3 slab pool is 1024 slots, so
this test only stresses reuse when the pool is reduced in a
runtime-configured run; see
`crates/mty-runtime/tests/mailbox_slab_pool.rs::slab_reuse_preserves_fifo_for_many_messages`
for the bound-down version).

The conformance harness drives this through the MtyIR interp, which
ignores mailbox shape entirely (handlers run synchronously back-to-back).
The shape here ensures the IR + spec mapping is in the suite; the
real concurrency assertion lives at the runtime layer.
