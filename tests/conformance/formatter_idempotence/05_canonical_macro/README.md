# formatter_idempotence/05_canonical_macro

Pins the macro-body preservation contract: declarative macros expose
their body as a token sequence and the formatter MUST treat that
sequence as opaque-but-printed-verbatim. Re-flowing the macro body
would break hygiene assumptions in the expander.

The call site (`assert_eq!(...)`) MAY be re-indented; the
declaration body (between `=> {` and `}`) MUST NOT.

Spec §27.4.4 (macro preservation) + §20.3 (declarative macros).
