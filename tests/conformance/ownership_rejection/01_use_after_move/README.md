# 01 use_after_move

`a` is moved into `b` via `move a`, then read on the next line. The
borrow checker must reject with MT3001. Spec §7.1.
