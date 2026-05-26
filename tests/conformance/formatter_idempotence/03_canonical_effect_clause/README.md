# formatter_idempotence/03_canonical_effect_clause

Pins multi-row-var effect-clause canonical form. The v0.19 HIR fix
restored source-order preservation of every `EFFECT_ROW_VAR` child;
the formatter MUST emit them in the same order, with a single space
after the `|` and a `, ` between row vars.

Spec §27.4 + RFC-008 §"v0.16" + dev/history/notes/HIR_MULTI_ROW_V0_19_NOTES.md.
