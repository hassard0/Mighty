//! mty-syntax: lexer, CST, parser.
pub mod language;
pub mod lexer;
pub mod parser;
pub mod syntax_kind;
pub use language::{GreenNode, Mighty, SyntaxElement, SyntaxNode, SyntaxNodeChildren, SyntaxToken};
pub use lexer::{lex, LexedToken};
pub use parser::{parse, ParseError, ParseResult};
pub use syntax_kind::SyntaxKind;
