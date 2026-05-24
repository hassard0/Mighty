//! Flat token representation used by the macro expander.
//!
//! mty-syntax uses rowan's CST which keeps trivia inline. For macro
//! expansion we want a flat `(kind, text)` sequence we can rewrite and
//! then concatenate back into source text for re-parsing. The expander
//! never inspects trivia semantically — it just preserves it so the
//! expanded source remains readable in dumps and error spans line up.

use mty_syntax::{SyntaxKind, SyntaxNode};

/// A flat token. `kind` is the lexer's [`SyntaxKind`] tag; `text` is the
/// exact source slice produced by the lexer. Trivia tokens (whitespace,
/// comments) preserve `kind == WHITESPACE` etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tok {
    pub kind: SyntaxKind,
    pub text: String,
}

impl Tok {
    pub fn new(kind: SyntaxKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }

    /// True for identifier-shaped tokens (`IDENT` only — keywords are
    /// excluded). Parameters and macro-introduced bindings are always
    /// `IDENT`s in well-formed Mighty source.
    pub fn is_ident(&self) -> bool {
        self.kind == SyntaxKind::IDENT
    }

    /// True for trivia tokens (whitespace + comments).
    pub fn is_trivia(&self) -> bool {
        self.kind.is_trivia()
    }
}

/// Extract every leaf token (including trivia) from a CST node, in
/// source order. Used by the registry to capture the macro body's
/// opaque token sequence.
pub fn tokens_from_body_node(body: &SyntaxNode) -> Vec<Tok> {
    let mut out = vec![];
    for elem in body.descendants_with_tokens() {
        if let Some(t) = elem.into_token() {
            out.push(Tok {
                kind: t.kind(),
                text: t.text().to_string(),
            });
        }
    }
    out
}

/// Concatenate tokens into source text. The expander emits a single
/// string that mty-syntax's [`parse_expr`](mty_syntax::parse_expr)
/// can consume.
pub fn tokens_to_source(toks: &[Tok]) -> String {
    let mut s = String::new();
    for t in toks {
        s.push_str(&t.text);
    }
    s
}

/// Lex a fragment of source back into tokens. Returns `None` if any
/// token errored. Used by the expander to turn argument source slices
/// into token sequences before splicing.
pub fn lex_fragment(src: &str) -> Option<Vec<Tok>> {
    let lexed = mty_syntax::lex(src);
    let mut out = Vec::with_capacity(lexed.len());
    for t in lexed {
        if t.kind == SyntaxKind::ERROR {
            return None;
        }
        if t.kind == SyntaxKind::EOF {
            continue;
        }
        out.push(Tok {
            kind: t.kind,
            text: t.text.to_string(),
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_fragment_basic() {
        let toks = lex_fragment("1 + 1").expect("lex ok");
        assert!(toks
            .iter()
            .any(|t| t.kind == SyntaxKind::INT_LITERAL && t.text == "1"));
        assert!(toks.iter().any(|t| t.kind == SyntaxKind::PLUS));
    }

    #[test]
    fn tokens_to_source_roundtrip() {
        let src = "if a != b { panic(\"x\") }";
        let toks = lex_fragment(src).unwrap();
        assert_eq!(tokens_to_source(&toks), src);
    }
}
