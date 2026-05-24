use sdust_syntax::{lex, SyntaxKind, SyntaxKind::*};

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
    let expected: Vec<SyntaxKind> = std::iter::repeat(DURATION_LITERAL)
        .take(6)
        .chain(std::iter::repeat(SIZE_LITERAL).take(4))
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
fn line_comment_is_trivia() {
    let toks = lex("// hello\nfn");
    assert_eq!(toks[0].kind, LINE_COMMENT);
    assert_eq!(toks[1].kind, WHITESPACE);
    assert_eq!(toks[2].kind, FN_KW);
}
