//! Type-namespace and value-namespace definitions. The `DefMap` is the
//! central name-resolution table the inference engine consults.

use crate::ty::{AdtId, EffectId, FnDefId, ParamId, TyId};
use mty_hir::SourceSpan;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdtKind {
    Struct,
    Enum,
    /// Opaque ADT introduced by the prelude for names referenced without a
    /// user-side declaration (`Url`, `Page`, `IoErr`, etc.). Field/variant
    /// access on opaque ADTs returns a fresh inference variable silently.
    Opaque,
}

#[derive(Debug, Clone)]
pub struct ParamDef {
    pub name: String,
    /// Bounds: unused in slice 3. Reserved.
    pub bounds: Vec<TyId>,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    /// `None` for tuple variants like `Enum.Variant(T, U)`. `Some` for
    /// struct fields.
    pub name: Option<String>,
    pub ty: TyId,
}

#[derive(Debug, Clone)]
pub struct VariantDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone)]
pub struct AdtDef {
    pub name: String,
    pub kind: AdtKind,
    pub generics: Vec<ParamDef>,
    /// Global ParamId for each generic slot, parallel to `generics`. Used
    /// when building the substitution map at instantiation sites.
    pub param_ids: Vec<ParamId>,
    /// For structs: exactly one variant whose name matches the struct.
    /// For enums: one per declared variant.
    /// For opaques: empty.
    pub variants: Vec<VariantDef>,
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub generics: Vec<ParamDef>,
    /// Global ParamIds for each generic slot, parallel to `generics`.
    pub param_ids: Vec<ParamId>,
    pub params: Vec<(String, TyId)>,
    pub ret: TyId,
    pub effects: Vec<EffectId>,
    pub is_pub: bool,
    /// `None` for built-in/extern fns. `Some` for fns with a Mighty body.
    pub body: Option<mty_hir::BlockId>,
    /// Original HIR fn for body-checking. `None` for built-ins.
    pub hir_fn: Option<mty_hir::FnId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefRef {
    Adt(AdtId),
    Fn(FnDefId),
    /// `Variant(adt_id, variant_index)` — e.g. `Option.Some`.
    Variant(AdtId, usize),
    Module(ModuleId),
    Param(ParamId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(pub u32);

/// A built-in method dispatched from `(receiver_shape, method_name)`. The
/// receiver shape lets us key the table without needing full trait
/// resolution.
///
/// v0.15: now carries an optional `row_sig` factory for stdlib HOFs
/// (e.g. `Iterator.map`, `Option.and_then`). When `Some`, the call-site
/// effect walker instantiates the [`crate::effects::row::RowPolySig`] it
/// returns, unifies the closure-argument's inferred effect row against the
/// signature's parameter row, then propagates the resolved return row into
/// the caller's effect set. The factory is stored as a `fn` pointer rather
/// than an owned `RowPolySig` so this struct can stay in `defs.rs` without
/// pulling the `effects::row` module into the dependency graph here (avoids
/// an `effects → defs → effects` import cycle).
#[derive(Debug, Clone)]
pub struct BuiltinMethod {
    /// Number of arguments (excluding the receiver). For variadic methods,
    /// set to `None` and the checker accepts any arity.
    pub arity: Option<usize>,
    /// Return type. `None` means "fresh inference variable" (permissive).
    pub ret: Option<TyId>,
    /// v0.15: row-polymorphic stdlib HOF signature factory. `None` for
    /// non-HOF methods (the legacy permissive behavior). `Some` registers
    /// the method as a row-poly stdlib HOF; the dispatcher in
    /// `crate::effects::walk_expr_effects` consults it on each call.
    pub row_sig: Option<RowSigFactory>,
}

/// v0.15: a factory producing a [`crate::effects::row::RowPolySig`] on
/// demand. Stored as a `fn` pointer to avoid importing the type into
/// `defs.rs` (cycle break — see [`BuiltinMethod::row_sig`]).
pub type RowSigFactory = fn() -> crate::effects::row::RowPolySig;

#[derive(Debug, Default)]
pub struct DefMap {
    pub adts: Vec<AdtDef>,
    pub fns: Vec<FnDef>,
    /// Top-level name lookup. May contain duplicates only for value/type
    /// disambiguation handled by `lookup_value` vs `lookup_type`.
    pub by_name: HashMap<String, DefRef>,
    /// Effect name -> id interner.
    pub effects: HashMap<String, EffectId>,
    /// Module table.
    pub modules: Vec<String>,
    pub module_by_name: HashMap<String, ModuleId>,
    /// Builtin method table keyed by method name only (slice-3
    /// simplification: we don't fork by receiver shape because the
    /// permissive return-Var fallback covers shape mismatches).
    pub builtin_methods: HashMap<String, BuiltinMethod>,
    /// Map an HIR fn id to its FnDefId.
    pub hir_fn_to_def: HashMap<mty_hir::FnId, FnDefId>,
    /// Map an HIR struct id to its AdtId.
    pub hir_struct_to_adt: HashMap<mty_hir::StructId, AdtId>,
    /// Map an HIR enum id to its AdtId.
    pub hir_enum_to_adt: HashMap<mty_hir::EnumId, AdtId>,
    /// Generic param symbol table indexed by paramid.
    pub params: Vec<ParamDef>,
    /// Slice-4 impl-method index: `(self_adt_id, method_name)` → FnDefId.
    pub impl_methods: HashMap<(AdtId, String), FnDefId>,
    /// Slice-4 protocol message index: `(protocol_name, msg_name)` →
    /// parameter types (parallel to the message's declared params). Used
    /// to type agent handler params by looking up the implemented protocol.
    pub protocol_msgs: HashMap<(String, String), Vec<TyId>>,
    /// Slice-5 protocol message names per protocol (declaration order),
    /// used for arity / missing-handler / extra-handler checks.
    pub protocol_msg_names: HashMap<String, Vec<String>>,
    /// Slice-5 set of user ADTs marked Copy via `#[derive(Copy)]`.
    pub user_copy: HashSet<AdtId>,
    /// v0.3 (A65) set of user ADTs marked Sendable via
    /// `#[derive(Sendable)]`. Sendable cross-agent messaging gate; see
    /// `crate::sendable` for the rules.
    pub user_sendable: HashSet<AdtId>,
    /// v0.27 Track B set of opaque ADTs declared by the prelude (i.e.
    /// from `std.*`) that are explicitly handler-safe — they bypass the
    /// MT2021 strict-handler-scope check in `check::synth_path`. The
    /// rationale: `std.*` opaque ADTs are effect-bearing, but the
    /// effect surface is already tracked through the fn's `!{...}`
    /// clause, so allowing their constructors inside `on Ask(...)` is
    /// safe and avoids the v0.26 demo 07 ctor-in-main workaround. See
    /// `dev/history/notes/OPAQUE_ADT_WASM_V0_27_NOTES.md`.
    pub handler_safe_adts: HashSet<AdtId>,
    /// Slice-5 trait coherence + dispatch table.
    pub traits: TraitTable,
}

/// Trait coherence + dispatch table (slice 5).
#[derive(Default, Debug)]
pub struct TraitTable {
    /// Per-trait declared method signatures (object-safety check, dyn dispatch).
    pub trait_methods: HashMap<String, Vec<TraitMethodSig>>,
    /// All `impl Trait for T` registrations.
    pub impls: Vec<TraitImpl>,
    /// `(receiver_adt, method_name) -> Vec<(trait_name, fn_def_id)>`.
    /// Used at method-call sites to find trait-provided methods.
    pub by_method: HashMap<(AdtId, String), Vec<(String, FnDefId)>>,
    /// Set of `(trait_name, self_adt_id)` pairs for coherence (overlap)
    /// detection.
    pub impl_keys: HashSet<(String, AdtId)>,
}

#[derive(Debug, Clone)]
pub struct TraitImpl {
    pub trait_name: String,
    pub self_adt: AdtId,
    pub method_fns: HashMap<String, FnDefId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct TraitMethodSig {
    pub name: String,
    /// Slice-5 object-safety conservative flag: true iff the method's
    /// signature mentions `Self` (in return or parameter type).
    pub has_self_ty: bool,
    /// Slice-5 object-safety: true iff the method has its own generics.
    pub has_generics: bool,
}

impl DefMap {
    pub fn adt(&self, id: AdtId) -> Option<&AdtDef> {
        self.adts.get(id.0 as usize)
    }

    pub fn adt_mut(&mut self, id: AdtId) -> Option<&mut AdtDef> {
        self.adts.get_mut(id.0 as usize)
    }

    pub fn fn_def(&self, id: FnDefId) -> Option<&FnDef> {
        self.fns.get(id.0 as usize)
    }

    pub fn fn_def_mut(&mut self, id: FnDefId) -> Option<&mut FnDef> {
        self.fns.get_mut(id.0 as usize)
    }

    pub fn intern_effect(&mut self, name: impl Into<String>) -> EffectId {
        let name = name.into();
        if let Some(id) = self.effects.get(&name) {
            return *id;
        }
        let id = EffectId(self.effects.len() as u32);
        self.effects.insert(name, id);
        id
    }

    pub fn alloc_adt(&mut self, def: AdtDef) -> AdtId {
        let id = AdtId(self.adts.len() as u32);
        self.adts.push(def);
        id
    }

    pub fn alloc_fn(&mut self, def: FnDef) -> FnDefId {
        let id = FnDefId(self.fns.len() as u32);
        self.fns.push(def);
        id
    }

    pub fn alloc_module(&mut self, name: impl Into<String>) -> ModuleId {
        let name = name.into();
        if let Some(id) = self.module_by_name.get(&name) {
            return *id;
        }
        let id = ModuleId(self.modules.len() as u32);
        self.modules.push(name.clone());
        self.module_by_name.insert(name, id);
        id
    }

    pub fn alloc_param(&mut self, def: ParamDef) -> ParamId {
        let id = ParamId(self.params.len() as u32);
        self.params.push(def);
        id
    }

    pub fn lookup(&self, name: &str) -> Option<DefRef> {
        self.by_name.get(name).copied()
    }

    /// v0.27 Track B — returns `true` iff `name` resolves to a prelude
    /// `std.*` opaque ADT that has been marked handler-safe (e.g.
    /// `Working`, `VectorStore`, `AnthropicClient`). Strict-scope
    /// MT2021 (`check::synth_path`) consults this so the v0.26 demo 07
    /// pattern — constructing a `std.memory.Working` value inside an
    /// `on Ask()` handler — type-checks. User-defined opaque ADTs that
    /// haven't been registered here continue to hit MT2021 (back-compat
    /// with v0.3 A65). See
    /// `dev/history/notes/OPAQUE_ADT_WASM_V0_27_NOTES.md`.
    pub fn is_handler_safe_name(&self, name: &str) -> bool {
        match self.lookup(name) {
            Some(DefRef::Adt(aid)) => self.handler_safe_adts.contains(&aid),
            _ => false,
        }
    }

    /// Multi-segment path lookup. For `std.http`, walks `std → http`. Slice
    /// 3 returns the *final* DefRef if the path resolves; for paths that
    /// dive into a module's members it returns `Module(module_of_member)`
    /// because we don't model module contents in detail.
    pub fn lookup_path(&self, segments: &[String]) -> Option<DefRef> {
        if segments.is_empty() {
            return None;
        }
        if segments.len() == 1 {
            return self.lookup(&segments[0]);
        }
        // Try the full dotted name first (e.g. `std.http`).
        let dotted = segments.join(".");
        if let Some(d) = self.lookup(&dotted) {
            return Some(d);
        }
        // Try the joined prefix as a module.
        for split in (1..segments.len()).rev() {
            let prefix = segments[..split].join(".");
            if let Some(DefRef::Module(_)) = self.lookup(&prefix) {
                // Member of module: opaque — return a synthetic Module ref
                // so callers can decide. (We re-use the module id of the
                // prefix.)
                if let Some(DefRef::Module(m)) = self.lookup(&prefix) {
                    return Some(DefRef::Module(m));
                }
            }
        }
        // Fall back to first segment if it's a module — treat the whole
        // path as a module access.
        if let Some(DefRef::Module(m)) = self.lookup(&segments[0]) {
            return Some(DefRef::Module(m));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_and_lookup() {
        let mut d = DefMap::default();
        let aid = d.alloc_adt(AdtDef {
            name: "Foo".into(),
            kind: AdtKind::Struct,
            generics: vec![],
            param_ids: vec![],
            variants: vec![],
        });
        d.by_name.insert("Foo".into(), DefRef::Adt(aid));
        assert_eq!(d.lookup("Foo"), Some(DefRef::Adt(aid)));
        assert!(d.lookup("Bar").is_none());
    }

    #[test]
    fn module_path_resolves() {
        let mut d = DefMap::default();
        let m = d.alloc_module("std.http");
        d.by_name.insert("std".into(), DefRef::Module(m));
        d.by_name.insert("std.http".into(), DefRef::Module(m));
        assert_eq!(
            d.lookup_path(&["std".into(), "http".into()]),
            Some(DefRef::Module(m))
        );
        assert_eq!(
            d.lookup_path(&["std".into(), "http".into(), "serve".into()]),
            Some(DefRef::Module(m))
        );
    }
}
