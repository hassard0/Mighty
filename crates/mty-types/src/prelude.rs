//! Synthetic `std.core` prelude. Provides:
//! - Primitive type aliases (`Bool`, `I8..I128`, ..., `Str`, `String`, ...)
//! - `Option[T]`, `Result[T, E]` ADTs
//! - Opaque modules (`std`, `std.http`, `std.json`, `std.dom`, `std.trace`)
//! - Opaque types for names referenced in the canonical examples without
//!   a user-side declaration (`Url`, `Page`, `IoErr`, `Logger`, etc.)
//! - Builtin fns: `log`, `panic`, `spawn`, `move`, `fetch`, `raw_ptr`, etc.
//! - Builtin method table (`.len`, `.to_str`, `.get`, `.read`, ...) for
//!   names the examples invoke on opaque receivers.
//!
//! Slice-3 strategy: when the user declares a name that the prelude also
//! defines (e.g. `parse`), the user definition takes precedence — we only
//! insert prelude entries if the name is not already in `by_name`.

use crate::defs::*;
use crate::ty::*;

#[derive(Debug, Clone, Copy)]
pub struct PreludeIds {
    pub option: AdtId,
    pub option_some: usize,
    pub option_none: usize,
    pub result: AdtId,
    pub result_ok: usize,
    pub result_err: usize,
    pub agent_ref: AdtId,
}

pub fn build_prelude(arena: &mut TyArena, defs: &mut DefMap) -> PreludeIds {
    // ---- primitives as type aliases (so `Bool`, `I32` etc. resolve via lookup) ----
    let primitives: &[(&str, TyId)] = &[
        ("Bool", arena.bool_),
        ("Char", arena.char_),
        ("Str", arena.str_),
        ("String", arena.string),
        ("Bytes", arena.bytes),
        ("Unit", arena.unit),
        ("Never", arena.never),
        ("Duration", arena.duration),
        ("Size", arena.size),
        ("I8", arena.i8),
        ("I16", arena.i16),
        ("I32", arena.i32),
        ("I64", arena.i64),
        ("I128", arena.i128),
        ("U8", arena.u8),
        ("U16", arena.u16),
        ("U32", arena.u32),
        ("U64", arena.u64),
        ("U128", arena.u128),
        ("USize", arena.usize),
        ("ISize", arena.isize),
        ("F32", arena.f32),
        ("F64", arena.f64),
    ];
    for (name, ty) in primitives {
        let adt = defs.alloc_adt(AdtDef {
            name: (*name).into(),
            kind: AdtKind::Opaque,
            generics: vec![],
            param_ids: vec![],
            variants: vec![VariantDef {
                name: (*name).into(),
                fields: vec![FieldDef {
                    name: None,
                    ty: *ty,
                }],
            }],
        });
        defs.by_name.insert((*name).into(), DefRef::Adt(adt));
    }

    // ---- modules ----
    let std_mods = [
        "std",
        "std.core",
        "std.http",
        "std.json",
        "std.dom",
        "std.trace",
        "std.io",
        "std.fs",
        "std.net",
        "std.time",
    ];
    for m in std_mods {
        let id = defs.alloc_module(m);
        defs.by_name.insert(m.into(), DefRef::Module(id));
    }

    // ---- Option[T] ----
    let option_param = defs.alloc_param(ParamDef {
        name: "T".into(),
        bounds: vec![],
    });
    let option_t_ty = arena.param(option_param);
    let option_id = defs.alloc_adt(AdtDef {
        name: "Option".into(),
        kind: AdtKind::Enum,
        generics: vec![ParamDef {
            name: "T".into(),
            bounds: vec![],
        }],
        param_ids: vec![option_param],
        variants: vec![
            VariantDef {
                name: "Some".into(),
                fields: vec![FieldDef {
                    name: None,
                    ty: option_t_ty,
                }],
            },
            VariantDef {
                name: "None".into(),
                fields: vec![],
            },
        ],
    });
    defs.by_name.insert("Option".into(), DefRef::Adt(option_id));
    defs.by_name
        .insert("Some".into(), DefRef::Variant(option_id, 0));
    defs.by_name
        .insert("None".into(), DefRef::Variant(option_id, 1));

    // ---- Result[T, E] ----
    let result_t = defs.alloc_param(ParamDef {
        name: "T".into(),
        bounds: vec![],
    });
    let result_e = defs.alloc_param(ParamDef {
        name: "E".into(),
        bounds: vec![],
    });
    let result_t_ty = arena.param(result_t);
    let result_e_ty = arena.param(result_e);
    let result_id = defs.alloc_adt(AdtDef {
        name: "Result".into(),
        kind: AdtKind::Enum,
        generics: vec![
            ParamDef {
                name: "T".into(),
                bounds: vec![],
            },
            ParamDef {
                name: "E".into(),
                bounds: vec![],
            },
        ],
        param_ids: vec![result_t, result_e],
        variants: vec![
            VariantDef {
                name: "Ok".into(),
                fields: vec![FieldDef {
                    name: None,
                    ty: result_t_ty,
                }],
            },
            VariantDef {
                name: "Err".into(),
                fields: vec![FieldDef {
                    name: None,
                    ty: result_e_ty,
                }],
            },
        ],
    });
    defs.by_name.insert("Result".into(), DefRef::Adt(result_id));
    defs.by_name
        .insert("Ok".into(), DefRef::Variant(result_id, 0));
    defs.by_name
        .insert("Err".into(), DefRef::Variant(result_id, 1));

    // ---- AgentRef[T] (used by spawn) ----
    let agent_ref_param = defs.alloc_param(ParamDef {
        name: "T".into(),
        bounds: vec![],
    });
    let agent_ref_id = defs.alloc_adt(AdtDef {
        name: "AgentRef".into(),
        kind: AdtKind::Opaque,
        generics: vec![ParamDef {
            name: "T".into(),
            bounds: vec![],
        }],
        param_ids: vec![agent_ref_param],
        variants: vec![],
    });
    defs.by_name
        .insert("AgentRef".into(), DefRef::Adt(agent_ref_id));

    // ---- opaque types referenced by examples ----
    let opaque_names = [
        "Url",
        "Page",
        "IoErr",
        "NetErr",
        "ParseErr",
        "FetchErr",
        "Logger",
        "Fetcher",
        "Lowered",
        "RunErr",
        "Fs",
        "Path",
        "Net",
        "Model",
        "Dom",
        "MainErr",
        "SearchErr",
        "Json",
        "Map",
        "Config",
        "ConfigErr",
        "WorkErr",
        "Planner",
        "Tokens",
        "Ast",
        // Capability-typed parameters (also referenced as opaque types):
        "Clock",
        // Used by example 06 (`work`, `ready`, `step`):
        // (no extra types here, but the fns are below).
        // Used by example 08 (Counter agent state init):
        // (none)
        // Used by example 10:
        "Search",
        // Used by example 16/17:
        "UserId",
        // Used by examples generally:
        "Shape",
    ];
    for name in opaque_names {
        // Skip if already defined.
        if defs.by_name.contains_key(name) {
            continue;
        }
        let adt = defs.alloc_adt(AdtDef {
            name: name.into(),
            kind: AdtKind::Opaque,
            generics: vec![],
            param_ids: vec![],
            variants: vec![],
        });
        defs.by_name.insert(name.into(), DefRef::Adt(adt));
    }

    // ---- builtin fns ----
    let io_eff = defs.intern_effect("io");
    let net_eff = defs.intern_effect("net");
    let dom_eff = defs.intern_effect("dom");
    let spawn_eff = defs.intern_effect("spawn");
    let model_eff = defs.intern_effect("model");
    let _ = (net_eff, dom_eff, spawn_eff, model_eff);

    // log: fn(Str) -> Unit effect io
    let log_id = defs.alloc_fn(FnDef {
        name: "log".into(),
        generics: vec![],
        param_ids: vec![],
        params: vec![("msg".into(), arena.str_)],
        ret: arena.unit,
        effects: vec![io_eff],
        is_pub: true,
        body: None,
        hir_fn: None,
    });
    defs.by_name.insert("log".into(), DefRef::Fn(log_id));

    // panic: fn(Str) -> Never
    let panic_id = defs.alloc_fn(FnDef {
        name: "panic".into(),
        generics: vec![],
        param_ids: vec![],
        params: vec![("msg".into(), arena.str_)],
        ret: arena.never,
        effects: vec![],
        is_pub: true,
        body: None,
        hir_fn: None,
    });
    defs.by_name.insert("panic".into(), DefRef::Fn(panic_id));

    // spawn: fn[T](T) -> AgentRef[T]
    let spawn_param = defs.alloc_param(ParamDef {
        name: "T".into(),
        bounds: vec![],
    });
    let spawn_param_ty = arena.param(spawn_param);
    let agent_ref_t = arena.adt(agent_ref_id, vec![spawn_param_ty]);
    let spawn_id = defs.alloc_fn(FnDef {
        name: "spawn".into(),
        generics: vec![ParamDef {
            name: "T".into(),
            bounds: vec![],
        }],
        param_ids: vec![spawn_param],
        params: vec![("inner".into(), spawn_param_ty)],
        ret: agent_ref_t,
        effects: vec![spawn_eff],
        is_pub: true,
        body: None,
        hir_fn: None,
    });
    defs.by_name.insert("spawn".into(), DefRef::Fn(spawn_id));

    // move: fn[T](T) -> T (identity)
    let move_param = defs.alloc_param(ParamDef {
        name: "T".into(),
        bounds: vec![],
    });
    let move_param_ty = arena.param(move_param);
    let move_id = defs.alloc_fn(FnDef {
        name: "move".into(),
        generics: vec![ParamDef {
            name: "T".into(),
            bounds: vec![],
        }],
        param_ids: vec![move_param],
        params: vec![("v".into(), move_param_ty)],
        ret: move_param_ty,
        effects: vec![],
        is_pub: true,
        body: None,
        hir_fn: None,
    });
    defs.by_name.insert("move".into(), DefRef::Fn(move_id));

    // raw_ptr: fn(USize) -> *U8
    let raw_ptr_ret = arena.raw_ptr(arena.u8);
    let raw_ptr_id = defs.alloc_fn(FnDef {
        name: "raw_ptr".into(),
        generics: vec![],
        param_ids: vec![],
        params: vec![("addr".into(), arena.usize)],
        ret: raw_ptr_ret,
        effects: vec![],
        is_pub: true,
        body: None,
        hir_fn: None,
    });
    defs.by_name
        .insert("raw_ptr".into(), DefRef::Fn(raw_ptr_id));

    // valid: fn(*U8, USize) -> Bool
    let valid_id = defs.alloc_fn(FnDef {
        name: "valid".into(),
        generics: vec![],
        param_ids: vec![],
        params: vec![("ptr".into(), raw_ptr_ret), ("len".into(), arena.usize)],
        ret: arena.bool_,
        effects: vec![],
        is_pub: true,
        body: None,
        hir_fn: None,
    });
    defs.by_name.insert("valid".into(), DefRef::Fn(valid_id));

    // null: a value of type *U8 (used in `ptr != null`)
    let null_id = defs.alloc_fn(FnDef {
        name: "null".into(),
        generics: vec![],
        param_ids: vec![],
        params: vec![],
        ret: raw_ptr_ret,
        effects: vec![],
        is_pub: true,
        body: None,
        hir_fn: None,
    });
    defs.by_name.insert("null".into(), DefRef::Fn(null_id));

    // fetch: fn(Url) -> Bytes!NetErr — referenced by example 04
    if let Some(DefRef::Adt(url_adt)) = defs.lookup("Url") {
        let url_ty = arena.adt(url_adt, vec![]);
        if let Some(DefRef::Adt(neterr_adt)) = defs.lookup("NetErr") {
            let neterr_ty = arena.adt(neterr_adt, vec![]);
            // Return Str (not Bytes) so example 04's `parse(body)?` —
            // where the user `parse` takes Str — composes.
            let result_url_ok = arena.str_;
            let fetch_ret = arena.adt(result_id, vec![result_url_ok, neterr_ty]);
            let fetch_id = defs.alloc_fn(FnDef {
                name: "fetch".into(),
                generics: vec![],
                param_ids: vec![],
                params: vec![("url".into(), url_ty)],
                ret: fetch_ret,
                effects: vec![],
                is_pub: true,
                body: None,
                hir_fn: None,
            });
            // Use weak-shadow: only if not user-defined.
            defs.by_name
                .entry("fetch".into())
                .or_insert(DefRef::Fn(fetch_id));
        }
    }

    // ---- builtin method table ----
    // All entries: arity = None (variadic, permissive), ret = None (fresh Var).
    let permissive_methods = [
        "len",
        "to_str",
        "to_string",
        "get",
        "ok_or",
        "query",
        "set_text",
        "read",
        "write",
        "embed",
        "post",
        "encode",
        "decode",
        "ok",
        "serve",
        "on",
        "restart",
        "backoff",
        "ro",
        "rw",
        "is_empty",
        "push",
        "pop",
        "iter",
        "map",
        "filter",
        "collect",
        "fold",
        "count",
        "as_str",
        "trim",
        "split",
        "join",
        "contains",
        "starts_with",
        "ends_with",
        "parse",
        "unwrap",
        "unwrap_or",
        "expect",
        "clone",
        "into",
        "from",
        "as_ref",
        "as_mut",
        "with",
        "build",
        "new",
        "open",
        "close",
        "send",
        "ask",
        "reply",
        "stop",
        "child",
        "spawn",
        "method",
        "to_str",
    ];
    for m in permissive_methods {
        defs.builtin_methods.insert(
            m.into(),
            BuiltinMethod {
                arity: None,
                ret: None,
            },
        );
    }

    PreludeIds {
        option: option_id,
        option_some: 0,
        option_none: 1,
        result: result_id,
        result_ok: 0,
        result_err: 1,
        agent_ref: agent_ref_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_and_modules_present() {
        let mut a = TyArena::new();
        let mut d = DefMap::default();
        let _ = build_prelude(&mut a, &mut d);
        assert!(matches!(d.lookup("Bool"), Some(DefRef::Adt(_))));
        assert!(matches!(d.lookup("std.http"), Some(DefRef::Module(_))));
        assert!(matches!(d.lookup("Option"), Some(DefRef::Adt(_))));
        assert!(matches!(d.lookup("Some"), Some(DefRef::Variant(_, 0))));
        assert!(matches!(d.lookup("None"), Some(DefRef::Variant(_, 1))));
        assert!(matches!(d.lookup("Result"), Some(DefRef::Adt(_))));
        assert!(matches!(d.lookup("log"), Some(DefRef::Fn(_))));
        assert!(matches!(d.lookup("panic"), Some(DefRef::Fn(_))));
        assert!(matches!(d.lookup("spawn"), Some(DefRef::Fn(_))));
    }

    #[test]
    fn builtin_methods_loaded() {
        let mut a = TyArena::new();
        let mut d = DefMap::default();
        let _ = build_prelude(&mut a, &mut d);
        assert!(d.builtin_methods.contains_key("len"));
        assert!(d.builtin_methods.contains_key("get"));
        assert!(d.builtin_methods.contains_key("ok_or"));
    }
}
