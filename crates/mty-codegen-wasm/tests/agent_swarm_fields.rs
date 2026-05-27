//! v0.29 Track B — std.swarm opaque-ADT agent fields lower to wasm32-web.
//!
//! Parallels the v0.27 Track B `agent_handle_fields.rs` test pattern,
//! but for the four swarm ADTs (`Member`, `DollarBudget`,
//! `ConsensusStrategy`, `Consensus`) that v0.29 Track B added to the
//! handler-safe carve-out. v0.27 already pinned the underlying lowering
//! contract — any `IrTy::Adt(_, _)` agent field lowers as a 4-byte i32
//! handle slot — so the test surface here exists to guarantee that the
//! contract still holds end-to-end for swarm-shaped state ADTs.
//!
//! These tests pin the same three properties:
//!
//! 1. An agent state ADT carrying a swarm opaque-ADT field compiles
//!    cleanly to wasm32-web (no `WasmError::Unsupported`).
//! 2. Reads against the field lower as `I32Load` against the agent's
//!    computed layout offset (verified via the set/get round-trip).
//! 3. The handle survives across callback re-entries — the v0.26
//!    Track D linear-memory persistence contract still holds for
//!    swarm-shaped fields.
//!
//! See `crates/mty-types/tests/opaque_adt_handler_scope.rs` for the
//! companion strict-handler-scope check (MT2021 carve-out).

mod common;

use mty_codegen_wasm::emit::compile_program_to_bytes;
use mty_codegen_wasm::WasmTarget;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    AdtRef, AdtRefKind, Agent, AgentIrId, Block, BlockId, Const, FieldRef, Function, IrFnId, IrTy,
    Local, LocalDecl, LocalSource, Operand, Place, Program, Projection, Rvalue, Stmt, Term,
    VariantRef,
};
use mty_types::AdtId;
use wasmtime::{Engine, Instance, Module, Store};

// AdtIds for the synthetic swarm opaque types we model below. Picked
// deliberately high to dodge any real prelude AdtIds and the v0.27
// `agent_handle_fields.rs` test fixture's IDs.
const MEMBER_ADT: AdtId = AdtId(20_101);
const DOLLAR_BUDGET_ADT: AdtId = AdtId(20_102);
const CONSENSUS_STRATEGY_ADT: AdtId = AdtId(20_103);
const CONSENSUS_ADT: AdtId = AdtId(20_104);
const REVIEWER_STATE_ADT: AdtId = AdtId(20_100);

/// Build a `Program` with a `CodeReviewer` agent carrying ALL FOUR
/// swarm opaque-ADT fields (`panel_handle: Member`,
/// `budget_handle: DollarBudget`, `strategy_handle: ConsensusStrategy`,
/// `last_consensus: Consensus`) inline in its state ADT. Each lowers
/// as a 4-byte i32 handle slot — the v0.27 Track B opaque-handle
/// lowering contract — and the offsets are computed by the layout
/// machinery without special-casing the swarm ADTs.
///
/// Exports:
///   * `main()` — spawns the agent (zero-arg ctor).
///   * `set_member(handle: i32)` / `get_member() -> i32`
///   * `set_consensus(handle: i32)` / `get_consensus() -> i32`
///     (exercising the last field — proves the offset math is
///     correct, not just the leading field).
fn program_with_swarm_field_agent() -> Program {
    let mut p = Program::default();
    let agent_id = AgentIrId(0);

    // Register each swarm opaque ADT in the IR program. Shapes don't
    // matter — wasm only sees i32-shaped handles when reading the
    // fields; the ADT registrations are a courtesy for tooling (e.g.
    // SIR dump). Each ADT is `Struct` with no fields, matching the
    // v0.27 `agent_handle_fields.rs` fixture pattern.
    for (adt, name) in [
        (MEMBER_ADT, "Member"),
        (DOLLAR_BUDGET_ADT, "DollarBudget"),
        (CONSENSUS_STRATEGY_ADT, "ConsensusStrategy"),
        (CONSENSUS_ADT, "Consensus"),
    ] {
        p.adts.push(AdtRef {
            adt,
            name: name.into(),
            kind: AdtRefKind::Struct,
            variants: vec![VariantRef {
                name: name.into(),
                fields: vec![],
            }],
        });
    }

    // Agent state ADT: four opaque-handle fields in declaration order.
    // Field 0 = panel member, Field 1 = budget, Field 2 = strategy,
    // Field 3 = last consensus. Layout machinery computes offsets at
    // 4-byte stride per field.
    p.adts.push(AdtRef {
        adt: REVIEWER_STATE_ADT,
        name: "__CodeReviewer::State".into(),
        kind: AdtRefKind::Struct,
        variants: vec![VariantRef {
            name: "__CodeReviewer::State".into(),
            fields: vec![
                FieldRef {
                    name: Some("panel".into()),
                    ty: IrTy::Adt(MEMBER_ADT, vec![]),
                },
                FieldRef {
                    name: Some("budget".into()),
                    ty: IrTy::Adt(DOLLAR_BUDGET_ADT, vec![]),
                },
                FieldRef {
                    name: Some("strategy".into()),
                    ty: IrTy::Adt(CONSENSUS_STRATEGY_ADT, vec![]),
                },
                FieldRef {
                    name: Some("last_consensus".into()),
                    ty: IrTy::Adt(CONSENSUS_ADT, vec![]),
                },
            ],
        }],
    });

    let ctor_id = IrFnId(0);
    p.agents.push(Agent {
        id: agent_id,
        name: "CodeReviewer".into(),
        state_adt: REVIEWER_STATE_ADT,
        ctor: ctor_id,
        handlers: vec![],
        span: SourceSpan { start: 0, end: 0 },
    });

    // Constructor stub.
    p.fns.push(Function {
        id: ctor_id,
        name: "__CodeReviewer::__new".into(),
        params: vec![],
        locals: vec![LocalDecl {
            name: "_0".into(),
            ty: IrTy::Adt(REVIEWER_STATE_ADT, vec![]),
            mutable: false,
            source: LocalSource::Return,
        }],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Adt(REVIEWER_STATE_ADT, vec![]),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    // main() — spawn the agent so the linear-memory region exists.
    p.fns.push(Function {
        id: IrFnId(1),
        name: "main".into(),
        params: vec![],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "agent_ptr".into(),
                ty: IrTy::Adt(REVIEWER_STATE_ADT, vec![]),
                mutable: true,
                source: LocalSource::Temp,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![Stmt::Assign(
                Place::local(Local(1)),
                Rvalue::AgentSpawn {
                    agent: agent_id,
                    args: vec![],
                },
            )],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    // set_member(handle) — writes to field 0 (Member).
    p.fns.push(set_field_fn(
        IrFnId(2),
        "set_member",
        agent_id,
        REVIEWER_STATE_ADT,
        0,
    ));
    // get_member() — reads field 0.
    p.fns.push(get_field_fn(
        IrFnId(3),
        "get_member",
        agent_id,
        REVIEWER_STATE_ADT,
        0,
    ));
    // set_consensus(handle) — writes to field 3 (the LAST field —
    // proves offset math is correct for non-leading positions).
    p.fns.push(set_field_fn(
        IrFnId(4),
        "set_consensus",
        agent_id,
        REVIEWER_STATE_ADT,
        3,
    ));
    // get_consensus() — reads field 3.
    p.fns.push(get_field_fn(
        IrFnId(5),
        "get_consensus",
        agent_id,
        REVIEWER_STATE_ADT,
        3,
    ));

    p
}

/// Synthesise `fn <name>(handle: i32)` writing `handle` into
/// `agent.<field_index>`. The agent is spawned fresh inside the body
/// so the test doesn't need a sticky agent_ptr across calls — the
/// v0.26 Track D linear-memory contract guarantees the per-agent
/// region is the same on every entry.
fn set_field_fn(
    id: IrFnId,
    name: &str,
    agent_id: AgentIrId,
    state_adt: AdtId,
    field_index: usize,
) -> Function {
    Function {
        id,
        name: name.into(),
        params: vec![Local(1)],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "handle".into(),
                ty: IrTy::Int(mty_types::IntKind::I32),
                mutable: false,
                source: LocalSource::Param,
            },
            LocalDecl {
                name: "agent_ptr".into(),
                ty: IrTy::Adt(state_adt, vec![]),
                mutable: true,
                source: LocalSource::Temp,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![
                Stmt::Assign(
                    Place::local(Local(2)),
                    Rvalue::AgentSpawn {
                        agent: agent_id,
                        args: vec![],
                    },
                ),
                Stmt::Assign(
                    Place {
                        local: Local(2),
                        proj: vec![Projection::Field(field_index)],
                    },
                    Rvalue::Use(Operand::Copy(Place::local(Local(1)))),
                ),
            ],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    }
}

/// Synthesise `fn <name>() -> i32` reading `agent.<field_index>`.
fn get_field_fn(
    id: IrFnId,
    name: &str,
    agent_id: AgentIrId,
    state_adt: AdtId,
    field_index: usize,
) -> Function {
    Function {
        id,
        name: name.into(),
        params: vec![],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Int(mty_types::IntKind::I32),
                mutable: true,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "agent_ptr".into(),
                ty: IrTy::Adt(state_adt, vec![]),
                mutable: true,
                source: LocalSource::Temp,
            },
            LocalDecl {
                name: "tmp".into(),
                ty: IrTy::Int(mty_types::IntKind::I32),
                mutable: true,
                source: LocalSource::Temp,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![
                Stmt::Assign(
                    Place::local(Local(1)),
                    Rvalue::AgentSpawn {
                        agent: agent_id,
                        args: vec![],
                    },
                ),
                Stmt::Assign(
                    Place::local(Local(2)),
                    Rvalue::FieldRead {
                        receiver: Place::local(Local(1)),
                        field: field_index,
                    },
                ),
            ],
            terminator: Term::Return(Operand::Move(Place::local(Local(2)))),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Int(mty_types::IntKind::I32),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    }
}

/// Compile `prog` to a wasm32-web core module and instantiate it
/// with wasmtime. Web-target builds reference imports under
/// `mty:web/*` namespaces; we attach no-op stubs so instantiation
/// succeeds.
fn instantiate_web_core(prog: &Program) -> (Store<()>, Instance) {
    let bytes = compile_program_to_bytes(prog, WasmTarget::Web).expect("compile");
    let engine = Engine::default();
    let module = Module::new(&engine, &bytes).expect("module decode");
    let mut store: Store<()> = Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    register_web_stubs(&mut linker, &module).expect("register web stubs");
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    (store, instance)
}

fn register_web_stubs(linker: &mut wasmtime::Linker<()>, module: &Module) -> wasmtime::Result<()> {
    use wasmtime::ExternType;
    let mut seen = std::collections::HashSet::new();
    for imp in module.imports() {
        let key = (imp.module().to_string(), imp.name().to_string());
        if !seen.insert(key.clone()) {
            continue;
        }
        if let ExternType::Func(ft) = imp.ty() {
            let ft_clone = ft.clone();
            linker.func_new(
                imp.module(),
                imp.name(),
                ft_clone,
                |_caller, _args, results| {
                    for r in results.iter_mut() {
                        *r = wasmtime::Val::I32(0);
                    }
                    Ok(())
                },
            )?;
        }
    }
    Ok(())
}

// -----------------------------------------------------------------
// Test 1: an agent state ADT carrying every swarm opaque-ADT field
// compiles cleanly to wasm32-web (no `WasmError::Unsupported`).
// -----------------------------------------------------------------
#[test]
fn agent_with_swarm_handle_fields_compiles_to_web() {
    let prog = program_with_swarm_field_agent();
    let bytes =
        compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile must not error out");
    assert!(
        bytes.len() > 8,
        "compiled wasm bytes too small: {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[..4], b"\0asm", "wasm magic header missing");
}

// -----------------------------------------------------------------
// Test 2: the leading swarm handle field (Member at index 0) loads
// + stores correctly. Mirrors the v0.27 round-trip identity check
// but proves the lowering still works when there are sibling
// opaque-ADT fields trailing it.
// -----------------------------------------------------------------
#[test]
fn swarm_member_field_loads_as_i32() {
    let prog = program_with_swarm_field_agent();
    let (mut store, instance) = instantiate_web_core(&prog);

    let main = instance
        .get_typed_func::<(), ()>(&mut store, "main")
        .expect("main export");
    main.call(&mut store, ()).expect("main call");

    let set_member = instance
        .get_typed_func::<i32, ()>(&mut store, "set_member")
        .expect("set_member export");
    let get_member = instance
        .get_typed_func::<(), i32>(&mut store, "get_member")
        .expect("get_member export");

    let expected: i32 = 0x5EED_1234_u32 as i32;
    set_member.call(&mut store, expected).expect("set_member");
    let got = get_member.call(&mut store, ()).expect("get_member");
    assert_eq!(
        got, expected,
        "Member handle (field 0) round-trip mismatch (i32 load vs store)"
    );
}

// -----------------------------------------------------------------
// Test 3: the trailing swarm handle field (Consensus at index 3)
// loads + stores correctly. This is the headline coverage gap that
// `agent_handle_fields.rs` couldn't pin (it only had a single-field
// state ADT) — the offset math has to thread three 4-byte slots
// before hitting the consensus handle slot.
// -----------------------------------------------------------------
#[test]
fn swarm_consensus_field_at_offset_round_trips() {
    let prog = program_with_swarm_field_agent();
    let (mut store, instance) = instantiate_web_core(&prog);

    let main = instance
        .get_typed_func::<(), ()>(&mut store, "main")
        .expect("main export");
    main.call(&mut store, ()).expect("main call");

    let set_consensus = instance
        .get_typed_func::<i32, ()>(&mut store, "set_consensus")
        .expect("set_consensus export");
    let get_consensus = instance
        .get_typed_func::<(), i32>(&mut store, "get_consensus")
        .expect("get_consensus export");

    let expected: i32 = 0x0C0F_FEE7_u32 as i32;
    set_consensus
        .call(&mut store, expected)
        .expect("set_consensus");
    let got = get_consensus.call(&mut store, ()).expect("get_consensus");
    assert_eq!(
        got, expected,
        "Consensus handle (field 3) round-trip mismatch — offset math may be wrong"
    );
}

// -----------------------------------------------------------------
// Test 4: writes to one swarm handle field do NOT clobber adjacent
// swarm handle slots. Verifies the per-field offsets are distinct —
// not just that one slot survives in isolation. This is the
// orthogonality property that distinguishes a real 4-byte-per-field
// layout from an accidental "all four fields share one slot" shape.
// -----------------------------------------------------------------
#[test]
fn swarm_handle_fields_do_not_alias() {
    let prog = program_with_swarm_field_agent();
    let (mut store, instance) = instantiate_web_core(&prog);

    let main = instance
        .get_typed_func::<(), ()>(&mut store, "main")
        .expect("main export");
    main.call(&mut store, ()).expect("main call");

    let set_member = instance
        .get_typed_func::<i32, ()>(&mut store, "set_member")
        .expect("set_member export");
    let get_member = instance
        .get_typed_func::<(), i32>(&mut store, "get_member")
        .expect("get_member export");
    let set_consensus = instance
        .get_typed_func::<i32, ()>(&mut store, "set_consensus")
        .expect("set_consensus export");
    let get_consensus = instance
        .get_typed_func::<(), i32>(&mut store, "get_consensus")
        .expect("get_consensus export");

    // Write distinct handles to field 0 + field 3. Read both back —
    // each must return its own value.
    let h_member: i32 = 0x1111_1111;
    let h_consensus: i32 = 0x4444_4444_u32 as i32;
    set_member.call(&mut store, h_member).expect("set_member");
    set_consensus
        .call(&mut store, h_consensus)
        .expect("set_consensus");

    assert_eq!(
        get_member.call(&mut store, ()).expect("get_member"),
        h_member,
        "Member field clobbered by Consensus write — fields alias"
    );
    assert_eq!(
        get_consensus.call(&mut store, ()).expect("get_consensus"),
        h_consensus,
        "Consensus field clobbered by Member write — fields alias"
    );
}
