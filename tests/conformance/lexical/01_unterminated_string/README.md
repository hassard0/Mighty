# 01 unterminated_string

Spec v1.0-RC §3 (lexical). Reserves **MT0002 UNTERMINATED_STRING**; the
v0.9 pipeline aggregates all lex/parse failures under **MT0001
UNEXPECTED_TOKEN**, so the harness asserts MT0001 here. The MT0002 →
distinct-code split is a documented v1.0-RC2 follow-up (see
`CONFORMANCE_V0_10_NOTES.md`).
