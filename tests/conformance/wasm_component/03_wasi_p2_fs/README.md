# wasm_component/03_wasi_p2_fs

Pins the v0.16 fs direct-lowering shape: an `fs.read(path)` call
MUST appear in the component's import list as
`wasi:filesystem/types@0.2.3` + `wasi:filesystem/preopens@0.2.3`,
NOT as a preview1 `fd_read` re-export.

Spec §30.3 (WASI P2 fs direct lowering).
