# 02 cap_narrow_fs_ro

Positive case: narrowing `fs` via `fs.ro("/data")` returns a Cap
with `And(ReadOnly, Path("/data"))`. The narrowed binding can be
used to call `read` without diagnostics. Spec §8.1 (narrowing
constructors).
