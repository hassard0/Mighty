# 09 move_via_ref_copy

`let x = *r` where `r: &I32`. `I32` is Copy, so reading through a
reference is just a load, not a move. Spec §7.3 + v0.3 (A56) accepts.
