use crate::SyntaxKind;
use logos::Logos;

pub struct LexedToken<'src> {
    pub kind: SyntaxKind,
    pub text: &'src str,
    pub start: usize,
    pub end: usize,
}

pub fn lex(src: &str) -> Vec<LexedToken<'_>> {
    let mut lex = SyntaxKind::lexer(src);
    let mut out = Vec::with_capacity(src.len() / 4);
    while let Some(result) = lex.next() {
        let kind = match result {
            Ok(k) => k,
            Err(_) => SyntaxKind::ERROR,
        };
        let span = lex.span();
        out.push(LexedToken {
            kind,
            text: &src[span.start..span.end],
            start: span.start,
            end: span.end,
        });
    }
    out.push(LexedToken {
        kind: SyntaxKind::EOF,
        text: "",
        start: src.len(),
        end: src.len(),
    });
    out
}
