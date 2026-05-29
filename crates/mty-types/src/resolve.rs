//! Name resolution: build a `DefMap` from a `Package`, then resolve
//! `HirType` to `TyId` and value paths to `DefRef`/local.

use crate::defs::*;
use crate::diag;
use crate::prelude::{build_prelude, PreludeIds};
use crate::ty::*;
use mty_diagnostics::Diagnostic;
use mty_hir::{Item as HirItem, *};

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

    // Slice 5: register trait method signatures EARLY (before fn-sig
    // resolution) so `dyn Trait` resolution inside fn signatures sees
    // the trait_methods table.
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        if let HirItem::Trait(t) = item {
            let mut sigs: Vec<TraitMethodSig> = vec![];
            for mfid in &t.methods {
                let hf = &pkg.fns[*mfid];
                let has_self_ty =
                    hf.params.iter().any(|p| match p.ty {
                        Some(t) => is_self_type(&pkg.types[t]),
                        None => false,
                    }) || hf.ret.map(|t| is_self_type(&pkg.types[t])).unwrap_or(false);
                sigs.push(TraitMethodSig {
                    name: hf.name.clone(),
                    has_self_ty,
                    has_generics: !hf.generics.is_empty(),
                });
            }
            defs.traits.trait_methods.insert(t.name.clone(), sigs);
        }
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

    // Agents: register methods. The ADT itself was already declared in
    // pass 1 (declare_item) so fn signatures resolve `AgentRef[Foo]`.
    let mut agent_method_ids: Vec<(AdtId, mty_hir::FnId, FnDefId)> = vec![];
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        if let HirItem::Agent(aid) = item {
            let hir_agent = &pkg.agents[*aid];
            let adt_id = match defs.lookup(&hir_agent.name) {
                Some(DefRef::Adt(id)) => id,
                _ => defs.alloc_adt(AdtDef {
                    name: hir_agent.name.clone(),
                    kind: AdtKind::Opaque,
                    generics: vec![],
                    param_ids: vec![],
                    variants: vec![],
                }),
            };
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
                    extern_abi: None,
                };
                let id = defs.alloc_fn(fdef);
                defs.hir_fn_to_def.insert(*mfid, id);
                agent_method_ids.push((adt_id, *mfid, id));
                defs.impl_methods.insert((adt_id, hf.name.clone()), id);
            }
        }
    }

    // Slice 5: process #[derive(...)] on structs / enums.
    apply_derives(
        &struct_ids,
        &enum_ids,
        pkg,
        &mut defs,
        arena,
        &mut diagnostics,
    );

    // Impl-block method indexing (slice 4, Task 11). For each `impl T { fn m() ... }`
    // (and `impl Trait for T { fn m() ... }`), register each method's FnDef so
    // method dispatch on user `Adt(T, _)` receivers can find it.
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        if let HirItem::Impl(impl_block) = item {
            // Resolve self_ty to an AdtId.
            let self_adt = match &pkg.types[impl_block.self_ty] {
                mty_hir::HirType::Path { segments, .. } if segments.len() == 1 => {
                    match defs.lookup(&segments[0]) {
                        Some(DefRef::Adt(aid)) => Some(aid),
                        _ => None,
                    }
                }
                _ => None,
            };
            // Slice 5: resolve trait_for to a trait name (single ident).
            let trait_name: Option<String> =
                impl_block.trait_for.and_then(|tid| match &pkg.types[tid] {
                    mty_hir::HirType::Path { segments, .. } if segments.len() == 1 => {
                        Some(segments[0].clone())
                    }
                    _ => None,
                });
            // Coherence: emit MT4022 if the (trait, self_adt) pair already
            // exists. Slice 5 detection is name-only.
            if let (Some(t), Some(sa)) = (trait_name.as_ref(), self_adt) {
                if defs.traits.impl_keys.contains(&(t.clone(), sa)) {
                    let sname = defs
                        .adt(sa)
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| format!("Adt{}", sa.0));
                    diagnostics.push(crate::diag::trait_coherence_violation(
                        t,
                        &sname,
                        &impl_block.span,
                    ));
                }
            }
            let mut method_fns: std::collections::HashMap<String, FnDefId> =
                std::collections::HashMap::new();
            for mfid in &impl_block.methods {
                let hf = &pkg.fns[*mfid];
                // Build a FnDef with the resolved signature.
                let mut param_scope = ParamScope::default();
                let mut generics_def = vec![];
                let mut method_param_ids = vec![];
                for g in &hf.generics {
                    let pid = defs.alloc_param(ParamDef {
                        name: g.clone(),
                        bounds: vec![],
                    });
                    param_scope.push(g.clone(), pid);
                    method_param_ids.push(pid);
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
                            Some(t) => resolve_hir_type(
                                t,
                                pkg,
                                &defs,
                                arena,
                                &param_scope,
                                &mut diagnostics,
                            ),
                            None => arena.error,
                        };
                        (p.name.clone(), ty)
                    })
                    .collect();
                let ret = match hf.ret {
                    Some(t) => {
                        resolve_hir_type(t, pkg, &defs, arena, &param_scope, &mut diagnostics)
                    }
                    None => arena.unit,
                };
                let fdef_id = defs.alloc_fn(FnDef {
                    name: hf.name.clone(),
                    generics: generics_def,
                    param_ids: method_param_ids,
                    params,
                    ret,
                    effects: vec![],
                    is_pub: hf.is_pub,
                    body: hf.body,
                    hir_fn: Some(*mfid),
                    extern_abi: None,
                });
                defs.hir_fn_to_def.insert(*mfid, fdef_id);
                if let Some(aid) = self_adt {
                    // Trait impls go into the trait dispatch table; only
                    // inherent impls win the impl_methods slot.
                    if trait_name.is_none() {
                        defs.impl_methods.insert((aid, hf.name.clone()), fdef_id);
                    }
                }
                method_fns.insert(hf.name.clone(), fdef_id);
            }
            // Register trait impl in the coherence/dispatch table.
            if let (Some(t), Some(sa)) = (trait_name, self_adt) {
                defs.traits.impl_keys.insert((t.clone(), sa));
                defs.traits.impls.push(TraitImpl {
                    trait_name: t.clone(),
                    self_adt: sa,
                    method_fns: method_fns.clone(),
                    span: impl_block.span.clone(),
                });
                for (mname, fid) in &method_fns {
                    defs.traits
                        .by_method
                        .entry((sa, mname.clone()))
                        .or_default()
                        .push((t.clone(), *fid));
                }
            }
        }
    }

    // Protocol message index (slice 4, Task 12). For each protocol's
    // declared message, store the parameter types so agent handlers can
    // look them up.
    //
    // v0.29 Track C: also store the resolved reply type when the message
    // declares `-> ReturnTy`. Used by the bang-send / ask type checker
    // so `agent ! Review(s)` lowers to its declared `Str` result instead
    // of the v0.28 stand-in Unit.
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        if let HirItem::Protocol(pid) = item {
            let proto = &pkg.protocols[*pid];
            for msg in &proto.messages {
                let ptys: Vec<TyId> = msg
                    .params
                    .iter()
                    .map(|p| match p.ty {
                        Some(t) => resolve_hir_type(
                            t,
                            pkg,
                            &defs,
                            arena,
                            &ParamScope::default(),
                            &mut diagnostics,
                        ),
                        None => arena.error,
                    })
                    .collect();
                defs.protocol_msgs
                    .insert((proto.name.clone(), msg.name.clone()), ptys);
                // v0.29 Track C: resolve the optional reply type and stash
                // it. Missing annotation → no entry, which the check site
                // treats as Unit (matching the surface-level default).
                if let Some(reply_tid) = msg.reply {
                    let reply_ty = resolve_hir_type(
                        reply_tid,
                        pkg,
                        &defs,
                        arena,
                        &ParamScope::default(),
                        &mut diagnostics,
                    );
                    defs.protocol_msg_reply
                        .insert((proto.name.clone(), msg.name.clone()), reply_ty);
                }
            }
            // Slice 5: also save the message-name list per protocol.
            let names: Vec<String> = proto.messages.iter().map(|m| m.name.clone()).collect();
            defs.protocol_msg_names.insert(proto.name.clone(), names);
        }
    }

    // v0.29 Track C: agent → protocol-name list. The agent's declared
    // protocols come from `agent Foo: A + B { ... }`; we collect the
    // single-segment path name from each TypeId so the bang-send check
    // site can drill from `AgentRef[Foo]` → "Foo" → protocols → reply.
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        if let HirItem::Agent(aid) = item {
            let hir_agent = &pkg.agents[*aid];
            let adt_id = match defs.lookup(&hir_agent.name) {
                Some(DefRef::Adt(id)) => Some(id),
                _ => None,
            };
            if let Some(adt_id) = adt_id {
                let mut proto_names: Vec<String> = Vec::new();
                for ptid in &hir_agent.protocols {
                    let ty = &pkg.types[*ptid];
                    collect_protocol_names_into(ty, &mut proto_names);
                }
                if !proto_names.is_empty() {
                    defs.agent_protocols.insert(adt_id, proto_names);
                }
            }
        }
    }
    let _ = agent_method_ids;

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

/// v0.29 Track C: walk a `HirType` and collect any single-segment path
/// names (the "protocol name" position used by `agent Foo: A + B`).
/// Composition `protocol Web = A + B` lands here as repeated entries —
/// duplicates are fine, callers iterate the full list.
fn collect_protocol_names_into(ty: &HirType, out: &mut Vec<String>) {
    // `A + B` composition isn't a distinct HirType variant in the
    // current AST — composition is unrolled at parse time into
    // multiple protocol TypeIds — so the single-segment path is the
    // only shape we need to cover here.
    if let HirType::Path { segments, .. } = ty {
        if let Some(last) = segments.last() {
            out.push(last.clone());
        }
    }
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
                extern_abi: None,
            });
            defs.by_name.insert(hf.name.clone(), DefRef::Fn(fdef_id));
            fn_ids.push((*fid, fdef_id));
        }
        HirItem::ExternBlock(eb) => {
            let abi = eb.abi.clone();
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
                    // v0.37 Track T3 — mark this fn as belonging to an
                    // extern <abi> { ... } block. The call-site checker
                    // gates FFI coercions (Str → *U8, &x for *U8 out-params,
                    // struct literals as args) on `extern_abi == Some("c")`.
                    extern_abi: Some(abi.clone().unwrap_or_else(|| "c".to_string())),
                });
                defs.by_name
                    .entry(hf.name.clone())
                    .or_insert(DefRef::Fn(fdef_id));
                fn_ids.push((*fid, fdef_id));
            }
        }
        HirItem::Agent(aid) => {
            // Declare the agent's type-namespace ADT in pass 1 so fn
            // signatures (e.g. `r: AgentRef[Foo]`) can resolve it.
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
        }
        HirItem::Use(u) => {
            // `use std.http` binds the last path segment (`http`) as a
            // module-reference value in scope. This makes single-segment
            // references like `http.serve(...)` resolve (the leading
            // `http` is a Module, slice-4 synth_path then makes the
            // chain opaque-permissive).
            if let Some(last) = u.path.last() {
                let dotted = u.path.join(".");
                // If we have a module by full dotted path, use it.
                if let Some(DefRef::Module(mid)) = defs.lookup(&dotted) {
                    defs.by_name
                        .entry(last.clone())
                        .or_insert(DefRef::Module(mid));
                } else {
                    // Else allocate an opaque module of the dotted name.
                    let mid = defs.alloc_module(dotted.clone());
                    defs.by_name
                        .entry(last.clone())
                        .or_insert(DefRef::Module(mid));
                    defs.by_name.entry(dotted).or_insert(DefRef::Module(mid));
                }
            }
            // Plus any leaf aliases (`use std.json::Json`-style; the
            // parser may not produce these yet for slice 4, so we leave
            // the leaf path for slice 5+).
        }
        HirItem::Protocol(_)
        | HirItem::Supervisor(_)
        | HirItem::Mod(_)
        | HirItem::ExportDecl(_)
        | HirItem::Macro(_)
        | HirItem::Impl(_)
        | HirItem::Trait(_)
        | HirItem::Const(_)
        | HirItem::Sandbox(_) => {
            // Handled separately or unsupported in slice 4.
        }
    }
}

/// Resolve an `HirType` to a `TyId`. Unknown identifiers emit `MT2002` and
/// return `Ty::Error`.
#[allow(clippy::too_many_arguments)]
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
            let Some(DefRef::Adt(result_id)) = defs.lookup("Result") else {
                return arena.error;
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
        HirType::Dyn { trait_name } => {
            // Slice-5 conservative object-safety: a `dyn T` is acceptable
            // only when T is a known trait whose methods have no `Self`
            // mention and no method-level generics. If T isn't declared
            // (slice-5 trait_methods table empty), we still produce the
            // dyn type — the coercion-site check later catches the
            // "no impl" condition.
            if let Some(sigs) = defs.traits.trait_methods.get(trait_name) {
                let unsafe_obj = sigs.iter().any(|s| s.has_self_ty || s.has_generics);
                if unsafe_obj {
                    diag_out.push(crate::diag::dyn_requires_object_safe(
                        trait_name,
                        &SourceSpan { start: 0, end: 0 },
                    ));
                }
            }
            arena.dyn_trait(trait_name)
        }
        HirType::Unit => arena.unit,
        HirType::Unknown => arena.error,
    }
}

/// Slice-5: map a top-level identifier to a `Cap` family if it is one of
/// the core capability names (`Net`, `Fs`, `Clock`, `Dom`, `Model`).
pub(crate) fn cap_family_for_name(name: &str) -> Option<crate::ty::CapFamily> {
    use crate::ty::CapFamily;
    Some(match name {
        "Net" => CapFamily::Net,
        "Fs" => CapFamily::Fs,
        "Clock" => CapFamily::Clock,
        "Dom" => CapFamily::Dom,
        "Model" => CapFamily::Model,
        _ => return None,
    })
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
    // Slice 5: core capability names resolve to TyData::Cap, not Adt.
    if let Some(fam) = cap_family_for_name(name) {
        if generics.is_empty() {
            return arena.cap(fam, crate::ty::CapConstraint::Any);
        }
    }
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
                    .map(|g| {
                        // v0.14 (Gap B / MT2023 emit-site): pre-flight check
                        // for value-name in type-arg position. If the arg is
                        // a single-segment path that resolves to a value-kind
                        // def (Fn / Variant), surface MT2023 here; pre-v0.14
                        // this funnelled through MT2002 which mis-named the
                        // failure as "unresolved type". The actual resolve
                        // below still runs and returns `arena.error`, so the
                        // rest of the body still type-checks.
                        if let mty_hir::HirType::Path { segments, .. } = &pkg.types[*g] {
                            if segments.len() == 1 {
                                let arg_name = &segments[0];
                                if let Some(d) = defs.lookup(arg_name) {
                                    let arg_kind = match d {
                                        DefRef::Fn(_) => Some("function"),
                                        DefRef::Variant(_, _) => Some("variant constructor"),
                                        _ => None,
                                    };
                                    if let Some(kind) = arg_kind {
                                        diag_out.push(diag::generic_arg_kind_mismatch(
                                            name,
                                            arg_name,
                                            kind,
                                            &SourceSpan { start: 0, end: 0 },
                                        ));
                                    }
                                }
                            }
                        }
                        resolve_hir_type(*g, pkg, defs, arena, scope, diag_out)
                    })
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

/// Slice 5: walk derives on each struct/enum, validate them, and
/// register the resulting Copy-set / synthetic trait impls.
fn apply_derives(
    struct_ids: &[(StructId, AdtId)],
    enum_ids: &[(EnumId, AdtId)],
    pkg: &Package,
    defs: &mut DefMap,
    arena: &mut TyArena,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let _ = arena;
    // Helper closures.
    fn validate_copy_struct(
        aid: AdtId,
        name: &str,
        defs: &DefMap,
        arena: &TyArena,
        out_diags: &mut Vec<Diagnostic>,
        span: &mty_hir::SourceSpan,
    ) -> bool {
        let Some(adt) = defs.adt(aid) else {
            return false;
        };
        for v in &adt.variants {
            for f in &v.fields {
                if !crate::is_field_copy(f.ty, arena, defs) {
                    let fname = f.name.clone().unwrap_or_else(|| "<unnamed>".into());
                    out_diags.push(crate::diag::derive_copy_field_not_copy(name, &fname, span));
                    return false;
                }
            }
        }
        true
    }

    for (sid, aid) in struct_ids {
        let hs = &pkg.structs[*sid];
        for d in &hs.derives {
            match d.as_str() {
                "Copy" => {
                    if validate_copy_struct(*aid, &hs.name, defs, arena, diagnostics, &hs.span) {
                        defs.user_copy.insert(*aid);
                    }
                }
                "Sendable" => {
                    // v0.3 (A65): opt the struct into the Sendable
                    // marker-trait set. Validation happens at use sites
                    // (the field-shape check is performed lazily by
                    // `crate::sendable::sendable_reason`).
                    defs.user_sendable.insert(*aid);
                    register_synthetic_trait_impl(defs, d, *aid, &hs.span);
                }
                "Hash" | "Eq" => {
                    register_synthetic_trait_impl(defs, d, *aid, &hs.span);
                }
                other => {
                    diagnostics.push(crate::diag::derive_unknown(other, &hs.span));
                }
            }
        }
    }
    for (eid, aid) in enum_ids {
        let he = &pkg.enums[*eid];
        for d in &he.derives {
            match d.as_str() {
                "Copy" => {
                    if validate_copy_struct(*aid, &he.name, defs, arena, diagnostics, &he.span) {
                        defs.user_copy.insert(*aid);
                    }
                }
                "Sendable" => {
                    defs.user_sendable.insert(*aid);
                    register_synthetic_trait_impl(defs, d, *aid, &he.span);
                }
                "Hash" | "Eq" => {
                    register_synthetic_trait_impl(defs, d, *aid, &he.span);
                }
                other => {
                    diagnostics.push(crate::diag::derive_unknown(other, &he.span));
                }
            }
        }
    }
}

fn register_synthetic_trait_impl(
    defs: &mut DefMap,
    trait_name: &str,
    self_adt: AdtId,
    span: &mty_hir::SourceSpan,
) {
    if defs
        .traits
        .impl_keys
        .contains(&(trait_name.to_string(), self_adt))
    {
        return;
    }
    defs.traits
        .impl_keys
        .insert((trait_name.to_string(), self_adt));
    defs.traits.impls.push(crate::TraitImpl {
        trait_name: trait_name.to_string(),
        self_adt,
        method_fns: Default::default(),
        span: span.clone(),
    });
    // Ensure trait_methods has an entry (so dyn resolution accepts it).
    defs.traits
        .trait_methods
        .entry(trait_name.to_string())
        .or_insert_with(|| match trait_name {
            "Hash" => vec![crate::TraitMethodSig {
                name: "hash".into(),
                has_self_ty: false,
                has_generics: false,
            }],
            "Eq" => vec![crate::TraitMethodSig {
                name: "eq".into(),
                has_self_ty: true,
                has_generics: false,
            }],
            // v0.3 (A65): Sendable is a pure marker trait — no methods.
            "Sendable" => vec![],
            _ => vec![],
        });
}

/// Slice 5: detect whether a HirType is the literal `Self` identifier.
pub(crate) fn is_self_type(ty: &HirType) -> bool {
    matches!(ty, HirType::Path { segments, .. } if segments.len() == 1 && segments[0] == "Self")
}

/// Const-eval an array-length expression. Only integer literals supported.
fn const_eval_len(e: &HirExpr) -> Option<u64> {
    match e {
        HirExpr::Literal(HirLiteral::Int(v, _)) if *v >= 0 => Some(*v as u64),
        _ => None,
    }
}
