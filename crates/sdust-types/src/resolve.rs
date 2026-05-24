//! Name resolution: build a `DefMap` from a `Package`, then resolve
//! `HirType` to `TyId` and value paths to `DefRef`/local.

use crate::defs::*;
use crate::diag;
use crate::prelude::{build_prelude, PreludeIds};
use crate::ty::*;
use sdust_diagnostics::Diagnostic;
use sdust_hir::{Item as HirItem, *};

/// Scope of generic parameters in flight (e.g. while resolving a fn signature).
#[derive(Default)]
pub struct ParamScope {
    pub params: Vec<(String, ParamId)>,
}

impl ParamScope {
    pub fn lookup(&self, name: &str) -> Option<ParamId> {
        self.params
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    pub fn push(&mut self, name: impl Into<String>, id: ParamId) {
        self.params.push((name.into(), id));
    }
}

pub struct ResolveOutput {
    pub defs: DefMap,
    pub prelude: PreludeIds,
    pub diagnostics: Vec<Diagnostic>,
}

/// Two-pass package resolution. Returns a fully populated DefMap with the
/// prelude merged in and user items registered.
pub fn build_def_map(pkg: &Package, arena: &mut TyArena) -> ResolveOutput {
    let mut defs = DefMap::default();
    let prelude = build_prelude(arena, &mut defs);
    let mut diagnostics: Vec<Diagnostic> = vec![];

    // Pass 1: declare all top-level ADTs and fns (with placeholder types).
    // We also assign FnDefId for agent methods + impl methods so call-site
    // resolution can find them later.
    let mut struct_ids: Vec<(StructId, AdtId)> = vec![];
    let mut enum_ids: Vec<(EnumId, AdtId)> = vec![];
    let mut fn_ids: Vec<(FnId, FnDefId)> = vec![];
    let mut type_alias_ids: Vec<(TypeAliasId, AdtId)> = vec![];

    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        declare_item(
            item,
            pkg,
            &mut defs,
            arena,
            &mut struct_ids,
            &mut enum_ids,
            &mut fn_ids,
            &mut type_alias_ids,
        );
    }

    // Pass 2: fill in fields, variants, fn signatures.
    for (sid, aid) in struct_ids.iter().copied() {
        let hs = &pkg.structs[sid];
        let mut param_scope = ParamScope::default();
        let mut generics_def = vec![];
        let mut param_ids = vec![];
        for g in &hs.generics {
            let pid = defs.alloc_param(ParamDef {
                name: g.clone(),
                bounds: vec![],
            });
            param_scope.push(g.clone(), pid);
            param_ids.push(pid);
            generics_def.push(ParamDef {
                name: g.clone(),
                bounds: vec![],
            });
        }
        let fields: Vec<FieldDef> = hs
            .fields
            .iter()
            .map(|f| FieldDef {
                name: Some(f.name.clone()),
                ty: resolve_hir_type(f.ty, pkg, &defs, arena, &param_scope, &mut diagnostics),
            })
            .collect();
        if let Some(adt) = defs.adt_mut(aid) {
            adt.generics = generics_def;
            adt.param_ids = param_ids;
            adt.variants = vec![VariantDef {
                name: hs.name.clone(),
                fields,
            }];
        }
    }

    for (eid, aid) in enum_ids.iter().copied() {
        let he = &pkg.enums[eid];
        let mut param_scope = ParamScope::default();
        let mut generics_def = vec![];
        let mut param_ids = vec![];
        for g in &he.generics {
            let pid = defs.alloc_param(ParamDef {
                name: g.clone(),
                bounds: vec![],
            });
            param_scope.push(g.clone(), pid);
            param_ids.push(pid);
            generics_def.push(ParamDef {
                name: g.clone(),
                bounds: vec![],
            });
        }
        let variants: Vec<VariantDef> = he
            .variants
            .iter()
            .map(|v| VariantDef {
                name: v.name.clone(),
                fields: v
                    .payload
                    .iter()
                    .map(|t| FieldDef {
                        name: None,
                        ty: resolve_hir_type(*t, pkg, &defs, arena, &param_scope, &mut diagnostics),
                    })
                    .collect(),
            })
            .collect();
        if let Some(adt) = defs.adt_mut(aid) {
            adt.generics = generics_def;
            adt.param_ids = param_ids;
            adt.variants = variants;
        }
        // Register each variant by short name.
        let vlen = variants_len(&defs, aid);
        for i in 0..vlen {
            let vname = defs.adt(aid).unwrap().variants[i].name.clone();
            defs.by_name
                .entry(vname.clone())
                .or_insert(DefRef::Variant(aid, i));
        }
    }

    for (tid, aid) in type_alias_ids.iter().copied() {
        let ta = &pkg.type_aliases[tid];
        let mut param_scope = ParamScope::default();
        let mut generics_def = vec![];
        let mut param_ids = vec![];
        for g in &ta.generics {
            let pid = defs.alloc_param(ParamDef {
                name: g.clone(),
                bounds: vec![],
            });
            param_scope.push(g.clone(), pid);
            param_ids.push(pid);
            generics_def.push(ParamDef {
                name: g.clone(),
                bounds: vec![],
            });
        }
        let aliased = resolve_hir_type(ta.ty, pkg, &defs, arena, &param_scope, &mut diagnostics);
        if let Some(adt) = defs.adt_mut(aid) {
            adt.generics = generics_def;
            adt.param_ids = param_ids;
            // Store aliased type as a single-field opaque variant.
            adt.variants = vec![VariantDef {
                name: ta.name.clone(),
                fields: vec![FieldDef {
                    name: None,
                    ty: aliased,
                }],
            }];
        }
    }

    for (fid, fdef_id) in fn_ids.iter().copied() {
        let hf = &pkg.fns[fid];
        let mut param_scope = ParamScope::default();
        let mut generics_def = vec![];
        let mut param_ids = vec![];
        for g in &hf.generics {
            let pid = defs.alloc_param(ParamDef {
                name: g.clone(),
                bounds: vec![],
            });
            param_scope.push(g.clone(), pid);
            param_ids.push(pid);
            generics_def.push(ParamDef {
                name: g.clone(),
                bounds: vec![],
            });
        }
        let params: Vec<(String, TyId)> = hf
            .params
            .iter()
            .map(|p| {
                let ty = match p.ty {
                    Some(t) => {
                        resolve_hir_type(t, pkg, &defs, arena, &param_scope, &mut diagnostics)
                    }
                    None => {
                        // Untyped param — synthetic Var slot. Inference will pin it.
                        arena.error
                    }
                };
                (p.name.clone(), ty)
            })
            .collect();
        let ret = match hf.ret {
            Some(t) => resolve_hir_type(t, pkg, &defs, arena, &param_scope, &mut diagnostics),
            None => arena.unit,
        };
        let effects: Vec<EffectId> = hf
            .effects
            .iter()
            .map(|n| defs.intern_effect(n.clone()))
            .collect();
        if let Some(f) = defs.fn_def_mut(fdef_id) {
            f.generics = generics_def;
            f.param_ids = param_ids;
            f.params = params;
            f.ret = ret;
            f.effects = effects;
        }
    }

    // Agents: declare and register methods.
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        if let HirItem::Agent(aid) = item {
            let hir_agent = &pkg.agents[*aid];
            let adt_id = defs.alloc_adt(AdtDef {
                name: hir_agent.name.clone(),
                kind: AdtKind::Opaque,
                generics: vec![],
                param_ids: vec![],
                variants: vec![],
            });
            defs.by_name
                .entry(hir_agent.name.clone())
                .or_insert(DefRef::Adt(adt_id));
            // Register methods as fns (with self-receiver elided).
            for mfid in &hir_agent.methods {
                let hf = &pkg.fns[*mfid];
                let fdef = FnDef {
                    name: hf.name.clone(),
                    generics: vec![],
                    param_ids: vec![],
                    params: vec![],
                    ret: arena.unit,
                    effects: vec![],
                    is_pub: hf.is_pub,
                    body: hf.body,
                    hir_fn: Some(*mfid),
                };
                let id = defs.alloc_fn(fdef);
                defs.hir_fn_to_def.insert(*mfid, id);
            }
        }
    }

    // Map struct_ids / enum_ids into the def map's HIR lookup tables.
    for (sid, aid) in struct_ids {
        defs.hir_struct_to_adt.insert(sid, aid);
    }
    for (eid, aid) in enum_ids {
        defs.hir_enum_to_adt.insert(eid, aid);
    }
    for (fid, fdef_id) in fn_ids {
        defs.hir_fn_to_def.insert(fid, fdef_id);
    }

    ResolveOutput {
        defs,
        prelude,
        diagnostics,
    }
}

fn variants_len(defs: &DefMap, aid: AdtId) -> usize {
    defs.adt(aid).map(|a| a.variants.len()).unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn declare_item(
    item: &HirItem,
    pkg: &Package,
    defs: &mut DefMap,
    arena: &mut TyArena,
    struct_ids: &mut Vec<(StructId, AdtId)>,
    enum_ids: &mut Vec<(EnumId, AdtId)>,
    fn_ids: &mut Vec<(FnId, FnDefId)>,
    type_alias_ids: &mut Vec<(TypeAliasId, AdtId)>,
) {
    match item {
        HirItem::Struct(sid) => {
            let hs = &pkg.structs[*sid];
            let aid = defs.alloc_adt(AdtDef {
                name: hs.name.clone(),
                kind: AdtKind::Struct,
                generics: vec![],
                param_ids: vec![],
                variants: vec![],
            });
            // User types shadow prelude opaques.
            defs.by_name.insert(hs.name.clone(), DefRef::Adt(aid));
            struct_ids.push((*sid, aid));
        }
        HirItem::Enum(eid) => {
            let he = &pkg.enums[*eid];
            let aid = defs.alloc_adt(AdtDef {
                name: he.name.clone(),
                kind: AdtKind::Enum,
                generics: vec![],
                param_ids: vec![],
                variants: vec![],
            });
            defs.by_name.insert(he.name.clone(), DefRef::Adt(aid));
            enum_ids.push((*eid, aid));
        }
        HirItem::TypeAlias(tid) => {
            let ta = &pkg.type_aliases[*tid];
            let aid = defs.alloc_adt(AdtDef {
                name: ta.name.clone(),
                kind: AdtKind::Opaque,
                generics: vec![],
                param_ids: vec![],
                variants: vec![],
            });
            defs.by_name.insert(ta.name.clone(), DefRef::Adt(aid));
            type_alias_ids.push((*tid, aid));
        }
        HirItem::Fn(fid) => {
            let hf = &pkg.fns[*fid];
            let fdef_id = defs.alloc_fn(FnDef {
                name: hf.name.clone(),
                generics: vec![],
                param_ids: vec![],
                params: vec![],
                ret: arena.unit,
                effects: vec![],
                is_pub: hf.is_pub,
                body: hf.body,
                hir_fn: Some(*fid),
            });
            defs.by_name.insert(hf.name.clone(), DefRef::Fn(fdef_id));
            fn_ids.push((*fid, fdef_id));
        }
        HirItem::ExternBlock(eb) => {
            for fid in &eb.fns {
                let hf = &pkg.fns[*fid];
                let fdef_id = defs.alloc_fn(FnDef {
                    name: hf.name.clone(),
                    generics: vec![],
                    param_ids: vec![],
                    params: vec![],
                    ret: arena.unit,
                    effects: vec![],
                    is_pub: true,
                    body: None,
                    hir_fn: Some(*fid),
                });
                defs.by_name
                    .entry(hf.name.clone())
                    .or_insert(DefRef::Fn(fdef_id));
                fn_ids.push((*fid, fdef_id));
            }
        }
        HirItem::Protocol(_)
        | HirItem::Supervisor(_)
        | HirItem::Use(_)
        | HirItem::Mod(_)
        | HirItem::ExportDecl(_)
        | HirItem::Macro(_)
        | HirItem::Impl(_)
        | HirItem::Trait(_)
        | HirItem::Const(_)
        | HirItem::Agent(_) => {
            // Agents handled separately.
        }
    }
}

/// Resolve an `HirType` to a `TyId`. Unknown identifiers emit `SD2002` and
/// return `Ty::Error`.
pub fn resolve_hir_type(
    ty_id: TypeId,
    pkg: &Package,
    defs: &DefMap,
    arena: &mut TyArena,
    scope: &ParamScope,
    diag_out: &mut Vec<Diagnostic>,
) -> TyId {
    let ty = &pkg.types[ty_id];
    match ty {
        HirType::Path { segments, generics } => {
            // Single-segment first: check generic params, then defs.
            if segments.len() == 1 {
                let name = &segments[0];
                if let Some(pid) = scope.lookup(name) {
                    if !generics.is_empty() {
                        // Generic params don't take args.
                        diag_out.push(diag::wrong_generic_arity(
                            0,
                            generics.len(),
                            &SourceSpan { start: 0, end: 0 },
                            name,
                        ));
                    }
                    return arena.param(pid);
                }
                if let Some(d) = defs.lookup(name) {
                    return resolve_def_to_ty(d, generics, pkg, defs, arena, scope, diag_out, name);
                }
                diag_out.push(diag::unresolved_type(
                    name,
                    &SourceSpan { start: 0, end: 0 },
                ));
                return arena.error;
            }
            // Multi-segment.
            if let Some(d) = defs.lookup_path(segments) {
                return resolve_def_to_ty(
                    d,
                    generics,
                    pkg,
                    defs,
                    arena,
                    scope,
                    diag_out,
                    &segments.join("."),
                );
            }
            // Fallback: silently opaque module-member.
            arena.error
        }
        HirType::Borrow { mutable, inner } => {
            let inner_ty = resolve_hir_type(*inner, pkg, defs, arena, scope, diag_out);
            arena.ref_to(*mutable, inner_ty)
        }
        HirType::Tuple(xs) => {
            let resolved: Vec<TyId> = xs
                .iter()
                .map(|t| resolve_hir_type(*t, pkg, defs, arena, scope, diag_out))
                .collect();
            arena.tuple(resolved)
        }
        HirType::Array { elem, len } => {
            let e = resolve_hir_type(*elem, pkg, defs, arena, scope, diag_out);
            let n = len.and_then(|lid| const_eval_len(&pkg.exprs[lid]));
            arena.array(e, n)
        }
        HirType::Fn { params, ret } => {
            let p: Vec<TyId> = params
                .iter()
                .map(|t| resolve_hir_type(*t, pkg, defs, arena, scope, diag_out))
                .collect();
            let r = match ret {
                Some(rt) => resolve_hir_type(*rt, pkg, defs, arena, scope, diag_out),
                None => arena.unit,
            };
            arena.fn_ty(p, r, vec![])
        }
        HirType::Result { ok, err } => {
            // Find Result adt id from defs.
            let result_id = match defs.lookup("Result") {
                Some(DefRef::Adt(id)) => id,
                _ => return arena.error,
            };
            let o = resolve_hir_type(*ok, pkg, defs, arena, scope, diag_out);
            let e = resolve_hir_type(*err, pkg, defs, arena, scope, diag_out);
            arena.adt(result_id, vec![o, e])
        }
        HirType::Union(_) => {
            // T!{A,B} — slice 3 doesn't model anonymous error unions
            // (post-v0.1 feature). Resolve as a single `Ty::Error` so
            // unification with any concrete error type is permissive.
            arena.error
        }
        HirType::Unit => arena.unit,
        HirType::Unknown => arena.error,
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_def_to_ty(
    d: DefRef,
    generics: &[TypeId],
    pkg: &Package,
    defs: &DefMap,
    arena: &mut TyArena,
    scope: &ParamScope,
    diag_out: &mut Vec<Diagnostic>,
    name: &str,
) -> TyId {
    match d {
        DefRef::Adt(aid) => {
            let expected = defs.adt(aid).map(|a| a.generics.len()).unwrap_or(0);
            if !generics.is_empty() && generics.len() != expected {
                diag_out.push(diag::wrong_generic_arity(
                    expected,
                    generics.len(),
                    &SourceSpan { start: 0, end: 0 },
                    name,
                ));
            }
            // If user passed N generics, resolve them; else fresh Param refs are
            // wrong — we want zero args when zero declared, and fresh Vars when
            // an arity is declared but no args given (let inference figure it out).
            let args: Vec<TyId> = if generics.is_empty() && expected > 0 {
                // No args given but ADT is generic: emit error-shape (we use
                // Error sentinel here; inference will not know what to do).
                // Slice 3 keeps this permissive — substitute `Error` for unknown.
                (0..expected).map(|_| arena.error).collect()
            } else if generics.is_empty() {
                vec![]
            } else {
                generics
                    .iter()
                    .map(|g| resolve_hir_type(*g, pkg, defs, arena, scope, diag_out))
                    .collect()
            };
            // Type-alias expansion: if the ADT is a single-variant opaque whose
            // single field is its aliased type, return the aliased type directly.
            // (Slice 3 ergonomics: lets `type UserId = U64` resolve to U64.)
            if let Some(adt) = defs.adt(aid) {
                if adt.kind == AdtKind::Opaque
                    && adt.variants.len() == 1
                    && adt.variants[0].fields.len() == 1
                    && adt.variants[0].fields[0].name.is_none()
                {
                    let aliased = adt.variants[0].fields[0].ty;
                    // Only return aliased if the ADT has no type params.
                    if adt.generics.is_empty() {
                        return aliased;
                    }
                }
            }
            arena.adt(aid, args)
        }
        DefRef::Module(_) => arena.error,
        DefRef::Param(p) => arena.param(p),
        DefRef::Variant(_, _) | DefRef::Fn(_) => {
            // Used as a type — not valid. Emit unresolved.
            diag_out.push(diag::unresolved_type(
                name,
                &SourceSpan { start: 0, end: 0 },
            ));
            arena.error
        }
    }
}

/// Const-eval an array-length expression. Only integer literals supported.
fn const_eval_len(e: &HirExpr) -> Option<u64> {
    match e {
        HirExpr::Literal(HirLiteral::Int(v, _)) if *v >= 0 => Some(*v as u64),
        _ => None,
    }
}
