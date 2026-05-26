# wasm_component/02_wasi_p2_log

Pins the v0.17 log direct-lowering shape: a `log(...)` call MUST
appear in the component's import list as `wasi:cli/stdout@0.2.3` +
`wasi:io/streams@0.2.3` (NOT as a re-export of the preview1
adapter's `fd_write`).

Implementations that funnel `log` through the legacy preview1
adapter fail this case.

Spec §30.2 (WASI P2 log direct lowering).
