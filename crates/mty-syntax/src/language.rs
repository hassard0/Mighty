use crate::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mighty {}

impl rowan::Language for Mighty {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        // SAFETY: We only ever round-trip SyntaxKind through u16 — see kind_to_raw.
        // The assert guards against rowan handing us a tag we never emitted.
        // PROC_MACRO_DECL is the last variant in SyntaxKind (see syntax_kind.rs).
        assert!(raw.0 <= (SyntaxKind::PROC_MACRO_DECL as u16));
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Mighty>;
pub type SyntaxToken = rowan::SyntaxToken<Mighty>;
pub type SyntaxElement = rowan::SyntaxElement<Mighty>;
pub type SyntaxNodeChildren = rowan::SyntaxNodeChildren<Mighty>;
pub type GreenNode = rowan::GreenNode;
