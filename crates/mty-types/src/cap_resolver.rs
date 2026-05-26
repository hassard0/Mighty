//! v0.21 — Capability name resolution.
//!
//! Closes the "cap-name resolver" gap on the Post-v1.0 roadmap. The
//! resolver maintains:
//!
//! - A flat registry of *declared* capability specs keyed by name.
//! - A stack of scope frames; each frame is a set of names that are
//!   currently in scope at this lexical depth. Pushing a frame
//!   models entering a `with cap(...)` or sandbox-with body; popping
//!   the frame undeclares every name introduced in it.
//!
//! The resolver itself is decoupled from the surface syntax — it
//! consumes (name, CapSpec) pairs that the caller (`mty-types::check`
//! and `mty-types::items`) feeds in. This lets `mty-types` own the
//! load-bearing six MT4060..MT4065 emit sites without pulling
//! `mty-syntax` or `mty-hir` into the dependency edge.
//!
//! ## The six newly active codes
//!
//! - **MT4060** [`CapResolutionError::Unbound`] — name not declared
//!   and not in any active scope frame.
//! - **MT4061** [`CapResolutionError::FamilyMismatch`] — declared
//!   family does not match the use-site's expected family.
//! - **MT4062** [`CapResolutionError::ScopeViolation`] — reference to
//!   a name that was popped (e.g. via `with cap(...)` body
//!   completion).
//! - **MT4063** [`CapResolutionError::Redeclaration`] — same name
//!   declared twice in the same scope frame.
//! - **MT4064** [`CapResolutionError::UnknownMethod`] — method name
//!   not in the resolved family's built-in surface.
//! - **MT4065** [`CapResolutionError::InvalidConstraint`] — narrowing
//!   constraint not accepted by the family.
//!
//! Each resolution path returns a `CapResolutionError` carrying the
//! detail needed to render a precise diagnostic; consumers in
//! `crate::diag` convert these into `Diagnostic`s with the correct
//! code.

use crate::ty::{CapConstraint, CapFamily};
use std::collections::HashMap;

/// A declared capability — name + family + initial constraint.
///
/// Mighty's surface syntax allows users to declare caps either at
/// module scope (`cap Foo: Fs`) or at handler-binding sites
/// (`with cap fs: Fs.ro("/data") ...`). Both lower into a `CapSpec`
/// consumed by the resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapSpec {
    pub family: CapFamily,
    pub constraint: CapConstraint,
}

impl CapSpec {
    pub fn new(family: CapFamily, constraint: CapConstraint) -> Self {
        Self { family, constraint }
    }

    pub fn top(family: CapFamily) -> Self {
        Self {
            family,
            constraint: CapConstraint::Any,
        }
    }
}

/// Resolution failure shapes. Each variant maps 1:1 to one of the six
/// v0.21 cap-resolver codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapResolutionError {
    /// MT4060 — name not declared anywhere.
    Unbound { name: String },
    /// MT4061 — declared family differs from expected.
    FamilyMismatch {
        name: String,
        declared: CapFamily,
        expected: CapFamily,
    },
    /// MT4062 — name was bound in a scope frame that has been popped.
    ScopeViolation {
        name: String,
        popped_at_depth: usize,
    },
    /// MT4063 — declaration would overwrite an active binding in the
    /// same scope frame.
    Redeclaration { name: String, frame_depth: usize },
    /// MT4064 — method not in the family's surface.
    UnknownMethod {
        family: CapFamily,
        method: String,
        available: Vec<String>,
    },
    /// MT4065 — narrowing constraint not accepted by family.
    InvalidConstraint {
        family: CapFamily,
        method: String,
        reason: String,
    },
}

/// Capability-name resolver.
///
/// The flat `declared` map is the module-level registry; the
/// `in_scope` stack frames mirror the handler-body / sandbox / cap-
/// narrow scope structure.
///
/// `popped` is a recency cache used to surface MT4062 instead of
/// MT4060 when the name was bound *recently* but the binding frame
/// has been popped. The cap-resolver pass walks bodies in a single
/// sweep, so the cache is bounded by handler size.
#[derive(Debug, Default)]
pub struct CapResolver {
    declared: HashMap<String, CapSpec>,
    in_scope: Vec<Vec<(String, CapSpec)>>,
    popped: HashMap<String, usize>,
}

impl CapResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of currently active scope frames (zero before any
    /// `push_scope`).
    pub fn depth(&self) -> usize {
        self.in_scope.len()
    }

    /// Declare a module-level capability. Visible in every active
    /// scope. Returns Redeclaration if the same name was already
    /// declared at module level OR is currently bound in the topmost
    /// scope frame.
    pub fn declare(&mut self, name: &str, spec: CapSpec) -> Result<(), CapResolutionError> {
        if self.declared.contains_key(name) {
            return Err(CapResolutionError::Redeclaration {
                name: name.to_string(),
                frame_depth: 0,
            });
        }
        // Same-frame collision with a top-scope bind also fires
        // MT4063 — declarations into module level cannot collide
        // with the topmost active frame.
        if let Some(top) = self.in_scope.last() {
            if top.iter().any(|(n, _)| n == name) {
                return Err(CapResolutionError::Redeclaration {
                    name: name.to_string(),
                    frame_depth: self.in_scope.len(),
                });
            }
        }
        self.declared.insert(name.to_string(), spec);
        Ok(())
    }

    /// Open a fresh scope frame. Bindings introduced via
    /// [`bind_in_scope`] in this frame are dropped on
    /// [`pop_scope`].
    pub fn push_scope(&mut self) {
        self.in_scope.push(vec![]);
    }

    /// Bind a capability into the current scope frame. Returns
    /// Redeclaration if the same name is already bound in this frame.
    /// Shadowing across frames is permitted (the inner binding wins
    /// while the frame is active).
    pub fn bind_in_scope(&mut self, name: &str, spec: CapSpec) -> Result<(), CapResolutionError> {
        if self.in_scope.is_empty() {
            // No active frame — treat as module-level declare.
            return self.declare(name, spec);
        }
        let depth = self.in_scope.len();
        let top = self.in_scope.last_mut().unwrap();
        if top.iter().any(|(n, _)| n == name) {
            return Err(CapResolutionError::Redeclaration {
                name: name.to_string(),
                frame_depth: depth,
            });
        }
        top.push((name.to_string(), spec));
        Ok(())
    }

    /// Pop the topmost scope frame, undeclaring every name introduced
    /// in it. Names popped here become MT4062 fodder if referenced
    /// later in the same resolver session.
    pub fn pop_scope(&mut self) {
        if let Some(frame) = self.in_scope.pop() {
            let depth = self.in_scope.len() + 1;
            for (name, _) in frame {
                self.popped.insert(name, depth);
            }
        }
    }

    /// Resolve a name to its `CapSpec`. Walks the scope stack
    /// inside-out, then the module-level registry. Reports MT4062
    /// instead of MT4060 if the name was popped during this session.
    pub fn resolve(&self, name: &str) -> Result<&CapSpec, CapResolutionError> {
        for frame in self.in_scope.iter().rev() {
            for (n, spec) in frame.iter().rev() {
                if n == name {
                    return Ok(spec);
                }
            }
        }
        if let Some(spec) = self.declared.get(name) {
            return Ok(spec);
        }
        if let Some(depth) = self.popped.get(name) {
            return Err(CapResolutionError::ScopeViolation {
                name: name.to_string(),
                popped_at_depth: *depth,
            });
        }
        Err(CapResolutionError::Unbound {
            name: name.to_string(),
        })
    }

    /// Resolve a name and assert its family matches `expected`.
    /// Returns FamilyMismatch when the families differ (after
    /// successful name lookup); otherwise propagates the underlying
    /// resolution error.
    pub fn resolve_as(
        &self,
        name: &str,
        expected: &CapFamily,
    ) -> Result<&CapSpec, CapResolutionError> {
        let spec = self.resolve(name)?;
        if &spec.family != expected {
            return Err(CapResolutionError::FamilyMismatch {
                name: name.to_string(),
                declared: spec.family.clone(),
                expected: expected.clone(),
            });
        }
        Ok(spec)
    }

    /// Validate a method call on a known capability family. Returns
    /// the resolved method or an UnknownMethod error listing the
    /// available surface methods.
    ///
    /// Operational methods (those handled by the typeck permissive
    /// fallback — read/write/list/get/post/now/...) are accepted
    /// without surfacing in the available list, so MT4064 fires
    /// only on truly unknown method names.
    pub fn check_method(
        &self,
        family: &CapFamily,
        method: &str,
    ) -> Result<&'static str, CapResolutionError> {
        let available = family_methods(family);
        if let Some(m) = available.iter().find(|&&m| m == method).copied() {
            return Ok(m);
        }
        if is_operational_method(family, method) {
            // Operational methods aren't in the narrowing surface
            // but are still valid — let the typeck handle them.
            return Ok("__operational");
        }
        Err(CapResolutionError::UnknownMethod {
            family: family.clone(),
            method: method.to_string(),
            available: available.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Validate that a narrowing constructor is compatible with the
    /// family — e.g. `Net.host(_)` is OK but `Net.ro()` is not.
    ///
    /// `constraint_shape` is the spec-tag of the constraint the
    /// constructor would produce (`"ReadOnly"`, `"Path"`, `"Host"`).
    pub fn check_narrowing(
        &self,
        family: &CapFamily,
        method: &str,
        constraint: &CapConstraint,
    ) -> Result<(), CapResolutionError> {
        // Method must exist on the family.
        let _ = self.check_method(family, method)?;
        let ok = match (family, constraint) {
            // Fs accepts: ReadOnly, Path, And.
            (CapFamily::Fs, CapConstraint::ReadOnly | CapConstraint::Path(_)) => true,
            // Net accepts: Host with at least one entry, And.
            (CapFamily::Net, CapConstraint::Host(hs)) => !hs.is_empty(),
            // And: every nested constraint must be acceptable to the
            // family. (Recursion is intentionally shallow — slice-5's
            // And is single-level.)
            (fam, CapConstraint::And(xs)) => xs
                .iter()
                .all(|x| self.check_narrowing(fam, method, x).is_ok()),
            // Any narrowing is always trivially OK (no restriction).
            (_, CapConstraint::Any) => true,
            // Clock / Dom / Model / Custom: no built-in narrowing in
            // slice 5; reject every non-Any constraint to push users
            // toward the typed-message-with-narrowed-authority
            // pattern.
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(CapResolutionError::InvalidConstraint {
                family: family.clone(),
                method: method.to_string(),
                reason: format!(
                    "family `{}` does not accept this narrowing constraint",
                    pretty_family(family)
                ),
            })
        }
    }

    /// True iff `name` is currently resolvable (either in scope or
    /// module-declared). Used by [`mty_types::check`] to gate strict
    /// MT2021 emission when the name is a known cap.
    pub fn is_known(&self, name: &str) -> bool {
        if self.declared.contains_key(name) {
            return true;
        }
        self.in_scope
            .iter()
            .any(|f| f.iter().any(|(n, _)| n == name))
    }

    /// Enumerate names visible at the current depth (declared +
    /// every active frame, inside-out, de-duplicated). Used by the
    /// MT2021 "did you mean" hint path.
    pub fn visible_names(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = vec![];
        for frame in self.in_scope.iter().rev() {
            for (n, _) in frame {
                if seen.insert(n.clone()) {
                    out.push(n.clone());
                }
            }
        }
        for n in self.declared.keys() {
            if seen.insert(n.clone()) {
                out.push(n.clone());
            }
        }
        out
    }
}

/// Per-family method surface table. Mirrors the runtime built-ins
/// listed in `docs/internals/capabilities.md`.
pub fn family_methods(family: &CapFamily) -> &'static [&'static str] {
    // v0.21 surface enumerates ONLY the narrowing constructors that
    // the cap-resolver pass validates. Operational methods are
    // delegated to the typeck's permissive-cap fallback (slice 5)
    // because the runtime / extern surface is open-ended.
    //
    // Surface methods (validated):
    //   Fs   → ro, path
    //   Net  → host
    //   Clock, Dom, Model — no narrowing constructors today; an
    //                       empty surface means MT4064 never fires
    //                       on these families (operational methods
    //                       go through the typeck fallback).
    match family {
        CapFamily::Fs => &["ro", "path"],
        CapFamily::Net => &["host"],
        CapFamily::Clock => &[],
        CapFamily::Dom => &[],
        CapFamily::Model => &[],
        CapFamily::Custom(_) => &[],
    }
}

/// Is `method` a *known operational* method on `family` — one that
/// goes through the typeck permissive-cap fallback and should NOT
/// trigger MT4064 even though it's not in the narrowing surface?
pub fn is_operational_method(family: &CapFamily, method: &str) -> bool {
    match family {
        CapFamily::Fs => matches!(
            method,
            "read" | "write" | "list" | "open" | "create" | "exists" | "remove"
        ),
        CapFamily::Net => matches!(method, "get" | "post" | "connect" | "fetch" | "request"),
        CapFamily::Clock => matches!(method, "now" | "sleep" | "elapsed" | "deadline"),
        CapFamily::Dom => matches!(method, "query" | "render" | "mount" | "select" | "update"),
        CapFamily::Model => matches!(method, "call" | "stream" | "embed" | "complete" | "chat"),
        // Custom: every method is permissively accepted.
        CapFamily::Custom(_) => true,
    }
}

fn pretty_family(family: &CapFamily) -> &'static str {
    match family {
        CapFamily::Fs => "Fs",
        CapFamily::Net => "Net",
        CapFamily::Clock => "Clock",
        CapFamily::Dom => "Dom",
        CapFamily::Model => "Model",
        CapFamily::Custom(_) => "<custom>",
    }
}
