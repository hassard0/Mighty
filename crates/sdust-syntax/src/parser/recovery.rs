use crate::SyntaxKind::{self, *};

pub const ITEM_START: &[SyntaxKind] = &[
    FN_KW, AGENT_KW, PROTOCOL_KW, STRUCT_KW, ENUM_KW, TYPE_KW,
    IMPL_KW, TRAIT_KW, USE_KW, MOD_KW, PACKAGE_KW, PUB_KW,
    CONST_KW, EXTERN_KW, EXPORT_KW, MACRO_KW, SUP_KW, UNSAFE_KW,
];

pub const STMT_START: &[SyntaxKind] = &[
    LET_KW, RETURN_KW, IF_KW, MATCH_KW, FOR_KW, WHILE_KW, LOOP_KW, UNSAFE_KW,
];

impl crate::parser::Parser<'_> {
    pub(crate) fn sync_to(&mut self, set: &[SyntaxKind]) {
        while !self.at(SyntaxKind::EOF) && !set.contains(&self.peek()) && !self.at(SyntaxKind::R_BRACE) {
            self.bump_any();
        }
        self.skip_trivia();
    }
}
