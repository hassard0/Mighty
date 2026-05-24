//! Per-local ownership/borrow state tracked by the linear walker.

use sdust_hir::SourceSpan;
use sdust_types::TyId;
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
    use sdust_types::TyArena;

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
}
