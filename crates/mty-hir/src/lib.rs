//! mty-hir: name-resolved HIR.
pub mod dump;
pub mod effects;
pub mod ids;
pub mod lower;
pub mod nodes;
pub mod resolve;

pub use effects::{HirEffectName, HirEffectRow, HirRowVar};
pub use ids::*;
pub use nodes::*;
