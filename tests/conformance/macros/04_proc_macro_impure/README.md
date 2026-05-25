# 04 proc_macro_impure

Positive-fire for **MT6005 PROC_MACRO_IMPURE**. Spec v1.0-RC §20 (macros).

The proc-macro body calls `effect.io(...)` (a runtime effect). Static
purity analysis catches this and reports MT6005.
