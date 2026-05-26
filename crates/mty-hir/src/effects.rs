//! v0.16 RFC-008 effect-row HIR shape.
//!
//! This module defines the typed-name representation of a function's
//! effect annotation that the v0.16 wiring uses. It is **additive** to
//! the existing [`crate::nodes::HirFn::effects`] field — that field
//! stays the canonical closed-row view (a flat `Vec<String>` of
//! concrete effect names) so every downstream consumer
//! (`mty-types::effects::infer_and_validate`, the borrow checker, the
//! formatter, the LSP) keeps working unchanged.
//!
//! The new [`HirEffectRow`] is attached as `HirFn::effect_row` and is
//! `Some(...)` only when the v0.15 surface syntax produced one of the
//! row-variable shapes (`!E`, `!{a | E}`, `!{| E}`, `effect a | E`).
//! Pure closed-set fns leave `effect_row = None` so existing code
//! paths (which key off `effects`) are unaffected.
//!
//! ## Design — why row vars are kept as strings here, not allocated IDs
//!
//! `mty-types::effects::row::RowVar` is a `u32` densely allocated by a
//! [`mty_types::effects::row::RowSubst`]. Two `RowVar(0)`s from
//! different substitutions are unrelated. The HIR is built once per
//! package and consumed by many separate substitution contexts (one
//! per fn body, one per call site, plus the package-level effect
//! fixpoint). Allocating concrete `RowVar` IDs at HIR build time would
//! either pin the IDs across all substitutions (and break the v0.13
//! "scoped to one table" invariant) or duplicate the bookkeeping in
//! every consumer. Keeping the row variable as a textual name here and
//! deferring ID allocation to the typeck layer matches the v0.13/v0.14
//! row-machinery design.

/// A single concrete effect name appearing in a `!{...}` set or after
/// the legacy `effect` keyword. Wraps a plain [`String`] for parity
/// with the existing [`crate::nodes::HirFn::effects`] field
/// (`Vec<String>`); `smol_str` is not yet a workspace dependency.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirEffectName(pub String);

impl HirEffectName {
    pub fn new(s: impl Into<String>) -> Self {
        HirEffectName(s.into())
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&str> for HirEffectName {
    fn from(s: &str) -> Self {
        HirEffectName(s.to_string())
    }
}

impl From<String> for HirEffectName {
    fn from(s: String) -> Self {
        HirEffectName(s)
    }
}

/// A row-variable reference in the source. Carries the source name
/// (e.g. `E`, `R`) plus a stable per-fn index assigned at HIR-lowering
/// time. The index is used by [`mty_types::effects::row`] to allocate
/// fresh substitution-scoped row vars deterministically — each
/// distinct row-variable name within a single fn signature gets its
/// own `idx`, so a fn declared `fn observed[E, F](f: fn()->()!E, g:
/// fn()->()!F)` would have two `HirRowVar`s with `idx == 0` and `idx
/// == 1` respectively.
///
/// v0.16 SHIPPED-SUBSET: per-fn signatures use AT MOST ONE row
/// variable (matching the v0.13 stdlib HOF shape). Multi-row-var fns
/// parse cleanly but the typeck layer treats every distinct name as
/// the same fresh var. The `idx` field is reserved for the v0.17
/// multi-row-var extension.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirRowVar {
    pub name: String,
    pub idx: u32,
}

impl HirRowVar {
    pub fn new(name: impl Into<String>, idx: u32) -> Self {
        HirRowVar {
            name: name.into(),
            idx,
        }
    }
}

/// A function's effect row.
///
/// `Closed` is the v1.0 default: a finite, fully-listed effect set
/// (`effect fs, net` or `!{fs, net}`). `Open` carries a polymorphic
/// tail (`!{fs | E}`, `!E`, `effect fs | E`) that the typeck layer
/// will instantiate to a fresh [`mty_types::effects::row::RowVar`] on
/// each call.
///
/// The empty bare row-var case (`!E` with no concrete effects) is
/// represented as `Open(vec![], var)`. The empty closed case (`!{}`)
/// is `Closed(vec![])`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirEffectRow {
    /// `!{a, b}` or `effect a, b` — exactly these effects.
    Closed(Vec<HirEffectName>),
    /// `!{a, b | E}` or `!E` (with empty `Vec`) or
    /// `effect a, b | E` — at least these effects plus whatever the
    /// row variable resolves to at each call site.
    Open(Vec<HirEffectName>, HirRowVar),
}

impl HirEffectRow {
    /// The concrete-effect names (the visible component, ignoring any
    /// row-variable tail).
    pub fn concrete(&self) -> &[HirEffectName] {
        match self {
            HirEffectRow::Closed(xs) | HirEffectRow::Open(xs, _) => xs,
        }
    }

    /// True iff this row has a row-variable tail.
    pub fn is_open(&self) -> bool {
        matches!(self, HirEffectRow::Open(_, _))
    }

    /// The row variable, if any.
    pub fn row_var(&self) -> Option<&HirRowVar> {
        match self {
            HirEffectRow::Closed(_) => None,
            HirEffectRow::Open(_, v) => Some(v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_empty_round_trips() {
        let r = HirEffectRow::Closed(vec![]);
        assert!(!r.is_open());
        assert!(r.row_var().is_none());
        assert_eq!(r.concrete().len(), 0);
    }

    #[test]
    fn open_bare_row_var_concrete_is_empty() {
        let r = HirEffectRow::Open(vec![], HirRowVar::new("E", 0));
        assert!(r.is_open());
        assert_eq!(r.row_var().unwrap().name.as_str(), "E");
        assert_eq!(r.concrete().len(), 0);
    }

    #[test]
    fn open_with_concrete_carries_both() {
        let r = HirEffectRow::Open(
            vec![HirEffectName::from("fs"), HirEffectName::from("net")],
            HirRowVar::new("E", 0),
        );
        assert!(r.is_open());
        assert_eq!(r.concrete().len(), 2);
        assert_eq!(r.concrete()[0].as_str(), "fs");
        assert_eq!(r.concrete()[1].as_str(), "net");
        assert_eq!(r.row_var().unwrap().idx, 0);
    }
}
