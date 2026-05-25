# 01 unknown_macro

Positive-fire for **MT6001 UNKNOWN_MACRO**. Spec v1.0-RC §20 (macros).

The `nonexistent!(1)` site uses the bang-macro syntax (`name!(...)`)
but no `macro nonexistent` declaration is in scope, so the macro
preprocessor reports MT6001.
