use super::Parser;
use crate::SyntaxKind;

pub fn expr(_p: &mut Parser) -> bool { false }
pub fn can_start_expr(_k: SyntaxKind) -> bool { false }
