use mty_syntax::{lex, SyntaxKind, SyntaxKind::*};

fn kinds(src: &str) -> Vec<SyntaxKind> {
    lex(src)
        .into_iter()
        .map(|t| t.kind)
        .filter(|k| !k.is_trivia())
        .collect()
}

#[test]
fn keywords() {
    assert_eq!(
        kinds("fn agent protocol on"),
        vec![FN_KW, AGENT_KW, PROTOCOL_KW, ON_KW, EOF]
    );
}

#[test]
fn literals() {
    assert_eq!(
        kinds(r#"42 3.14 "hi" 'c' true false"#),
        vec![
            INT_LITERAL,
            FLOAT_LITERAL,
            STRING_LITERAL,
            CHAR_LITERAL,
            TRUE_KW,
            FALSE_KW,
            EOF
        ]
    );
}

#[test]
fn duration_and_size() {
    let expected: Vec<SyntaxKind> = std::iter::repeat_n(DURATION_LITERAL, 6)
        .chain(std::iter::repeat_n(SIZE_LITERAL, 4))
        .chain(std::iter::once(EOF))
        .collect();
    assert_eq!(
        kinds("10ns 5us 3ms 2s 1m 1h 64B 4KiB 128MiB 1GiB"),
        expected
    );
}

#[test]
fn typed_int() {
    assert_eq!(
        kinds("42u32 3i64 100u8"),
        vec![INT_LITERAL, INT_LITERAL, INT_LITERAL, EOF]
    );
}

#[test]
fn punctuation() {
    assert_eq!(
        kinds("!= == -> => :: .. ..="),
        vec![
            BANG_EQ,
            EQ_EQ,
            THIN_ARROW,
            FAT_ARROW,
            COLON_COLON,
            DOT_DOT,
            DOT_DOT_EQ,
            EOF
        ]
    );
}

#[test]
fn agent_send_ask() {
    assert_eq!(kinds("logger!Info"), vec![IDENT, BANG, IDENT, EOF]);
    assert_eq!(kinds("fetcher?Page"), vec![IDENT, QUESTION, IDENT, EOF]);
    assert_eq!(kinds("@2s"), vec![AT, DURATION_LITERAL, EOF]);
}

#[test]
fn html_literal() {
    assert_eq!(kinds(r#"html"<h1>Hi</h1>""#), vec![HTML_LITERAL, EOF]);
}

#[test]
fn lex_decimal_size_suffix_k() {
    // `1k` = 1000 (decimal). Distinct from `1KiB` (1024).
    let toks = lex("1k");
    assert_eq!(toks[0].kind, SIZE_LITERAL);
    assert_eq!(toks[0].text, "1k");
}

#[test]
fn lex_decimal_size_suffix_uppercase_m() {
    // Uppercase `M` is chosen for million to avoid collision with the
    // `m` (=minutes) DURATION_LITERAL suffix. See amendments A1.
    let toks = lex("4096M");
    assert_eq!(toks[0].kind, SIZE_LITERAL);
    assert_eq!(toks[0].text, "4096M");
}

#[test]
fn lex_binary_size_suffix_still_works() {
    let toks = lex("128MiB");
    assert_eq!(toks[0].kind, SIZE_LITERAL);
    assert_eq!(toks[0].text, "128MiB");
}

#[test]
fn line_comment_is_trivia() {
    let toks = lex("// hello\nfn");
    assert_eq!(toks[0].kind, LINE_COMMENT);
    assert_eq!(toks[1].kind, WHITESPACE);
    assert_eq!(toks[2].kind, FN_KW);
}

// ---- v0.36 T1: radix-prefixed integer literals -----------------------

#[test]
fn hex_bare() {
    let toks = lex("0xFF 0xdeadbeef 0xABCDEF");
    assert_eq!(toks[0].kind, HEX_INT_LITERAL);
    assert_eq!(toks[0].text, "0xFF");
    assert_eq!(toks[2].kind, HEX_INT_LITERAL);
    assert_eq!(toks[2].text, "0xdeadbeef");
    assert_eq!(toks[4].kind, HEX_INT_LITERAL);
}

#[test]
fn hex_with_underscores() {
    let toks = lex("0xDEAD_BEEF 0xFF_FF_FF_FF");
    assert_eq!(toks[0].kind, HEX_INT_LITERAL);
    assert_eq!(toks[0].text, "0xDEAD_BEEF");
    assert_eq!(toks[2].kind, HEX_INT_LITERAL);
    assert_eq!(toks[2].text, "0xFF_FF_FF_FF");
}

#[test]
fn hex_with_typed_suffix_u8() {
    let toks = lex("0xFF_u8");
    assert_eq!(toks[0].kind, HEX_INT_LITERAL);
    assert_eq!(toks[0].text, "0xFF_u8");
}

#[test]
fn hex_with_typed_suffix_no_underscore() {
    let toks = lex("0xFFu8 0xFFu16 0xFFFFu32 0xFFu64");
    assert_eq!(toks[0].kind, HEX_INT_LITERAL);
    assert_eq!(toks[0].text, "0xFFu8");
    assert_eq!(toks[2].kind, HEX_INT_LITERAL);
    assert_eq!(toks[2].text, "0xFFu16");
}

#[test]
fn hex_with_all_int_suffixes() {
    // Every signed + unsigned width + sized variant lexes as HEX_INT_LITERAL.
    for (src, expected) in [
        ("0xFF_u8", "0xFF_u8"),
        ("0xFF_u16", "0xFF_u16"),
        ("0xDEADBEEF_u32", "0xDEADBEEF_u32"),
        ("0xFF_u64", "0xFF_u64"),
        ("0xFF_u128", "0xFF_u128"),
        ("0xFF_i8", "0xFF_i8"),
        ("0xFF_i16", "0xFF_i16"),
        ("0xFF_i32", "0xFF_i32"),
        ("0xFF_i64", "0xFF_i64"),
        ("0xFF_i128", "0xFF_i128"),
        ("0xFF_usize", "0xFF_usize"),
        ("0xFF_isize", "0xFF_isize"),
    ] {
        let toks = lex(src);
        assert_eq!(
            toks[0].kind, HEX_INT_LITERAL,
            "expected HEX_INT_LITERAL for {src}, got {:?}",
            toks[0].kind
        );
        assert_eq!(toks[0].text, expected, "text mismatch for {src}");
    }
}

#[test]
fn binary_and_octal_with_suffix() {
    let toks = lex("0b1010_u8 0o777_u32");
    assert_eq!(toks[0].kind, BIN_INT_LITERAL);
    assert_eq!(toks[0].text, "0b1010_u8");
    assert_eq!(toks[2].kind, OCT_INT_LITERAL);
    assert_eq!(toks[2].text, "0o777_u32");
}

#[test]
fn radix_literals_in_expr_position() {
    // Each radix literal should parse as the head of its own LITERAL_EXPR.
    let toks = lex("let x = 0xFF_u8;");
    let kinds: Vec<_> = toks
        .iter()
        .map(|t| t.kind)
        .filter(|k| !k.is_trivia())
        .collect();
    assert_eq!(kinds, vec![LET_KW, IDENT, EQ, HEX_INT_LITERAL, SEMI, EOF]);
}
