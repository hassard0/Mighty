# 10 arena_escape

Positive-fire for **MT3010 ARENA_ESCAPE**. Spec v1.0-RC §10 (arenas).

A non-Copy `String` is bound inside `arena turn { ... }` and then
named as the arena's tail expression — that would let the value
outlive its arena. The borrow checker reports MT3010.
