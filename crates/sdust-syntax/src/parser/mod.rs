use crate::{lexer::LexedToken, SyntaxKind};
use rowan::GreenNodeBuilder;

pub mod recovery;
pub mod paths;
pub mod items;
pub mod types;
pub mod patterns;
pub mod exprs;
pub mod stmts;
pub mod agents;
pub mod concurrency;
pub mod extern_;
pub mod macros;
pub mod unsafe_;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

pub struct ParseResult {
    pub green: rowan::GreenNode,
    pub errors: Vec<ParseError>,
}

pub struct Parser<'src> {
    pub(crate) tokens: Vec<LexedToken<'src>>,
    pub(crate) pos: usize,
    pub(crate) builder: GreenNodeBuilder<'static>,
    pub(crate) errors: Vec<ParseError>,
}

impl<'src> Parser<'src> {
    pub fn new(src: &'src str) -> Self {
        Self {
            tokens: crate::lex(src),
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
        }
    }

    pub fn parse_file(mut self) -> ParseResult {
        self.builder.start_node(SyntaxKind::FILE.into());
        self.skip_trivia();
        while !self.at(SyntaxKind::EOF) {
            if !items::item(&mut self) {
                let t_start = self.tokens[self.pos].start;
                let t_end = self.tokens[self.pos].end;
                let t_text = self.tokens[self.pos].text.to_string();
                self.error_at(format!("unexpected token `{}`", t_text), t_start, t_end);
                self.bump_any();
                self.skip_trivia();
            }
        }
        self.builder.finish_node();
        ParseResult { green: self.builder.finish(), errors: self.errors }
    }

    // ---- cursor primitives ----

    pub(crate) fn peek(&self) -> SyntaxKind {
        self.tokens[self.pos].kind
    }
    pub(crate) fn peek_n(&self, n: usize) -> SyntaxKind {
        self.tokens.get(self.pos + n).map(|t| t.kind).unwrap_or(SyntaxKind::EOF)
    }
    pub(crate) fn at(&self, kind: SyntaxKind) -> bool { self.peek() == kind }
    pub(crate) fn at_set(&self, set: &[SyntaxKind]) -> bool { set.contains(&self.peek()) }

    pub(crate) fn bump_any(&mut self) {
        let t = &self.tokens[self.pos];
        if t.kind != SyntaxKind::EOF {
            self.builder.token(t.kind.into(), t.text);
            self.pos += 1;
        }
    }
    pub(crate) fn bump(&mut self, kind: SyntaxKind) {
        assert_eq!(self.peek(), kind);
        self.bump_any();
    }
    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) { self.bump_any(); self.skip_trivia(); true } else { false }
    }
    pub(crate) fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) { true } else {
            let t_text = self.tokens[self.pos].text.to_string();
            let s = self.tokens[self.pos].start;
            let e = self.tokens[self.pos].end;
            self.error_at(format!("expected {:?}, got `{}`", kind, t_text), s, e);
            false
        }
    }

    pub(crate) fn skip_trivia(&mut self) {
        while self.peek().is_trivia() { self.bump_any(); }
    }

    pub(crate) fn start_node(&mut self, kind: SyntaxKind) { self.builder.start_node(kind.into()); }
    pub(crate) fn finish_node(&mut self) { self.builder.finish_node(); }

    pub(crate) fn checkpoint(&self) -> rowan::Checkpoint { self.builder.checkpoint() }
    pub(crate) fn start_node_at(&mut self, cp: rowan::Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(cp, kind.into());
    }

    pub(crate) fn error_at(&mut self, message: String, start: usize, end: usize) {
        self.errors.push(ParseError { message, start, end });
    }

    pub(crate) fn error(&mut self, message: impl Into<String>) {
        let s = self.tokens[self.pos].start;
        let e = self.tokens[self.pos].end;
        self.error_at(message.into(), s, e);
    }
}

pub fn parse(src: &str) -> ParseResult {
    Parser::new(src).parse_file()
}

pub fn parse_type(src: &str) -> ParseResult {
    let mut p = Parser::new(src);
    p.builder.start_node(SyntaxKind::FILE.into());
    p.skip_trivia();
    types::type_expr(&mut p);
    p.builder.finish_node();
    ParseResult { green: p.builder.finish(), errors: p.errors }
}
