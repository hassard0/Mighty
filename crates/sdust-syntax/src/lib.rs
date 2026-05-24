//! sdust-syntax: lexer, CST, parser.
pub mod syntax_kind;
pub mod lexer;
pub use syntax_kind::SyntaxKind;
pub use lexer::{lex, LexedToken};
