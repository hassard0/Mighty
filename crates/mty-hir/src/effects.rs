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
/// (`effect fs, net` or `!{fs, net}`). `Open` carries one or more
/// polymorphic tails (`!{fs | E}`, `!E`, `effect fs | E`, and — once
/// the parser catches up in v0.18 — `!{fs | E1, E2}`) that the
/// typeck layer will instantiate to fresh
/// [`mty_types::effects::row::RowVar`]s on each call.
///
/// The empty bare row-var case (`!E` with no concrete effects) is
/// represented as `Open(vec![], vec![E])`. The empty closed case
/// (`!{}`) is `Closed(vec![])`.
///
/// ## v0.17 — multi-row-var representation
///
/// The second field of `Open` was widened from a single
/// [`HirRowVar`] to `Vec<HirRowVar>`. The v0.15 parser today emits
/// exactly one row variable per fn signature; the v0.18 parser
/// follow-up will emit length-N for `!{fs | E1, E2}`. The HIR
/// representation absorbs that change without further plumbing —
/// the v0.17 typeck already walks the `Vec` to allocate one fresh
/// [`mty_types::effects::row::RowVar`] per row-var name.
///
/// The single-row-var path (the v0.16 SHIPPED-SUBSET) is preserved
/// bit-for-bit: a length-1 vec with one fn-typed parameter still
/// binds to a single fresh row var exactly as before.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirEffectRow {
    /// `!{a, b}` or `effect a, b` — exactly these effects.
    Closed(Vec<HirEffectName>),
    /// `!{a, b | E}` or `!E` (with empty `Vec` of concrete) or
    /// `effect a, b | E` — at least these effects plus whatever the
    /// row variables resolve to at each call site.
    ///
    /// The vec of [`HirRowVar`] always has length >= 1 (else the
    /// variant should be [`HirEffectRow::Closed`]). v0.15 parser
    /// emits length-1; v0.18 parser will emit length-N once
    /// `!{a | E1, E2}` is wired.
    Open(Vec<HirEffectName>, Vec<HirRowVar>),
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

    /// The first row variable, if any. Convenience accessor for the
    /// v0.16 SHIPPED-SUBSET single-row-var path. Prefer
    /// [`Self::row_vars`] when the multi-row-var case matters.
    pub fn row_var(&self) -> Option<&HirRowVar> {
        match self {
            HirEffectRow::Closed(_) => None,
            HirEffectRow::Open(_, vs) => vs.first(),
        }
    }

    /// All row variables in this row, in source order. Length is 0
    /// for [`HirEffectRow::Closed`], 1 for the v0.15 parser's
    /// single-row-var shape, or N for the future v0.18 multi-row-var
    /// shape.
    pub fn row_vars(&self) -> &[HirRowVar] {
        match self {
            HirEffectRow::Closed(_) => &[],
            HirEffectRow::Open(_, vs) => vs.as_slice(),
        }
    }

    /// Number of distinct row variables declared (0 for Closed).
    pub fn row_var_count(&self) -> usize {
        self.row_vars().len()
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
        let r = HirEffectRow::Open(vec![], vec![HirRowVar::new("E", 0)]);
        assert!(r.is_open());
        assert_eq!(r.row_var().unwrap().name.as_str(), "E");
        assert_eq!(r.concrete().len(), 0);
        assert_eq!(r.row_var_count(), 1);
    }

    #[test]
    fn open_with_concrete_carries_both() {
        let r = HirEffectRow::Open(
            vec![HirEffectName::from("fs"), HirEffectName::from("net")],
            vec![HirRowVar::new("E", 0)],
        );
        assert!(r.is_open());
        assert_eq!(r.concrete().len(), 2);
        assert_eq!(r.concrete()[0].as_str(), "fs");
        assert_eq!(r.concrete()[1].as_str(), "net");
        assert_eq!(r.row_var().unwrap().idx, 0);
    }

    /// v0.17: multi-row-var representation. The parser doesn't yet
    /// emit this shape (the v0.15 parser caps at one row var per fn);
    /// this test asserts the HIR representation is ready for the
    /// v0.18 parser extension.
    #[test]
    fn open_multi_row_vars_round_trip() {
        let r = HirEffectRow::Open(
            vec![HirEffectName::from("fs")],
            vec![HirRowVar::new("E1", 0), HirRowVar::new("E2", 1)],
        );
        assert!(r.is_open());
        assert_eq!(r.row_var_count(), 2);
        assert_eq!(r.row_vars()[0].name, "E1");
        assert_eq!(r.row_vars()[1].name, "E2");
        assert_eq!(r.row_vars()[0].idx, 0);
        assert_eq!(r.row_vars()[1].idx, 1);
        // first-var convenience accessor still works.
        assert_eq!(r.row_var().unwrap().name, "E1");
    }

    /// v0.17: closed rows report empty row-var lists (rather than
    /// panicking) so multi-row-var-aware walkers can iterate
    /// uniformly.
    #[test]
    fn closed_row_has_empty_row_vars() {
        let r = HirEffectRow::Closed(vec![HirEffectName::from("fs")]);
        assert_eq!(r.row_var_count(), 0);
        assert!(r.row_vars().is_empty());
        assert!(r.row_var().is_none());
    }
}
