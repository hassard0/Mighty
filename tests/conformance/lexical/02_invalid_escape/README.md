# 02 invalid_escape

Spec v1.0-RC §3 (lexical). Reserves **MT0003 INVALID_ESCAPE**.

The v0.9 lexer is currently permissive with unknown escapes (passes
the `\q` through). This case is a **canary**: when the strict escape
check ships in v1.0-RC2 (see `CONFORMANCE_V0_10_NOTES.md`), the
expected exit code should flip to 1 with `MT0003` in
`expected_diagnostics.txt`. Tracked as a documented gap, NOT a
regression.
