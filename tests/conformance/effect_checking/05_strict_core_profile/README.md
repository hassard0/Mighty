# 05_strict_core_profile

v0.3 (A65) conformance: the strict `profile = "core"` rejects `alloc`.
The case-shape is preserved here; firing SD4002 requires a per-case
`star.toml` override that the conformance harness does not yet support.
See the dedicated sdust-types unit test `core_profile_rejects_alloc` for
the positive-fire path.

When per-case star.toml support lands, switch
`expected_exit_code.txt` to `1` and add `SD4002` to a new
`expected_diagnostics.txt`.
