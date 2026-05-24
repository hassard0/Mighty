# 05 nll_last_use

The shared borrow `&a` deactivates after its borrower `r` is last used
at `use_ref(r)`. The subsequent `&mut a` is therefore well-formed.
Under the slice-4 lexical region this case would have been rejected
with MT3004; v0.3 (A55) accepts it.
