# 01 panic_exits

Positive-fire for **MT5001 RUNTIME_PANIC**. Spec v1.0-RC §17 (errors), §25 (runtime).

`panic("boom")` traps the interpreter; the trap surfaces as MT5001
and the process exits non-zero (1).
