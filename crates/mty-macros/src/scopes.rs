//! Set-of-scopes hygiene primitives (RFC-009).
//!
//! Each occurrence of a name (binding or reference) is associated with
//! a [`Scopes`] — the set of scope identifiers that were active when
//! the name was introduced. Resolution then asks: among all visible
//! bindings whose scope set is a subset of the reference's scope set,
//! choose the one with the largest such subset (Flatt 2016, "Bindings
//! as Sets of Scopes", POPL).
//!
//! v0.13 introduces this data layer alongside the existing
//! mangling-based hygiene. The two are intentionally redundant: marks
//! catch the textbook capture cases, scope sets catch the
//! composition cases (macro-in-macro, swap-macro) that marks miss.
//!
//! ## Why a `BTreeSet`?
//!
//! Scope sets are compared by subset and equality far more often than
//! mutated; storing them as `BTreeSet<ScopeId>` keeps both operations
//! O(n) without hashing overhead and makes [`Scopes`] hashable (used
//! as a map key during resolution).

use std::collections::BTreeSet;

/// A scope identifier. Fresh values are minted by [`ScopeGen`] each
/// time a macro is invoked; the same identifier never recurs within a
/// translation unit.
///
/// `u32` is wide enough for any realistic program (4 billion macro
/// invocations); using a fixed-width integer keeps `Scopes` cheap to
/// clone and hash.
pub type ScopeId = u32;

/// The set of scope IDs attached to a name. Two names with equal
/// [`Scopes`] were introduced in the same expansion context; subset
/// relationships drive resolution.
///
/// `Scopes::default()` is the empty set — used for tokens that came
/// straight from the user's source with no macro involvement.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct Scopes(pub BTreeSet<ScopeId>);

impl Scopes {
    /// The empty scope set. Equivalent to `Scopes::default()` but
    /// reads more clearly at call sites that mean "no macro scopes".
    pub fn empty() -> Self {
        Self(BTreeSet::new())
    }

    /// True if no scopes are present.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of scopes in the set.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// A new set containing every scope in `self` plus `s`.
    ///
    /// Returns a *new* set; the caller-side ergonomics match Flatt's
    /// formal rules, which never mutate an existing name's scope set.
    pub fn with(&self, s: ScopeId) -> Self {
        let mut out = self.0.clone();
        out.insert(s);
        Self(out)
    }

    /// A new set containing every scope in `self` except `s`.
    ///
    /// Used by the "flip" rule: when a macro re-injects a user token
    /// back into its expansion, the macro's own scope is *removed*
    /// from that token so the token still resolves to the user's
    /// binding (per Flatt §3 "Add or Flip").
    pub fn without(&self, s: ScopeId) -> Self {
        let mut out = self.0.clone();
        out.remove(&s);
        Self(out)
    }

    /// In-place insert; returns whether the scope was newly added.
    pub fn insert(&mut self, s: ScopeId) -> bool {
        self.0.insert(s)
    }

    /// In-place remove; returns whether the scope was present.
    pub fn remove(&mut self, s: ScopeId) -> bool {
        self.0.remove(&s)
    }

    /// True iff every scope in `self` is also in `other`.
    pub fn is_subset(&self, other: &Self) -> bool {
        self.0.is_subset(&other.0)
    }

    /// True iff every scope in `other` is also in `self`.
    pub fn is_superset(&self, other: &Self) -> bool {
        self.0.is_superset(&other.0)
    }

    /// Intersection — scopes present in both sets.
    pub fn intersect(&self, other: &Self) -> Self {
        Self(self.0.intersection(&other.0).copied().collect())
    }

    /// Union — scopes present in either set.
    pub fn union(&self, other: &Self) -> Self {
        Self(self.0.union(&other.0).copied().collect())
    }

    /// Iterate scope IDs in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = ScopeId> + '_ {
        self.0.iter().copied()
    }
}

impl FromIterator<ScopeId> for Scopes {
    fn from_iter<I: IntoIterator<Item = ScopeId>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// Monotonic scope-ID allocator. One [`ScopeGen`] is created per
/// translation unit; each macro invocation calls
/// [`ScopeGen::fresh`] to mint a new ID.
///
/// Wrapping is checked: emitting more than `u32::MAX` scopes in a
/// single TU panics — a TU that ever approaches that scale almost
/// certainly has runaway recursive macro expansion that the
/// recursion-depth limit should have caught first.
#[derive(Debug, Default, Clone)]
pub struct ScopeGen {
    next: u32,
}

impl ScopeGen {
    /// A fresh allocator starting from scope-ID 1. (Scope 0 is
    /// reserved so the "empty set" never accidentally equals "the set
    /// containing scope 0", a useful invariant for debug dumps.)
    pub fn new() -> Self {
        Self { next: 1 }
    }

    /// Mint a fresh scope ID. Panics if the u32 space is exhausted —
    /// the recursion limit should prevent this in practice.
    pub fn fresh(&mut self) -> ScopeId {
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .expect("ScopeGen exhausted u32 range");
        id
    }

    /// How many scope IDs have been minted so far (excludes the
    /// reserved 0).
    pub fn count_minted(&self) -> u32 {
        self.next - 1
    }
}

/// Pick the binding whose scope set is the maximal subset of `name`.
///
/// `candidates` are `(scope_set, payload)` pairs — typically `payload`
/// is an opaque binding ID supplied by the caller (the resolver
/// doesn't need to know what a "binding" is, only how to compare
/// scope sets).
///
/// Returns:
///   * `Ok(Some(payload))` — unique best match.
///   * `Ok(None)` — no candidate's scope set is a subset of `name`'s
///     scope set.
///   * `Err(ResolveAmbiguity)` — two or more candidates tied at the
///     same maximum subset size.
///
/// This is the *core* of set-of-scopes name resolution. Callers wrap
/// the `Err` arm in their diagnostic of choice (the macro layer uses
/// `MT5901`; see RFC-009 §6).
pub fn resolve<'a, P: Clone>(
    name: &Scopes,
    candidates: impl IntoIterator<Item = (&'a Scopes, P)>,
) -> Result<Option<P>, ResolveAmbiguity> {
    let mut best: Option<(usize, P)> = None;
    let mut tied = false;
    for (cand_scopes, payload) in candidates {
        if !cand_scopes.is_subset(name) {
            continue;
        }
        let score = cand_scopes.len();
        match &best {
            None => best = Some((score, payload)),
            Some((prev_score, _)) => {
                if score > *prev_score {
                    best = Some((score, payload));
                    tied = false;
                } else if score == *prev_score {
                    tied = true;
                }
            }
        }
    }
    if tied {
        return Err(ResolveAmbiguity);
    }
    Ok(best.map(|(_, p)| p))
}

/// Two or more candidate bindings tied at the same maximum subset
/// score. The macro front-end translates this into MT5901.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveAmbiguity;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_basics() {
        let s = Scopes::empty();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert!(s.is_subset(&Scopes::empty()));
        assert!(Scopes::empty().is_superset(&s));
    }

    #[test]
    fn with_and_without_are_pure() {
        let s = Scopes::empty();
        let s1 = s.with(1);
        let s2 = s1.with(2);
        // Original is unchanged.
        assert!(s.is_empty());
        assert_eq!(s1.len(), 1);
        assert_eq!(s2.len(), 2);
        // without is the inverse of with.
        assert_eq!(s2.without(2), s1);
    }

    #[test]
    fn subset_and_intersect() {
        let a = Scopes::from_iter([1, 2, 3]);
        let b = Scopes::from_iter([2, 3]);
        let c = Scopes::from_iter([2, 4]);
        assert!(b.is_subset(&a));
        assert!(!c.is_subset(&a));
        assert_eq!(a.intersect(&c), Scopes::from_iter([2]));
        assert_eq!(a.union(&c), Scopes::from_iter([1, 2, 3, 4]));
    }

    #[test]
    fn scope_gen_is_monotonic_and_skips_zero() {
        let mut g = ScopeGen::new();
        let a = g.fresh();
        let b = g.fresh();
        let c = g.fresh();
        assert_eq!(a, 1);
        assert_eq!(b, 2);
        assert_eq!(c, 3);
        assert_eq!(g.count_minted(), 3);
    }

    #[test]
    fn resolve_picks_maximal_subset() {
        // Name scope: {1, 2, 3}. Two bindings: x@{1,2} and x@{1}. The
        // x@{1,2} binding wins (larger subset).
        let name = Scopes::from_iter([1, 2, 3]);
        let outer = Scopes::from_iter([1]);
        let inner = Scopes::from_iter([1, 2]);
        let pick = resolve(&name, [(&outer, "outer"), (&inner, "inner")]).unwrap();
        assert_eq!(pick, Some("inner"));
    }

    #[test]
    fn resolve_skips_non_subsets() {
        // Name scope: {1, 2}. Binding scope: {1, 3} — not a subset, skip.
        let name = Scopes::from_iter([1, 2]);
        let bind = Scopes::from_iter([1, 3]);
        let pick = resolve(&name, [(&bind, "wrong")]).unwrap();
        assert_eq!(pick, None);
    }

    #[test]
    fn resolve_reports_ambiguity() {
        // Two distinct bindings, both scope = {1}, name = {1, 2}.
        let name = Scopes::from_iter([1, 2]);
        let a = Scopes::from_iter([1]);
        let b = Scopes::from_iter([1]);
        let err = resolve(&name, [(&a, "a"), (&b, "b")]).unwrap_err();
        assert_eq!(err, ResolveAmbiguity);
    }

    #[test]
    fn resolve_empty_candidate_beats_nothing() {
        // Name scope: {1}. The empty binding is a subset of every
        // name's scope set, so it wins when nothing else is in scope.
        let name = Scopes::from_iter([1]);
        let bind = Scopes::empty();
        let pick = resolve(&name, [(&bind, "global")]).unwrap();
        assert_eq!(pick, Some("global"));
    }
}
