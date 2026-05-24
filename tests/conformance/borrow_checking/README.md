# borrow_checking

Spec §7.2 + §7.3. Aliasing-XOR-mutation rule plus the v0.3 NLL,
field-level, and SD3009 hardening (amendments A54/A55/A56).

## Cases

### Slice 4 baseline (whole-local borrows, lexical regions)

- `01_mut_while_shared` — SD3004: cannot create `&mut` while a `&` exists.
- `02_shared_while_mut` — SD3005: cannot create `&` while a `&mut` exists.
- `03_two_mut_borrows` — SD3006: only one `&mut` may exist at once.
- `04_mut_borrow_of_immut_local` — SD3013: cannot `&mut` an immutable binding.

### v0.3 hardening

- `05_nll_last_use` — accepted: shared borrow ends at borrower's last
  use; subsequent `&mut` is OK (A55).
- `06_field_disjoint` — accepted: `&mut s.a` + `&s.b` borrow disjoint
  places (A54).
- `07_field_overlap` — SD3006: `&mut s.a` + `&mut s.a.x` overlap on
  `s.a` (A54).
- `08_move_via_ref` — SD3009: `let x = *r` where `r: &String` is
  unsound (A56).
- `09_move_via_ref_copy` — accepted: `let x = *r` where `r: &I32` is
  a Copy load (A56).
