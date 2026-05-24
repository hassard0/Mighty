# 07 field_overlap

`&mut s.a` and `&mut s.a.x` borrow overlapping places (`s.a` is a prefix
of `s.a.x`). Spec §7.2 + v0.3 (A54). Two mutable borrows of overlapping
places must error with SD3006.

Note: v0.3 truncates projection chains at depth 1 (`s.a.x` is treated
as `s.a` for conflict purposes; see amendment A54). v0.4 will extend
to deeper paths.
