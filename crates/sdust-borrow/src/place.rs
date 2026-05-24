//! Place algebra for field-level borrow tracking (v0.3 / A54).
//!
//! A `Place` is a rooted projection path identifying a memory location:
//!
//! ```text
//! Place := root local | Field(Place, name) | Index(Place) | Deref(Place)
//! ```
//!
//! Two borrows conflict iff their Places **overlap**: one is a prefix
//! of the other (e.g. `s.a` overlaps `s` and `s.a.b`; `s.a` does **not**
//! overlap `s.b`).
//!
//! v0.3 ships field-level disjointness one level deep (`&mut s.a` and
//! `&s.b` coexist). Deeper projections degrade to whole-struct (see the
//! `truncate_for_v0_3` helper); A56 tightens this in v0.4.

use std::fmt;

/// A projection step over a root local.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Proj {
    /// `place.field`
    Field(String),
    /// `place[i]` — slice 4 collapses all indices into one "any index"
    /// edge to stay tractable.
    Index,
    /// `*place`
    Deref,
}

/// A rooted projection path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Place {
    pub root: String,
    pub projs: Vec<Proj>,
}

impl Place {
    /// `Place::root("x")` — a bare local.
    pub fn root(name: impl Into<String>) -> Self {
        Self {
            root: name.into(),
            projs: vec![],
        }
    }

    pub fn with_field(mut self, name: impl Into<String>) -> Self {
        self.projs.push(Proj::Field(name.into()));
        self
    }

    pub fn with_deref(mut self) -> Self {
        self.projs.push(Proj::Deref);
        self
    }

    pub fn with_index(mut self) -> Self {
        self.projs.push(Proj::Index);
        self
    }

    /// Is this place a prefix of `other`?
    /// (`s` is a prefix of `s.a`; `s.a` is a prefix of `s.a.b`.)
    pub fn is_prefix_of(&self, other: &Place) -> bool {
        if self.root != other.root {
            return false;
        }
        if self.projs.len() > other.projs.len() {
            return false;
        }
        self.projs
            .iter()
            .zip(other.projs.iter())
            .all(|(a, b)| a == b)
    }

    /// Do these two places overlap (i.e. either is a prefix of the other)?
    /// Two completely disjoint paths return false.
    pub fn overlaps(&self, other: &Place) -> bool {
        self.is_prefix_of(other) || other.is_prefix_of(self)
    }

    /// v0.3 keeps at most ONE projection step. Deeper projections fall
    /// back to whole-root borrow (conservative — accepts fewer programs
    /// but stays sound). v0.4 will extend.
    pub fn truncate_for_v0_3(&self) -> Place {
        let mut p = Place::root(&self.root);
        if let Some(first) = self.projs.first() {
            p.projs.push(first.clone());
        }
        p
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.root)?;
        for p in &self.projs {
            match p {
                Proj::Field(n) => write!(f, ".{}", n)?,
                Proj::Index => f.write_str("[_]")?,
                Proj::Deref => f.write_str(".*")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_overlaps_self() {
        let a = Place::root("x");
        assert!(a.overlaps(&a));
    }

    #[test]
    fn disjoint_fields_dont_overlap() {
        let a = Place::root("s").with_field("a");
        let b = Place::root("s").with_field("b");
        assert!(!a.overlaps(&b));
        assert!(!b.overlaps(&a));
    }

    #[test]
    fn parent_overlaps_child() {
        let s = Place::root("s");
        let s_a = Place::root("s").with_field("a");
        assert!(s.overlaps(&s_a));
        assert!(s_a.overlaps(&s));
    }

    #[test]
    fn nested_field_overlap() {
        let a = Place::root("s").with_field("a");
        let a_b = Place::root("s").with_field("a").with_field("b");
        assert!(a.overlaps(&a_b));
    }

    #[test]
    fn disjoint_roots_never_overlap() {
        let x = Place::root("x");
        let y = Place::root("y");
        assert!(!x.overlaps(&y));
    }

    #[test]
    fn truncate_keeps_one_level() {
        let deep = Place::root("s")
            .with_field("a")
            .with_field("b")
            .with_field("c");
        let t = deep.truncate_for_v0_3();
        assert_eq!(t.projs.len(), 1);
        assert_eq!(t.projs[0], Proj::Field("a".into()));
    }

    #[test]
    fn truncate_root_stays_root() {
        let r = Place::root("x");
        assert_eq!(r.truncate_for_v0_3(), r);
    }
}
