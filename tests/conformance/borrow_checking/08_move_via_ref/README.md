# 08 move_via_ref

`let x = *r` where `r: &String` tries to move out of a reference for a
non-Copy type. Spec §7.3 + v0.3 (A56). References don't own; this is
fundamentally unsound and must error with MT3009.
