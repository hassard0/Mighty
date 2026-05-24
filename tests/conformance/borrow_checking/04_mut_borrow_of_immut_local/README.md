# 04 mut_borrow_of_immut_local

Cannot `&mut` an immutable local. Spec §7.2 / §7.1 — SD3013.

Note: the slice-4 spec lists SD3013 as "borrow outlives owner" in
some prose, but the implementation pairs SD3013 with
mut-borrow-of-immut-local; the conformance suite tracks the actual
implementation behaviour (see `crates/sdust-borrow`).
