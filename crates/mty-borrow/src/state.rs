//! Per-local ownership/borrow state tracked by the linear walker.
//!
//! v0.3 (A54/A55) extends slice 4 with:
//! - **Place-keyed borrow tracking** so `&mut s.a` and `&s.b` coexist
//!   (see `place.rs`).
//! - **Borrow ledger** mapping each borrower binding → the borrow it
//!   created. When the borrower binding reaches its last-use point,
//!   the corresponding entry on the source Place is decremented (NLL).

use crate::place::Place;
use mty_hir::SourceSpan;
use mty_types::TyId;
use std::collections::HashMap;

/// Slice-4 ownership state for a single local binding.
#[derive(Clone, Debug)]
pub enum Ownership {
    /// The local owns its value (or is a Copy value sitting in storage).
    Owned,
    /// The local was moved; reading it is SD3001.
    Moved { at: SourceSpan },
    /// One or more shared borrows are live; `count` is their count.
    Borrowed { count: u32 },
    /// A single mutable borrow is live.
    BorrowedMut,
    /// Declared but never assigned (let with no init). Reading is SD3015.
    Uninit,
}

/// State for a single local binding inside a fn body.
#[derive(Clone, Debug)]
pub struct LocalState {
    pub name: String,
    pub ty: TyId,
    pub state: Ownership,
    pub declared_at: SourceSpan,
    pub mutable: bool,
    pub is_copy: bool,
    /// `Some(id)` if the local was bound inside an arena region. Used by
    /// the arena escape check.
    pub arena_region: Option<ArenaRegionId>,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
pub struct ArenaRegionId(pub u32);

/// A frame on the borrow-checker's lexical scope stack. Captures the
/// names of locals introduced within this scope (for end-of-scope drop
/// insertion and borrow decay).
#[derive(Default, Clone, Debug)]
pub struct ScopeFrame {
    /// Names introduced in this scope frame (in declaration order).
    pub locals: Vec<String>,
    /// Active arena region for this frame (if it's an arena body).
    pub arena_region: Option<ArenaRegionId>,
}

/// Kind of a live borrow (v0.3 / A54).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorrowKind {
    Shared,
    Mut,
}

/// A live borrow record (v0.3 / A54). Indexed by `Place` (the borrowed
/// location). The borrower binding is recorded so NLL can decay the
/// borrow when that binding hits its last-use point.
#[derive(Clone, Debug)]
pub struct BorrowRecord {
    pub place: Place,
    pub kind: BorrowKind,
    /// The local name holding the `&T` / `&mut T` value (e.g. `r` in
    /// `let r = &x`). `None` for temporary borrows that don't bind a
    /// name (e.g. `use_ref(&x)` directly).
    pub borrower: Option<String>,
    pub at: SourceSpan,
}

/// The ledger of active borrows. Indexed positionally; entries are
/// removed when they decay (last-use of `borrower`) or on scope-end.
#[derive(Default, Clone, Debug)]
pub struct BorrowLedger {
    pub records: Vec<BorrowRecord>,
}

impl BorrowLedger {
    pub fn push(&mut self, r: BorrowRecord) {
        self.records.push(r);
    }

    /// Remove every active borrow whose `borrower` is `name`. Returns
    /// the removed records so callers can update per-Place counters.
    pub fn decay_borrower(&mut self, name: &str) -> Vec<BorrowRecord> {
        let mut removed = vec![];
        self.records.retain(|r| {
            let drop = r.borrower.as_deref() == Some(name);
            if drop {
                removed.push(r.clone());
            }
            !drop
        });
        removed
    }

    /// Remove every active borrow whose `borrower` is in `names`. Used
    /// on scope-end.
    pub fn decay_borrowers<I, S>(&mut self, names: I) -> Vec<BorrowRecord>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut removed = vec![];
        let names: Vec<String> = names.into_iter().map(|s| s.as_ref().to_string()).collect();
        self.records.retain(|r| {
            let drop = match &r.borrower {
                Some(b) => names.iter().any(|n| n == b),
                None => false,
            };
            if drop {
                removed.push(r.clone());
            }
            !drop
        });
        removed
    }

    /// Iterate over the records that conflict with `place` (i.e. share
    /// an overlapping Place).
    pub fn conflicts_with<'a>(
        &'a self,
        place: &'a Place,
    ) -> impl Iterator<Item = &'a BorrowRecord> + 'a {
        self.records.iter().filter(move |r| r.place.overlaps(place))
    }
}

/// Join two state maps. For each key present in both, intersect the
/// states; if either side has `Moved`, both sides are considered Moved
/// after the join (we only require the local to be definitely moved on
/// one branch to *not* be usable afterwards, matching the conservative
/// MVP). For `Borrowed`/`BorrowedMut`, take the more restrictive side.
pub fn join_states(
    mut a: HashMap<String, LocalState>,
    b: &HashMap<String, LocalState>,
) -> HashMap<String, LocalState> {
    for (k, vb) in b {
        if let Some(va) = a.get_mut(k) {
            match (&va.state, &vb.state) {
                (_, Ownership::Moved { at }) | (Ownership::Moved { at }, _) => {
                    va.state = Ownership::Moved { at: at.clone() };
                }
                (Ownership::Uninit, _) | (_, Ownership::Uninit) => {
                    va.state = Ownership::Uninit;
                }
                (Ownership::BorrowedMut, _) | (_, Ownership::BorrowedMut) => {
                    va.state = Ownership::BorrowedMut;
                }
                (Ownership::Borrowed { count: c1 }, Ownership::Borrowed { count: c2 }) => {
                    va.state = Ownership::Borrowed {
                        count: (*c1).max(*c2),
                    };
                }
                _ => { /* both Owned: keep Owned */ }
            }
        } else {
            a.insert(k.clone(), vb.clone());
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place::Place;
    use mty_types::TyArena;

    fn mk(name: &str, ty: TyId, state: Ownership) -> LocalState {
        LocalState {
            name: name.into(),
            ty,
            state,
            declared_at: SourceSpan { start: 0, end: 0 },
            mutable: false,
            is_copy: false,
            arena_region: None,
        }
    }

    #[test]
    fn join_moved_wins() {
        let a = TyArena::new();
        let mut m1 = HashMap::new();
        let mut m2 = HashMap::new();
        m1.insert("x".into(), mk("x", a.i32, Ownership::Owned));
        m2.insert(
            "x".into(),
            mk(
                "x",
                a.i32,
                Ownership::Moved {
                    at: SourceSpan { start: 0, end: 0 },
                },
            ),
        );
        let j = join_states(m1, &m2);
        assert!(matches!(j.get("x").unwrap().state, Ownership::Moved { .. }));
    }

    #[test]
    fn join_borrowed_takes_max() {
        let a = TyArena::new();
        let mut m1 = HashMap::new();
        let mut m2 = HashMap::new();
        m1.insert("x".into(), mk("x", a.i32, Ownership::Borrowed { count: 1 }));
        m2.insert("x".into(), mk("x", a.i32, Ownership::Borrowed { count: 3 }));
        let j = join_states(m1, &m2);
        match j.get("x").unwrap().state {
            Ownership::Borrowed { count } => assert_eq!(count, 3),
            _ => panic!("expected Borrowed"),
        }
    }

    #[test]
    fn ledger_decay_by_borrower() {
        let mut l = BorrowLedger::default();
        l.push(BorrowRecord {
            place: Place::root("x"),
            kind: BorrowKind::Shared,
            borrower: Some("r".into()),
            at: SourceSpan { start: 0, end: 0 },
        });
        l.push(BorrowRecord {
            place: Place::root("y"),
            kind: BorrowKind::Mut,
            borrower: Some("m".into()),
            at: SourceSpan { start: 0, end: 0 },
        });
        let removed = l.decay_borrower("r");
        assert_eq!(removed.len(), 1);
        assert_eq!(l.records.len(), 1);
        assert_eq!(l.records[0].borrower.as_deref(), Some("m"));
    }

    #[test]
    fn ledger_conflicts_detect_disjoint_fields() {
        let mut l = BorrowLedger::default();
        l.push(BorrowRecord {
            place: Place::root("s").with_field("a"),
            kind: BorrowKind::Mut,
            borrower: None,
            at: SourceSpan { start: 0, end: 0 },
        });
        let probe_b = Place::root("s").with_field("b");
        let probe_a_b = Place::root("s").with_field("a").with_field("b");
        assert_eq!(l.conflicts_with(&probe_b).count(), 0);
        assert_eq!(l.conflicts_with(&probe_a_b).count(), 1);
    }
}
