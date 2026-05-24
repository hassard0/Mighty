use super::Parser;
use crate::SyntaxKind::{self, *};

pub fn expr(p: &mut Parser) -> bool {
    p.skip_trivia();
    // Minimal placeholder: consume a single literal as a LITERAL_EXPR so that
    // constructs like `[U8; 16]` (array type length) parse cleanly before
    // the full Pratt expression parser lands in a later task.
    match p.peek() {
        INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | CHAR_LITERAL
        | TRUE_KW | FALSE_KW | DURATION_LITERAL | SIZE_LITERAL => {
            p.start_node(LITERAL_EXPR);
            p.bump_any();
            p.finish_node();
            p.skip_trivia();
            true
        }
        _ => false,
    }
}

pub fn can_start_expr(k: SyntaxKind) -> bool {
    matches!(k,
        INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | CHAR_LITERAL
        | TRUE_KW | FALSE_KW | DURATION_LITERAL | SIZE_LITERAL
    )
}
