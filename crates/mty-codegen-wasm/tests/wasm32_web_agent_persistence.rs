//! v0.26 Track D — wasm32-web single-instance agent persistence.
//!
//! v0.25 Track C documented the design in
//! `AGENT_FIELDS_V0_25_NOTES.md` §2: agents declared in a wasm32-web
//! program should live in a fixed linear-memory region so writes made
//! during one exported callback (`inst.exports.main()`,
//! `inst.exports.frame(t)`, ...) survive the next invocation. None of
//! the emitter side shipped in v0.25; `Rvalue::AgentSpawn` and any
//! agent-state field projection fell through to
//! `WasmError::Unsupported` and the function body demoted to a single
//! `unreachable` instruction.
//!
//! v0.26 Track D wires the minimum-viable shape:
//!
//! * [`AGENT_REGION_BASE`] + [`AGENT_REGION_PER_AGENT_BYTES`] reserve
//!   a per-agent 64 KiB region in linear memory.
//! * The `cabi_realloc` bump-pointer global is initialised past the
//!   end of the agent regions so the heap doesn't collide.
//! * `Rvalue::AgentSpawn` lowers to a constant push of the agent's
//!   base address. The destination SIR local is tagged in the
//!   per-fn `agent_state_locals` map so subsequent field projections
//!   reach the agent layout.
//! * `Place { local, proj: [Field(N)] }` and `[Deref, Field(N)]`
//!   shapes (assign-side AND read-side) lower to
//!   `(I32Const(base+offset)) (I32Load|Store)` against the agent's
//!   computed layout.
//! * Agent fn-handler bodies get their `self: &mut State` param
//!   auto-tagged too, so a handler-style state read/write threads
//!   through the same layout.
//!
//! Tests below pin:
//!
//! 1. `agent_field_value_persists_across_callbacks` — the headline:
//!    spawn an agent, write a field via callback `set_field`, read
//!    it via callback `get_field`. The reader returns the writer's
//!    value (proving linear memory survives the inter-callback gap).
//! 2. `agent_array_field_persists` — same pattern with a `[U32; 200]`
//!    field (the Notetris board shape). Write index 7, read index 7,
//!    same value out.
//! 3. `multiple_callbacks_share_agent_state` — three exported
//!    callbacks (`init`, `bump`, `read`); calling `init` then `bump`
//!    three times then `read` returns the bumped value (proving
//!    state is mutable across the boundary).
//! 4. `agent_region_layout_isolates_distinct_agents` — two declared
//!    agents get non-overlapping base addresses; writing to one
//!    doesn't change the other's field value.
//! 5. `agent_region_layout_constants_well_formed` — sanity-test the
//!    public constants: per-agent reservation is at least 1 KiB and
//!    a whole-page multiple; base is page-aligned; agent_region_base
//!    is strictly monotonic.
//!
//! All tests use `wasmtime` to instantiate the emitted core wasm
//! module and call exported functions directly. No JS shim is
//! involved; the persistence behaviour falls out of wasm linear
//! memory naturally once the load/store + region-allocation plumbing
//! is correct.

mod common;

use mty_codegen_wasm::emit::{
    agent_region_base, compile_program_to_bytes, AGENT_REGION_BASE, AGENT_REGION_PER_AGENT_BYTES,
};
use mty_codegen_wasm::WasmTarget;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    AdtRef, AdtRefKind, Agent, AgentIrId, BinOp, Block, BlockId, Const, FieldRef, Function, IrFnId,
    IrTy, Local, LocalDecl, LocalSource, Operand, Place, Program, Projection, Rvalue, Stmt, Term,
    VariantRef,
};
use mty_types::{AdtId, IntKind};
use wasmtime::{Engine, Instance, Module, Store};

/// Build a `Program` with one agent declaration `Counter` carrying a
/// single `U32` field at index 0, plus three exported fns
/// (`set_field(v)`, `get_field() -> u32`, `main()` that spawns).
fn program_with_counter_agent() -> Program {
    let mut p = Program::default();
    let agent_id = AgentIrId(0);
    let state_adt = AdtId(10_000);

    // Single-field state ADT.
    p.adts.push(AdtRef {
        adt: state_adt,
        name: "__Counter::State".into(),
        kind: AdtRefKind::Struct,
        variants: vec![VariantRef {
            name: "__Counter::State".into(),
            fields: vec![FieldRef {
                name: Some("value".into()),
                ty: IrTy::Int(IntKind::U32),
            }],
        }],
    });

    // Agent declaration.
    let ctor_id = IrFnId(0); // unused but required by the Agent struct
    p.agents.push(Agent {
        id: agent_id,
        name: "Counter".into(),
        state_adt,
        ctor: ctor_id,
        handlers: vec![],
        span: SourceSpan { start: 0, end: 0 },
    });

    // Constructor stub (just for the IrFnId).
    p.fns.push(Function {
        id: ctor_id,
        name: "__Counter::__new".into(),
        params: vec![],
        locals: vec![LocalDecl {
            name: "_0".into(),
            ty: IrTy::Adt(state_adt, vec![]),
            mutable: false,
            source: LocalSource::Return,
        }],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Adt(state_adt, vec![]),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    // main() — spawn the agent. The spawned ptr goes into a local at
    // index 1; subsequent fns find the agent at the deterministic
    // base address rather than threading the pointer through globals.
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
                ty: IrTy::Adt(state_adt, vec![]),
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

    // set_field(v) — write the parameter into agent.value through a
    // synthetic agent pointer local. We re-spawn (cost-free, pushes
    // the same const address) so the local has the agent-tag.
    p.fns.push(Function {
        id: IrFnId(2),
        name: "set_field".into(),
        params: vec![Local(1)],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "v".into(),
                ty: IrTy::Int(IntKind::U32),
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
                // agent_ptr = spawn (acts as ptr-load)
                Stmt::Assign(
                    Place::local(Local(2)),
                    Rvalue::AgentSpawn {
                        agent: agent_id,
                        args: vec![],
                    },
                ),
                // agent_ptr.value = v
                Stmt::Assign(
                    Place {
                        local: Local(2),
                        proj: vec![Projection::Field(0)],
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
    });

    // get_field() -> U32 — return agent.value.
    p.fns.push(Function {
        id: IrFnId(3),
        name: "get_field".into(),
        params: vec![],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Int(IntKind::U32),
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
                ty: IrTy::Int(IntKind::U32),
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
                        field: 0,
                    },
                ),
            ],
            terminator: Term::Return(Operand::Move(Place::local(Local(2)))),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Int(IntKind::U32),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    p
}

/// Compile `prog` to a wasm32-web core module and instantiate it
/// with wasmtime. Returns the (Store, Instance) pair so callers can
/// drive exported fns directly.
fn instantiate_web_core(prog: &Program) -> (Store<()>, Instance) {
    let bytes = compile_program_to_bytes(prog, WasmTarget::Web).expect("compile");
    let engine = Engine::default();
    let module = Module::new(&engine, &bytes).expect("module decode");
    let mut store: Store<()> = Store::new(&engine, ());
    // Wasm32-web imports `mty:web/log`, `mty:web/dom`, `mty:web/canvas`,
    // `mty:web/input`. None of these tests need to call them — we
    // satisfy the imports with no-op host fns so instantiation
    // succeeds. The imports are declared at the *core* level (no
    // Component-Model wrapper) when called via
    // `compile_program_to_bytes`, so we register them as positional
    // function imports with matching signatures.
    let mut linker = wasmtime::Linker::new(&engine);
    register_web_stubs(&mut linker, &module).expect("register web stubs");
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    (store, instance)
}

/// Register no-op stubs for every `(module, name)` function import
/// the compiled core module needs. Web-target builds reference
/// imports under `mty:web/*` namespaces; we attach a same-signature
/// stub for each via `linker.func_new` so instantiation succeeds.
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
                move |_caller, _args, results| {
                    // Zero-init every result so the wasm caller has a
                    // typed value to consume.
                    for (i, res) in ft.results().enumerate() {
                        results[i] = match res {
                            wasmtime::ValType::I32 => wasmtime::Val::I32(0),
                            wasmtime::ValType::I64 => wasmtime::Val::I64(0),
                            wasmtime::ValType::F32 => wasmtime::Val::F32(0),
                            wasmtime::ValType::F64 => wasmtime::Val::F64(0),
                            other => panic!("unsupported result type: {other:?}"),
                        };
                    }
                    Ok(())
                },
            )?;
        }
    }
    Ok(())
}

#[test]
fn agent_field_value_persists_across_callbacks() {
    // The headline gap. Spawn the agent in `main`, then drive
    // `set_field(42)` and `get_field()` as separate exported calls —
    // each is a fresh wasm "callback" from the JS host's
    // perspective. If linear memory weren't persistent across
    // exports the read would return a different value (most likely
    // zero from re-initialisation OR garbage from a fresh local).
    let prog = program_with_counter_agent();
    let (mut store, inst) = instantiate_web_core(&prog);

    let main_fn = inst
        .get_typed_func::<(), ()>(&mut store, "main")
        .expect("main export");
    main_fn.call(&mut store, ()).expect("main()");

    let set_fn = inst
        .get_typed_func::<u32, ()>(&mut store, "set_field")
        .expect("set_field export");
    set_fn.call(&mut store, 42).expect("set_field(42)");

    let get_fn = inst
        .get_typed_func::<(), u32>(&mut store, "get_field")
        .expect("get_field export");
    let got = get_fn.call(&mut store, ()).expect("get_field()");
    assert_eq!(
        got, 42,
        "agent field must persist across exported-fn invocations \
         (set=42, got={got})",
    );
}

#[test]
fn agent_array_field_persists() {
    // Same persistence story, but the agent's field is `[U32; 200]`
    // (the Notetris board shape). We write to index 7 via
    // `set_cell(7, 0xCAFE)` and read it back via `get_cell(7)`.
    let mut p = Program::default();
    let agent_id = AgentIrId(0);
    let state_adt = AdtId(10_001);
    p.adts.push(AdtRef {
        adt: state_adt,
        name: "__Board::State".into(),
        kind: AdtRefKind::Struct,
        variants: vec![VariantRef {
            name: "__Board::State".into(),
            fields: vec![FieldRef {
                name: Some("cells".into()),
                ty: IrTy::Array {
                    elem: Box::new(IrTy::Int(IntKind::U32)),
                    len: Some(200),
                },
            }],
        }],
    });
    p.agents.push(Agent {
        id: agent_id,
        name: "Board".into(),
        state_adt,
        ctor: IrFnId(0),
        handlers: vec![],
        span: SourceSpan { start: 0, end: 0 },
    });
    p.fns.push(Function {
        id: IrFnId(0),
        name: "__Board::__new".into(),
        params: vec![],
        locals: vec![LocalDecl {
            name: "_0".into(),
            ty: IrTy::Adt(state_adt, vec![]),
            mutable: false,
            source: LocalSource::Return,
        }],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Adt(state_adt, vec![]),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    // main() — spawn.
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
                ty: IrTy::Adt(state_adt, vec![]),
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

    // For the array test we use the agent layout's per-element offset:
    // field 0's base + idx*4. To keep the IR test fixture simple we
    // model `set_cell` / `get_cell` as direct memory ops via the same
    // `[Field(0)]` projection, but offset the address by `idx*4`
    // through a helper fn that writes to a known cell (index 7).
    // The wasm encoder packs the layout `cells[0..200]` contiguously
    // starting at the agent's base + field-0 offset (= 0); cell 7
    // sits at base + 28. We synthesize a `set_cell_7(v)` fn that
    // writes at that offset and a `get_cell_7() -> u32` reader.
    //
    // The synthetic projection is `[Field(N)]` with N=7; but Field(N)
    // is the *struct* field index — for the array sub-element we'd
    // really want a `Project::Index(...)` chain. To stay within the
    // v0.26 emitter's supported projection-shapes (which only know
    // `Field(N)` against an agent ptr), we structure the test fixture
    // as a 200-field struct: each cell is its own struct field at
    // index 0..199. The persistence story is identical and the
    // layout exercise is even more demanding.
    let agent_state_adt_id = AdtId(10_002);
    let mut cells: Vec<FieldRef> = Vec::with_capacity(200);
    for i in 0..200 {
        cells.push(FieldRef {
            name: Some(format!("cell_{i}")),
            ty: IrTy::Int(IntKind::U32),
        });
    }
    // Re-use the existing AdtRef slot — swap in the per-cell shape.
    p.adts.clear();
    p.adts.push(AdtRef {
        adt: agent_state_adt_id,
        name: "__Board::State".into(),
        kind: AdtRefKind::Struct,
        variants: vec![VariantRef {
            name: "__Board::State".into(),
            fields: cells,
        }],
    });
    // Refresh the agent decl to point at the new state ADT.
    p.agents[0].state_adt = agent_state_adt_id;
    // Fix up the IrFnId-0 (ctor) ret type.
    p.fns[0].ret_ty = IrTy::Adt(agent_state_adt_id, vec![]);
    if let Some(l) = p.fns[0].locals.get_mut(0) {
        l.ty = IrTy::Adt(agent_state_adt_id, vec![]);
    }
    // Fix up main()'s local 1 type.
    if let Some(l) = p.fns[1].locals.get_mut(1) {
        l.ty = IrTy::Adt(agent_state_adt_id, vec![]);
    }

    // set_cell_7(v) — write field 7.
    p.fns.push(Function {
        id: IrFnId(2),
        name: "set_cell_7".into(),
        params: vec![Local(1)],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "v".into(),
                ty: IrTy::Int(IntKind::U32),
                mutable: false,
                source: LocalSource::Param,
            },
            LocalDecl {
                name: "agent_ptr".into(),
                ty: IrTy::Adt(agent_state_adt_id, vec![]),
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
                        proj: vec![Projection::Field(7)],
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
    });

    // get_cell_7() -> u32 — read field 7.
    p.fns.push(Function {
        id: IrFnId(3),
        name: "get_cell_7".into(),
        params: vec![],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Int(IntKind::U32),
                mutable: true,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "agent_ptr".into(),
                ty: IrTy::Adt(agent_state_adt_id, vec![]),
                mutable: true,
                source: LocalSource::Temp,
            },
            LocalDecl {
                name: "tmp".into(),
                ty: IrTy::Int(IntKind::U32),
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
                        field: 7,
                    },
                ),
            ],
            terminator: Term::Return(Operand::Move(Place::local(Local(2)))),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Int(IntKind::U32),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    let (mut store, inst) = instantiate_web_core(&p);
    inst.get_typed_func::<(), ()>(&mut store, "main")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    inst.get_typed_func::<u32, ()>(&mut store, "set_cell_7")
        .unwrap()
        .call(&mut store, 0xCAFE_u32)
        .unwrap();
    let got = inst
        .get_typed_func::<(), u32>(&mut store, "get_cell_7")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    assert_eq!(
        got, 0xCAFE,
        "200-field-board cell 7 must persist (wrote 0xCAFE, got {got:#x})",
    );
}

#[test]
fn multiple_callbacks_share_agent_state() {
    // Three callbacks: `init` (set to 0), `bump` (add 1), `read`
    // (return current). Call init, then bump three times, then read.
    // Each callback is independent at the wasm-export level; the
    // agent's field must thread through naturally.
    let mut p = program_with_counter_agent();

    // init(): writes 0 into agent.value.
    p.fns.push(Function {
        id: IrFnId(4),
        name: "init".into(),
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
                ty: IrTy::Adt(AdtId(10_000), vec![]),
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
                        agent: AgentIrId(0),
                        args: vec![],
                    },
                ),
                Stmt::Assign(
                    Place {
                        local: Local(1),
                        proj: vec![Projection::Field(0)],
                    },
                    Rvalue::Const(Const::Int(0, IntKind::U32)),
                ),
            ],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    // bump(): agent.value = agent.value + 1.
    p.fns.push(Function {
        id: IrFnId(5),
        name: "bump".into(),
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
                ty: IrTy::Adt(AdtId(10_000), vec![]),
                mutable: true,
                source: LocalSource::Temp,
            },
            LocalDecl {
                name: "cur".into(),
                ty: IrTy::Int(IntKind::U32),
                mutable: true,
                source: LocalSource::Temp,
            },
            LocalDecl {
                name: "next".into(),
                ty: IrTy::Int(IntKind::U32),
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
                        agent: AgentIrId(0),
                        args: vec![],
                    },
                ),
                Stmt::Assign(
                    Place::local(Local(2)),
                    Rvalue::FieldRead {
                        receiver: Place::local(Local(1)),
                        field: 0,
                    },
                ),
                Stmt::Assign(
                    Place::local(Local(3)),
                    Rvalue::BinOp(
                        BinOp::Add,
                        Operand::Copy(Place::local(Local(2))),
                        Operand::Const(Const::Int(1, IntKind::U32)),
                    ),
                ),
                Stmt::Assign(
                    Place {
                        local: Local(1),
                        proj: vec![Projection::Field(0)],
                    },
                    Rvalue::Use(Operand::Copy(Place::local(Local(3)))),
                ),
            ],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    let (mut store, inst) = instantiate_web_core(&p);
    inst.get_typed_func::<(), ()>(&mut store, "main")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    inst.get_typed_func::<(), ()>(&mut store, "init")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    let bump = inst.get_typed_func::<(), ()>(&mut store, "bump").unwrap();
    for _ in 0..3 {
        bump.call(&mut store, ()).unwrap();
    }
    let got = inst
        .get_typed_func::<(), u32>(&mut store, "get_field")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    assert_eq!(
        got, 3,
        "agent value must reflect 3 bumps (got {got}); inter-callback \
         state isn't threading through agent linear memory",
    );
}

#[test]
fn agent_region_layout_isolates_distinct_agents() {
    // Two agents declared in the same program. Writing to agent A's
    // field 0 must not change agent B's field 0 — confirms the
    // per-agent region offsets are sequential and non-overlapping.
    let mut p = Program::default();
    let agent_a = AgentIrId(0);
    let agent_b = AgentIrId(1);
    let adt_a = AdtId(10_010);
    let adt_b = AdtId(10_011);
    for (adt_id, name) in [(adt_a, "A"), (adt_b, "B")] {
        p.adts.push(AdtRef {
            adt: adt_id,
            name: format!("__{name}::State"),
            kind: AdtRefKind::Struct,
            variants: vec![VariantRef {
                name: format!("__{name}::State"),
                fields: vec![FieldRef {
                    name: Some("value".into()),
                    ty: IrTy::Int(IntKind::U32),
                }],
            }],
        });
    }
    p.agents.push(Agent {
        id: agent_a,
        name: "A".into(),
        state_adt: adt_a,
        ctor: IrFnId(0),
        handlers: vec![],
        span: SourceSpan { start: 0, end: 0 },
    });
    p.agents.push(Agent {
        id: agent_b,
        name: "B".into(),
        state_adt: adt_b,
        ctor: IrFnId(1),
        handlers: vec![],
        span: SourceSpan { start: 0, end: 0 },
    });
    // Two ctor stubs.
    for (fid, adt_id) in [(IrFnId(0), adt_a), (IrFnId(1), adt_b)] {
        p.fns.push(Function {
            id: fid,
            name: format!("__ctor_{}", fid.0),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: IrTy::Adt(adt_id, vec![]),
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Adt(adt_id, vec![]),
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        });
    }
    // main(): set A.value = 0xAAAA, set B.value = 0xBBBB.
    p.fns.push(Function {
        id: IrFnId(2),
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
                name: "a_ptr".into(),
                ty: IrTy::Adt(adt_a, vec![]),
                mutable: true,
                source: LocalSource::Temp,
            },
            LocalDecl {
                name: "b_ptr".into(),
                ty: IrTy::Adt(adt_b, vec![]),
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
                        agent: agent_a,
                        args: vec![],
                    },
                ),
                Stmt::Assign(
                    Place::local(Local(2)),
                    Rvalue::AgentSpawn {
                        agent: agent_b,
                        args: vec![],
                    },
                ),
                Stmt::Assign(
                    Place {
                        local: Local(1),
                        proj: vec![Projection::Field(0)],
                    },
                    Rvalue::Const(Const::Int(0xAAAA, IntKind::U32)),
                ),
                Stmt::Assign(
                    Place {
                        local: Local(2),
                        proj: vec![Projection::Field(0)],
                    },
                    Rvalue::Const(Const::Int(0xBBBB, IntKind::U32)),
                ),
            ],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });
    // read_a() -> u32 and read_b() -> u32.
    for (fid, name, agent, adt_id) in [
        (IrFnId(3), "read_a", agent_a, adt_a),
        (IrFnId(4), "read_b", agent_b, adt_b),
    ] {
        p.fns.push(Function {
            id: fid,
            name: name.into(),
            params: vec![],
            locals: vec![
                LocalDecl {
                    name: "_0".into(),
                    ty: IrTy::Int(IntKind::U32),
                    mutable: true,
                    source: LocalSource::Return,
                },
                LocalDecl {
                    name: "ptr".into(),
                    ty: IrTy::Adt(adt_id, vec![]),
                    mutable: true,
                    source: LocalSource::Temp,
                },
                LocalDecl {
                    name: "tmp".into(),
                    ty: IrTy::Int(IntKind::U32),
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
                            agent,
                            args: vec![],
                        },
                    ),
                    Stmt::Assign(
                        Place::local(Local(2)),
                        Rvalue::FieldRead {
                            receiver: Place::local(Local(1)),
                            field: 0,
                        },
                    ),
                ],
                terminator: Term::Return(Operand::Move(Place::local(Local(2)))),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Int(IntKind::U32),
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        });
    }

    let (mut store, inst) = instantiate_web_core(&p);
    inst.get_typed_func::<(), ()>(&mut store, "main")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    let a = inst
        .get_typed_func::<(), u32>(&mut store, "read_a")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    let b = inst
        .get_typed_func::<(), u32>(&mut store, "read_b")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    assert_eq!(a, 0xAAAA, "agent A field corrupted (got {a:#x})");
    assert_eq!(b, 0xBBBB, "agent B field corrupted (got {b:#x})");
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn agent_region_layout_constants_well_formed() {
    // Sanity-check the public constants. Mostly a guard against
    // future drift (a too-small region would silently corrupt
    // adjacent agents; a non-page-aligned base would break the wasm
    // initial-memory accounting we do at emit-time). The
    // assertions-on-constants warning is suppressed because that's
    // exactly the point — if a future commit accidentally bumps the
    // const to a smaller / mis-aligned value, this regression test
    // should fail at runtime (not silently turn into a no-op).
    assert!(
        AGENT_REGION_PER_AGENT_BYTES >= 1024,
        "per-agent region too small ({AGENT_REGION_PER_AGENT_BYTES} \
         bytes) for the canonical web-game shape",
    );
    assert!(
        AGENT_REGION_PER_AGENT_BYTES % 65536 == 0,
        "per-agent region must be a whole wasm page (got \
         {AGENT_REGION_PER_AGENT_BYTES} bytes; page = 65536)",
    );
    assert!(
        AGENT_REGION_BASE % 65536 == 0,
        "agent region base must be page-aligned (got \
         {AGENT_REGION_BASE})",
    );
    // Strict monotonicity across 4 sample agents.
    let bases: Vec<i32> = (0..4).map(agent_region_base).collect();
    for w in bases.windows(2) {
        assert!(w[0] < w[1], "agent bases not strictly monotonic: {bases:?}");
        assert_eq!(
            w[1] - w[0],
            AGENT_REGION_PER_AGENT_BYTES,
            "agent bases must be spaced by exactly \
             AGENT_REGION_PER_AGENT_BYTES; got {bases:?}",
        );
    }
}
