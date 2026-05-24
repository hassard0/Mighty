//! Resolved type representation. Distinct from `mty_hir::HirType` (which
//! is purely syntactic). `TyData` values are interned in a `TyArena` so
//! equal types share the same `TyId`. Inference variables live in a
//! `Substitution` (see `infer.rs`), not in the arena.
//!
//! Reference: slice 3 design §3.3.

use la_arena::{Arena, Idx};
use std::collections::HashMap;

pub type TyId = Idx<TyData>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyVarId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParamId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AdtId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnDefId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntKind {
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    USize,
    ISize,
    /// Unsuffixed integer literal — defaults to I32 if not constrained.
    IntInfer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatKind {
    F32,
    F64,
    /// Unsuffixed float literal — defaults to F64 if not constrained.
    FloatInfer,
}

/// Capability family (spec §8). Each family has its own narrowing
/// constructors and built-in method surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CapFamily {
    Net,
    Fs,
    Clock,
    Dom,
    Model,
    /// User-declared `cap Foo` family. The string is the declared name.
    Custom(String),
}

/// Capability constraint — narrowed authority. Slice 5 only models the
/// subset needed for spec §8.1 examples: top, read-only, path-prefix
/// glob, host allowlist, and conjunction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CapConstraint {
    /// Top — no restriction.
    Any,
    /// Read-only narrowing (applies to Fs only in slice 5).
    ReadOnly,
    /// Path-prefix narrowing — `Path("/data")` accepts only paths under
    /// `/data`.
    Path(String),
    /// Network host:port allowlist (accepts only the listed entries).
    Host(Vec<String>),
    /// All-of conjunction — every sub-constraint holds.
    And(Vec<CapConstraint>),
}

impl CapConstraint {
    /// Returns true iff `self` is at least as narrow as `other` (so an
    /// arg with constraint `self` can satisfy a parameter with
    /// constraint `other`).
    pub fn is_narrower_or_eq(&self, other: &CapConstraint) -> bool {
        // Top accepts everything; narrower-than-top is trivially true.
        if matches!(other, CapConstraint::Any) {
            return true;
        }
        if self == other {
            return true;
        }
        // `And(xs)` narrower than `c` iff any element of xs is narrower
        // than `c` (set of constraints; each adds restriction).
        if let CapConstraint::And(xs) = self {
            return xs.iter().any(|x| x.is_narrower_or_eq(other));
        }
        match (self, other) {
            (CapConstraint::Path(a), CapConstraint::Path(b)) => a.starts_with(b.as_str()),
            (CapConstraint::Host(a), CapConstraint::Host(b)) => a.iter().all(|h| b.contains(h)),
            (CapConstraint::ReadOnly, CapConstraint::ReadOnly) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TyData {
    Bool,
    Int(IntKind),
    Float(FloatKind),
    Char,
    Str,
    String,
    Bytes,
    Unit,
    Never,
    /// Duration literal type (`Duration`). Opaque ADT-like; minimal slice-3 support.
    Duration,
    /// Size literal type (`Size`). Opaque.
    Size,
    Tuple(Vec<TyId>),
    Array {
        elem: TyId,
        /// `None` for slice types `&[T]`; `Some(n)` for fixed arrays.
        len: Option<u64>,
    },
    Ref {
        mutable: bool,
        inner: TyId,
    },
    Fn {
        params: Vec<TyId>,
        ret: TyId,
        effects: Vec<EffectId>,
    },
    /// Algebraic data type (struct/enum) with optional type arguments.
    Adt(AdtId, Vec<TyId>),
    /// Inference variable.
    Var(TyVarId),
    /// Generic parameter slot (e.g. `T` inside `fn first[T](xs: &[T])`).
    Param(ParamId),
    /// Raw pointer (`*U8`). Slice 3: only the unsafe-block surface; no
    /// real semantics.
    RawPtr(TyId),
    /// Opaque module like `std.http`. Field access returns Var.
    Module(String),
    /// Capability value (spec §8). Carries family + narrowing
    /// constraint; subsumption is checked at call sites (MT4010).
    Cap {
        family: CapFamily,
        constraint: CapConstraint,
    },
    /// `dyn Trait` — dynamically-dispatched value of a trait object.
    /// Slice 5 keeps the type opaque to the back-end and only validates
    /// object-safety + coercion sites.
    Dyn {
        trait_name: String,
    },
    /// Poisoned type. Suppresses cascading diagnostics.
    Error,
}

#[derive(Debug)]
pub struct TyArena {
    storage: Arena<TyData>,
    intern: HashMap<TyData, TyId>,
    /// Pre-interned primitives.
    pub bool_: TyId,
    pub unit: TyId,
    pub never: TyId,
    pub char_: TyId,
    pub str_: TyId,
    pub string: TyId,
    pub bytes: TyId,
    pub error: TyId,
    pub duration: TyId,
    pub size: TyId,
    pub int_infer: TyId,
    pub float_infer: TyId,
    pub i8: TyId,
    pub i16: TyId,
    pub i32: TyId,
    pub i64: TyId,
    pub i128: TyId,
    pub u8: TyId,
    pub u16: TyId,
    pub u32: TyId,
    pub u64: TyId,
    pub u128: TyId,
    pub usize: TyId,
    pub isize: TyId,
    pub f32: TyId,
    pub f64: TyId,
}

impl Default for TyArena {
    fn default() -> Self {
        Self::new()
    }
}

impl TyArena {
    pub fn new() -> Self {
        let mut storage = Arena::<TyData>::default();
        let mut intern = HashMap::<TyData, TyId>::new();
        let mut alloc = |t: TyData| {
            if let Some(id) = intern.get(&t) {
                *id
            } else {
                let id = storage.alloc(t.clone());
                intern.insert(t, id);
                id
            }
        };
        let bool_ = alloc(TyData::Bool);
        let unit = alloc(TyData::Unit);
        let never = alloc(TyData::Never);
        let char_ = alloc(TyData::Char);
        let str_ = alloc(TyData::Str);
        let string = alloc(TyData::String);
        let bytes = alloc(TyData::Bytes);
        let error = alloc(TyData::Error);
        let duration = alloc(TyData::Duration);
        let size = alloc(TyData::Size);
        let int_infer = alloc(TyData::Int(IntKind::IntInfer));
        let float_infer = alloc(TyData::Float(FloatKind::FloatInfer));
        let i8 = alloc(TyData::Int(IntKind::I8));
        let i16 = alloc(TyData::Int(IntKind::I16));
        let i32 = alloc(TyData::Int(IntKind::I32));
        let i64 = alloc(TyData::Int(IntKind::I64));
        let i128 = alloc(TyData::Int(IntKind::I128));
        let u8 = alloc(TyData::Int(IntKind::U8));
        let u16 = alloc(TyData::Int(IntKind::U16));
        let u32 = alloc(TyData::Int(IntKind::U32));
        let u64 = alloc(TyData::Int(IntKind::U64));
        let u128 = alloc(TyData::Int(IntKind::U128));
        let usize = alloc(TyData::Int(IntKind::USize));
        let isize = alloc(TyData::Int(IntKind::ISize));
        let f32 = alloc(TyData::Float(FloatKind::F32));
        let f64 = alloc(TyData::Float(FloatKind::F64));
        Self {
            storage,
            intern,
            bool_,
            unit,
            never,
            char_,
            str_,
            string,
            bytes,
            error,
            duration,
            size,
            int_infer,
            float_infer,
            i8,
            i16,
            i32,
            i64,
            i128,
            u8,
            u16,
            u32,
            u64,
            u128,
            usize,
            isize,
            f32,
            f64,
        }
    }

    pub fn intern(&mut self, ty: TyData) -> TyId {
        if let Some(id) = self.intern.get(&ty) {
            return *id;
        }
        let id = self.storage.alloc(ty.clone());
        self.intern.insert(ty, id);
        id
    }

    pub fn get(&self, id: TyId) -> &TyData {
        &self.storage[id]
    }

    pub fn int(&mut self, k: IntKind) -> TyId {
        match k {
            IntKind::I8 => self.i8,
            IntKind::I16 => self.i16,
            IntKind::I32 => self.i32,
            IntKind::I64 => self.i64,
            IntKind::I128 => self.i128,
            IntKind::U8 => self.u8,
            IntKind::U16 => self.u16,
            IntKind::U32 => self.u32,
            IntKind::U64 => self.u64,
            IntKind::U128 => self.u128,
            IntKind::USize => self.usize,
            IntKind::ISize => self.isize,
            IntKind::IntInfer => self.int_infer,
        }
    }

    pub fn float(&mut self, k: FloatKind) -> TyId {
        match k {
            FloatKind::F32 => self.f32,
            FloatKind::F64 => self.f64,
            FloatKind::FloatInfer => self.float_infer,
        }
    }

    pub fn ref_to(&mut self, mutable: bool, inner: TyId) -> TyId {
        self.intern(TyData::Ref { mutable, inner })
    }

    pub fn tuple(&mut self, xs: Vec<TyId>) -> TyId {
        if xs.is_empty() {
            self.unit
        } else {
            self.intern(TyData::Tuple(xs))
        }
    }

    pub fn array(&mut self, elem: TyId, len: Option<u64>) -> TyId {
        self.intern(TyData::Array { elem, len })
    }

    pub fn adt(&mut self, id: AdtId, args: Vec<TyId>) -> TyId {
        self.intern(TyData::Adt(id, args))
    }

    pub fn fn_ty(&mut self, params: Vec<TyId>, ret: TyId, effects: Vec<EffectId>) -> TyId {
        self.intern(TyData::Fn {
            params,
            ret,
            effects,
        })
    }

    pub fn var(&mut self, v: TyVarId) -> TyId {
        self.intern(TyData::Var(v))
    }

    pub fn param(&mut self, p: ParamId) -> TyId {
        self.intern(TyData::Param(p))
    }

    pub fn raw_ptr(&mut self, inner: TyId) -> TyId {
        self.intern(TyData::RawPtr(inner))
    }

    pub fn module(&mut self, name: impl Into<String>) -> TyId {
        self.intern(TyData::Module(name.into()))
    }

    pub fn cap(&mut self, family: CapFamily, constraint: CapConstraint) -> TyId {
        self.intern(TyData::Cap { family, constraint })
    }

    pub fn dyn_trait(&mut self, trait_name: impl Into<String>) -> TyId {
        self.intern(TyData::Dyn {
            trait_name: trait_name.into(),
        })
    }
}

/// Render a type for diagnostics. Walks the substitution to dereference
/// inference variables.
pub fn pretty_ty(
    ty: TyId,
    arena: &TyArena,
    subst: Option<&crate::infer::Substitution>,
    defs: Option<&crate::defs::DefMap>,
) -> String {
    let ty = match subst {
        Some(s) => s.resolve_shallow(ty, arena),
        None => ty,
    };
    match arena.get(ty) {
        TyData::Bool => "Bool".into(),
        TyData::Int(k) => match k {
            IntKind::I8 => "I8",
            IntKind::I16 => "I16",
            IntKind::I32 => "I32",
            IntKind::I64 => "I64",
            IntKind::I128 => "I128",
            IntKind::U8 => "U8",
            IntKind::U16 => "U16",
            IntKind::U32 => "U32",
            IntKind::U64 => "U64",
            IntKind::U128 => "U128",
            IntKind::USize => "USize",
            IntKind::ISize => "ISize",
            IntKind::IntInfer => "{integer}",
        }
        .into(),
        TyData::Float(k) => match k {
            FloatKind::F32 => "F32",
            FloatKind::F64 => "F64",
            FloatKind::FloatInfer => "{float}",
        }
        .into(),
        TyData::Char => "Char".into(),
        TyData::Str => "Str".into(),
        TyData::String => "String".into(),
        TyData::Bytes => "Bytes".into(),
        TyData::Unit => "Unit".into(),
        TyData::Never => "Never".into(),
        TyData::Duration => "Duration".into(),
        TyData::Size => "Size".into(),
        TyData::Error => "{error}".into(),
        TyData::Var(v) => format!("?{}", v.0),
        TyData::Param(p) => {
            // Without scope info, just print as T<n>. The diag layer can
            // optionally render with the param name when it has scope.
            format!("T{}", p.0)
        }
        TyData::Tuple(xs) => {
            let parts: Vec<String> = xs
                .iter()
                .map(|t| pretty_ty(*t, arena, subst, defs))
                .collect();
            format!("({})", parts.join(", "))
        }
        TyData::Array { elem, len } => match len {
            Some(n) => format!("[{}; {}]", pretty_ty(*elem, arena, subst, defs), n),
            None => format!("[{}]", pretty_ty(*elem, arena, subst, defs)),
        },
        TyData::Ref { mutable, inner } => {
            let m = if *mutable { "mut " } else { "" };
            format!("&{}{}", m, pretty_ty(*inner, arena, subst, defs))
        }
        TyData::Fn {
            params,
            ret,
            effects: _,
        } => {
            let ps: Vec<String> = params
                .iter()
                .map(|t| pretty_ty(*t, arena, subst, defs))
                .collect();
            format!(
                "fn({}) -> {}",
                ps.join(", "),
                pretty_ty(*ret, arena, subst, defs)
            )
        }
        TyData::Adt(id, args) => {
            let name = defs
                .and_then(|d| d.adt(*id).map(|a| a.name.clone()))
                .unwrap_or_else(|| format!("Adt{}", id.0));
            if args.is_empty() {
                name
            } else {
                let parts: Vec<String> = args
                    .iter()
                    .map(|t| pretty_ty(*t, arena, subst, defs))
                    .collect();
                format!("{}[{}]", name, parts.join(", "))
            }
        }
        TyData::RawPtr(inner) => format!("*{}", pretty_ty(*inner, arena, subst, defs)),
        TyData::Module(name) => format!("module {}", name),
        TyData::Cap { family, constraint } => {
            let fname = match family {
                CapFamily::Net => "Net".to_string(),
                CapFamily::Fs => "Fs".to_string(),
                CapFamily::Clock => "Clock".to_string(),
                CapFamily::Dom => "Dom".to_string(),
                CapFamily::Model => "Model".to_string(),
                CapFamily::Custom(s) => s.clone(),
            };
            match constraint {
                CapConstraint::Any => fname,
                _ => format!("{}[{}]", fname, pretty_constraint(constraint)),
            }
        }
        TyData::Dyn { trait_name } => format!("dyn {}", trait_name),
    }
}

fn pretty_constraint(c: &CapConstraint) -> String {
    match c {
        CapConstraint::Any => "Any".into(),
        CapConstraint::ReadOnly => "ReadOnly".into(),
        CapConstraint::Path(p) => format!("Path({:?})", p),
        CapConstraint::Host(hs) => format!("Host({:?})", hs),
        CapConstraint::And(xs) => {
            let parts: Vec<String> = xs.iter().map(pretty_constraint).collect();
            format!("And({})", parts.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_collapses_identical() {
        let mut a = TyArena::new();
        let t1 = a.intern(TyData::Tuple(vec![a.bool_, a.i32]));
        let t2 = a.intern(TyData::Tuple(vec![a.bool_, a.i32]));
        assert_eq!(t1, t2);
    }

    #[test]
    fn primitives_are_distinct() {
        let a = TyArena::new();
        assert_ne!(a.bool_, a.unit);
        assert_ne!(a.i32, a.u32);
        assert_ne!(a.int_infer, a.i32);
    }

    #[test]
    fn pretty_basics() {
        let mut a = TyArena::new();
        assert_eq!(pretty_ty(a.bool_, &a, None, None), "Bool");
        assert_eq!(pretty_ty(a.i32, &a, None, None), "I32");
        let r = a.ref_to(false, a.str_);
        assert_eq!(pretty_ty(r, &a, None, None), "&Str");
        let arr = a.array(a.i32, None);
        assert_eq!(pretty_ty(arr, &a, None, None), "[I32]");
    }
}
