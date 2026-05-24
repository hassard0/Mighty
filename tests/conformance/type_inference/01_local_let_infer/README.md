# 01 local_let_infer

Local `let` bindings without type annotations. Spec §6.4 (inference).

`let a = 1` should infer `a: I64` from the integer literal, then
`a + b` should unify both operands.
