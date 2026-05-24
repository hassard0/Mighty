# 06 field_disjoint

`&mut s.a` and `&s.b` borrow disjoint fields of the same struct.
Spec §7.2 + v0.3 (A54): field-level Place tracking accepts this case.
Slice 4 rejected it because borrows were tracked at the whole-local
granularity.
