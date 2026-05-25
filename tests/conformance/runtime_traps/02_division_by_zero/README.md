# 02 division_by_zero

Positive-fire for **MT5003 DIVISION_BY_ZERO**. Spec v1.0-RC §17, §25 (runtime).

The interpreter traps when integer division has a zero RHS at runtime.
The static checker does not currently flag the literal zero (post-v0.1
work), so the trap fires during evaluation.
