//! mty-syntax: lexer, CST, parser.
pub mod language;
pub mod lexer;
pub mod parser;
pub mod syntax_kind;
pub mod token_cache;
pub use language::{GreenNode, Mighty, SyntaxElement, SyntaxNode, SyntaxNodeChildren, SyntaxToken};
pub use lexer::{lex, LexedToken};
pub use parser::{parse, parse_with_opts, ParseError, ParseOpts, ParseResult};
pub use syntax_kind::SyntaxKind;
pub use token_cache::{CachedToken, TokenCache};
