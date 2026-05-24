//! sdust-syntax: lexer, CST, parser.
pub mod syntax_kind;
pub mod lexer;
pub mod language;
pub mod parser;
pub use syntax_kind::SyntaxKind;
pub use lexer::{lex, LexedToken};
pub use language::{Stardust, SyntaxNode, SyntaxToken, SyntaxElement, SyntaxNodeChildren, GreenNode};
pub use parser::{parse, ParseResult, ParseError};
