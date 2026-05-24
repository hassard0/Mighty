# borrow_checking

Spec §7.2 + slice-4 design. Aliasing-XOR-mutation rule. Each case
must be rejected with the corresponding SD3xxx code.

## Cases

- `01_mut_while_shared` — SD3004: cannot create `&mut` while a `&` exists.
- `02_shared_while_mut` — SD3005: cannot create `&` while a `&mut` exists.
- `03_two_mut_borrows` — SD3006: only one `&mut` may exist at once.
- `04_mut_borrow_of_immut_local` — SD3013: cannot `&mut` an immutable binding.
