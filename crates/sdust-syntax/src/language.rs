use crate::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stardust {}

impl rowan::Language for Stardust {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        // SAFETY: We only ever round-trip SyntaxKind through u16 — see kind_to_raw.
        // The assert guards against rowan handing us a tag we never emitted.
        // CONST_DECL is the last variant in SyntaxKind (see syntax_kind.rs).
        assert!(raw.0 <= (SyntaxKind::CONST_DECL as u16));
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Stardust>;
pub type SyntaxToken = rowan::SyntaxToken<Stardust>;
pub type SyntaxElement = rowan::SyntaxElement<Stardust>;
pub type SyntaxNodeChildren = rowan::SyntaxNodeChildren<Stardust>;
pub type GreenNode = rowan::GreenNode;
