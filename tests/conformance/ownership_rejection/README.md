# ownership_rejection

Spec §7.1 + slice-4 design (borrow checker). Each sub-case is a
program that violates an ownership rule and must be rejected by
`mty check` with the corresponding SD3xxx code.

## Cases

- `01_use_after_move` — MT3001 use-after-move.
- `02_borrow_after_move` — MT3003 borrow-after-move.
- `03_cannot_move_borrowed` — MT3008 cannot-move-while-borrowed.
- `04_assign_to_immut_local` — MT3014 assign-to-immut-local.
