use crate::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mighty {}

impl rowan::Language for Mighty {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        // SAFETY: We only ever round-trip SyntaxKind through u16 — see kind_to_raw.
        // The assert guards against rowan handing us a tag we never emitted.
        // TOOL_ATTR_CAP_ARG is the last variant in SyntaxKind (see syntax_kind.rs).
        // v0.27 Track A appended TOOL_ATTR / TOOL_ATTR_ARGS / TOOL_ATTR_CAP_ARG
        // after PROC_MACRO_DECL; the upper bound shifts to match.
        assert!(raw.0 <= (SyntaxKind::TOOL_ATTR_CAP_ARG as u16));
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
