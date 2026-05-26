//! Typed AST accessors for the v0.15 RFC-008 effect-row surface syntax.
//!
//! The v0.15 parser added four new `SyntaxKind` variants for the
//! row-polymorphic effect annotations (`!E`, `!{a, b | E}`, `!{| E}`,
//! `effect a, b | E`):
//!
//!   * [`SyntaxKind::EFFECT_SET`] — the braced `!{...}` body (concrete
//!     effects + optional row-var tail).
//!   * [`SyntaxKind::EFFECT_NAME`] — a single concrete effect name
//!     inside an `EFFECT_SET`.
//!   * [`SyntaxKind::EFFECT_ROW_TAIL`] — the `| E` portion of an
//!     `EFFECT_SET` or the trailing `| E` of the legacy keyword form.
//!   * [`SyntaxKind::EFFECT_ROW_VAR`] — the row variable identifier
//!     itself (e.g. `E`, `R`).
//!
//! This module wraps those nodes in the same `ast_node!`-based typed
//! view used by [`crate::generated`] so the v0.16 HIR lowerer can walk
//! them without re-implementing CST queries.
//!
//! The bare `!E` form (no braces) places an `EFFECT_ROW_VAR` directly
//! under the parent `EFFECT_CLAUSE` (parser
//! `types::effect_clause_bang`); the braced and keyword forms place it
//! under an `EFFECT_ROW_TAIL`. [`EffectClause::row_var_direct`] and
//! [`EffectClause::row_var_via_tail`] expose both shapes so callers can
//! distinguish them when needed; [`EffectClause::row_var`] returns the
//! row-var name irrespective of which shape the parser produced.

#[allow(unused_imports)]
use crate::{ast_node, AstNode, EffectClause};
use mty_syntax::{SyntaxKind, SyntaxNode};

ast_node!(EffectSet, EFFECT_SET);
ast_node!(EffectName, EFFECT_NAME);
ast_node!(EffectRowTail, EFFECT_ROW_TAIL);
ast_node!(EffectRowVar, EFFECT_ROW_VAR);

impl EffectName {
    /// The concrete effect identifier as a [`String`].
    ///
    /// The parser unwraps `paths::name_or_keyword` into this node, so
    /// the first non-trivia token is always the identifier (or a
    /// keyword token allowed as an effect name like `spawn`).
    pub fn text(&self) -> String {
        self.0
            .first_token()
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
}

impl EffectRowVar {
    /// The row-variable identifier as a [`String`] (e.g. `E`, `R`).
    pub fn text(&self) -> String {
        self.0
            .first_token()
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
}

impl EffectRowTail {
    /// The [`EffectRowVar`] inside this tail, if the parser produced
    /// one (it always should for well-formed input — a missing row var
    /// is a parser error).
    pub fn row_var(&self) -> Option<EffectRowVar> {
        self.0.children().find_map(EffectRowVar::cast)
    }
}

impl EffectSet {
    /// Iterate over the concrete effect names inside this `!{...}` set,
    /// in source order.
    pub fn names(&self) -> impl Iterator<Item = EffectName> + '_ {
        self.0.children().filter_map(EffectName::cast)
    }

    /// The trailing `| E` part of the set, if present.
    pub fn row_tail(&self) -> Option<EffectRowTail> {
        self.0.children().find_map(EffectRowTail::cast)
    }
}

impl EffectClause {
    /// The braced `EffectSet` child, if this clause was parsed in the
    /// `!{...}` form (bang-with-braces — RFC-008 §Syntax).
    pub fn effect_set(&self) -> Option<EffectSet> {
        self.0.children().find_map(EffectSet::cast)
    }

    /// The `EffectRowTail` child directly under the clause — only used
    /// by the legacy `effect a, b | E` keyword form. The braced
    /// `!{... | E}` form attaches its tail under the inner `EFFECT_SET`
    /// (use [`Self::effect_set`] → [`EffectSet::row_tail`] for that).
    pub fn keyword_row_tail(&self) -> Option<EffectRowTail> {
        self.0.children().find_map(EffectRowTail::cast)
    }

    /// The bare `!E` form: a single `EFFECT_ROW_VAR` is a direct child
    /// of the `EFFECT_CLAUSE` (no wrapping `EFFECT_ROW_TAIL`).
    pub fn row_var_direct(&self) -> Option<EffectRowVar> {
        self.0.children().find_map(EffectRowVar::cast)
    }

    /// Lookup the row-variable name across all three shapes (`!E`,
    /// `!{... | E}`, `effect a, b | E`). Returns the first match in
    /// source order, or `None` if this clause has no row variable.
    ///
    /// # v0.19 deprecation
    ///
    /// This first-only accessor was the v0.15-v0.18 single-row-var
    /// path. v0.18 broadened the parser surface to emit any number
    /// of `EFFECT_ROW_VAR` children (`!{| E1, E2}` etc.), but this
    /// accessor still returns just the first match — silently
    /// dropping the rest. Prefer [`Self::row_var_names`] which
    /// yields every row var in source order.
    #[deprecated(
        since = "0.19.0",
        note = "use row_var_names() — first-only accessor drops multi-row-var tails"
    )]
    pub fn row_var_name(&self) -> Option<String> {
        if let Some(v) = self.row_var_direct() {
            return Some(v.text());
        }
        if let Some(set) = self.effect_set() {
            if let Some(t) = set.row_tail() {
                if let Some(v) = t.row_var() {
                    return Some(v.text());
                }
            }
        }
        if let Some(t) = self.keyword_row_tail() {
            if let Some(v) = t.row_var() {
                return Some(v.text());
            }
        }
        None
    }

    /// v0.19: iterate over EVERY row-variable identifier in this
    /// clause, in source order, regardless of which of the three
    /// surface shapes it came from:
    ///
    ///   * Bare `!E` — a single direct `EFFECT_ROW_VAR` child.
    ///   * Braced `!{... | E1, E2}` — `EFFECT_ROW_VAR` children of
    ///     the inner `EFFECT_SET → EFFECT_ROW_TAIL`.
    ///   * Legacy `effect a, b | E1, E2` — `EFFECT_ROW_VAR` children
    ///     of the `EFFECT_ROW_TAIL` directly under the clause.
    ///
    /// The v0.18 parser emits at most one shape per clause, so the
    /// three sources never produce duplicate row vars for a single
    /// well-formed clause. Returns an empty iterator if the clause
    /// has no row variable in any shape.
    ///
    /// Single-source-of-truth replacement for the first-only
    /// [`Self::row_var_name`]; the v0.19 HIR lowerer reads this so
    /// every row variable lands in the `Vec<HirRowVar>` that v0.17
    /// typeck already consumes.
    pub fn row_var_names(&self) -> impl Iterator<Item = EffectRowVar> + '_ {
        // Direct `!E` child (only present on the bare form).
        let direct = self.row_var_direct().into_iter();
        // Inner `EFFECT_SET → EFFECT_ROW_TAIL → EFFECT_ROW_VAR*`.
        let braced = self
            .effect_set()
            .and_then(|s| s.row_tail())
            .into_iter()
            .flat_map(|t| t.0.children().filter_map(EffectRowVar::cast));
        // Legacy keyword form: `EFFECT_ROW_TAIL → EFFECT_ROW_VAR*`
        // as a direct child of the clause (no enclosing
        // EFFECT_SET).
        let keyword = self
            .keyword_row_tail()
            .into_iter()
            .flat_map(|t| t.0.children().filter_map(EffectRowVar::cast));
        direct.chain(braced).chain(keyword)
    }

    /// True iff this clause has a row variable in any of the three
    /// shapes (`!E`, `!{a | E}`, `effect a | E`).
    pub fn has_row_var(&self) -> bool {
        self.row_var_names().next().is_some()
    }

    /// The concrete effect names from the braced `!{a, b | E}` form
    /// (returned as [`String`]s). For the legacy `effect a, b | E`
    /// form, the names are bare `NAME` children that the existing
    /// `mty-hir` lowerer already handles via `Name::cast`; this method
    /// only walks the `EFFECT_SET → EFFECT_NAME` children to avoid
    /// double-counting.
    pub fn braced_concrete_names(&self) -> Vec<String> {
        self.effect_set()
            .map(|s| s.names().map(|n| n.text()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: every accessor builds with the right `SyntaxKind`. We
    /// don't synthesize CST nodes here (that would couple to
    /// `mty-syntax`'s parser internals); the kind-discrimination logic
    /// lives in the `ast_node!` macro and is exercised end-to-end by
    /// the `mty-hir` lowerer tests.
    #[test]
    fn syntax_kinds_match() {
        assert!(matches!(SyntaxKind::EFFECT_SET, SyntaxKind::EFFECT_SET));
        assert!(matches!(SyntaxKind::EFFECT_NAME, SyntaxKind::EFFECT_NAME));
        assert!(matches!(
            SyntaxKind::EFFECT_ROW_TAIL,
            SyntaxKind::EFFECT_ROW_TAIL
        ));
        assert!(matches!(
            SyntaxKind::EFFECT_ROW_VAR,
            SyntaxKind::EFFECT_ROW_VAR
        ));
    }
}
