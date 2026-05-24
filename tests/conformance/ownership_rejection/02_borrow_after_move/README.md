# 02 borrow_after_move

After `a` is moved into `b`, `&a` is illegal because `a` is no longer
a valid owner. Spec §7.1 — MT3003.
