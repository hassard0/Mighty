//! Drop-intent emission. Slice 4 records at scope exit which Owned, non-Copy
//! locals would need their `drop()` invoked. Codegen consumes this in a
//! later slice (no SIR/codegen in slice 4).

use sdust_hir::SourceSpan;

#[derive(Debug, Clone)]
pub struct DropEntry {
    pub local_name: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Default)]
pub struct DropPlan {
    pub entries: Vec<DropEntry>,
}
