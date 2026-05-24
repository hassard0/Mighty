# ownership_rejection

Spec §7.1 + slice-4 design (borrow checker). Each sub-case is a
program that violates an ownership rule and must be rejected by
`sdust check` with the corresponding SD3xxx code.

## Cases

- `01_use_after_move` — SD3001 use-after-move.
- `02_borrow_after_move` — SD3003 borrow-after-move.
- `03_cannot_move_borrowed` — SD3008 cannot-move-while-borrowed.
- `04_assign_to_immut_local` — SD3014 assign-to-immut-local.
