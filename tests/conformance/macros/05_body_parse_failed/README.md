# macros/05_body_parse_failed

MT6003 positive-fire (Gap F — v0.10 audit). The macro-call argument
`$` has no lexer token rule, so `mty_macros::token::lex_fragment`
returns None and `expand()` returns
`ExpandError::BadArgumentTokens { index: 0 }`. The HIR macros lowerer
translates that into `MT6003 MACRO_BODY_PARSE_FAILED`.

Closing this fixture moves Gap F (proc-macro / macro body codes) from
4-of-5 missing to 3-of-5 missing in the conformance_full corpus.

Spec ref: §20 compile-time metaprogramming.
