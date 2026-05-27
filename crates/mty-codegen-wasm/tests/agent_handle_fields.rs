//! v0.27 Track B — opaque-ADT agent fields lower to wasm32-web.
//!
//! v0.26 demo 07 (Track E) noted that the wasm32-web emitter didn't
//! know how to store an opaque ADT (`AnthropicClient`, `VectorStore`,
//! `Episodic`, `Working`) in the agent's 64 KiB linear-memory region.
//! The SIR interpreter threaded them through `Value::Opaque` slots
//! fine, so the demo ran end-to-end on the tokio runtime via
//! `mty run`, but the wasm-side lift was deferred to v0.27.
//!
//! v0.27 Track B confirms the shape works end-to-end:
//!
//! 1. Agent fields of opaque ADT type (`IrTy::Adt(_, _)` with empty
//!    args) lower as 4-byte i32 handle slots in the agent's linear-
//!    memory region. The i32 is an opaque integer the host-side JS
//!    shim uses to index into a resource table that owns the underlying
//!    Rust value.
//! 2. Loads + stores against the field route through `I32Load` /
//!    `I32Store` against the agent's computed layout offset.
//! 3. The handle survives across callback boundaries (linear memory
//!    persists across exported-fn invocations — the v0.26 Track D
//!    contract still holds for handle-typed fields).
//!
//! Tests pin all three properties. See
//! `dev/history/notes/OPAQUE_ADT_WASM_V0_27_NOTES.md` for the
//! resource-table convention and how the JS shim cooperates.

mod common;

use mty_codegen_wasm::emit::{
    agent_region_base, compile_program_to_bytes, AGENT_REGION_BASE, AGENT_REGION_PER_AGENT_BYTES,
};
use mty_codegen_wasm::WasmTarget;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    AdtRef, AdtRefKind, Agent, AgentIrId, Block, BlockId, Const, FieldRef, Function, IrFnId, IrTy,
    Local, LocalDecl, LocalSource, Operand, Place, Program, Projection, Rvalue, Stmt, Term,
    VariantRef,
};
use mty_types::AdtId;
use wasmtime::{Engine, Instance, Module, Store};

// AdtId for the synthetic AnthropicClient opaque type we model below.
// Picked deliberately high to dodge any real prelude AdtIds.
const ANTHROPIC_CLIENT_ADT: AdtId = AdtId(20_001);

/// Build a `Program` with a `Researcher` agent carrying a single
/// `client: AnthropicClient` opaque-ADT field. The agent's state ADT
/// has one field whose `IrTy` is `IrTy::Adt(ANTHROPIC_CLIENT_ADT,
/// vec![])` — the canonical opaque-handle shape the v0.27 carve-out
/// shipped support for.
///
/// Exports:
///   * `main()` — spawns the agent (zero-arg ctor).
///   * `set_client(handle: i32)` — writes the handle into the field.
///   * `get_client() -> i32` — reads it back.
fn program_with_llm_field_agent() -> Program {
    let mut p = Program::default();
    let agent_id = AgentIrId(0);
    let state_adt = AdtId(20_000);

    // Register the opaque AnthropicClient ADT in the IR program. Its
    // shape doesn't matter — wasm only sees i32-shaped handles when
    // reading the field; the ADT registration is a courtesy for
    // tooling (e.g. SIR dump).
    p.adts.push(AdtRef {
        adt: ANTHROPIC_CLIENT_ADT,
        name: "AnthropicClient".into(),
        kind: AdtRefKind::Struct,
        variants: vec![VariantRef {
            name: "AnthropicClient".into(),
            fields: vec![],
        }],
    });

    // Agent state ADT: single `client: AnthropicClient` field.
    p.adts.push(AdtRef {
        adt: state_adt,
        name: "__Researcher::State".into(),
        kind: AdtRefKind::Struct,
        variants: vec![VariantRef {
            name: "__Researcher::State".into(),
            fields: vec![FieldRef {
                name: Some("client".into()),
                ty: IrTy::Adt(ANTHROPIC_CLIENT_ADT, vec![]),
            }],
        }],
    });

    let ctor_id = IrFnId(0);
    p.agents.push(Agent {
        id: agent_id,
        name: "Researcher".into(),
        state_adt,
        ctor: ctor_id,
        handlers: vec![],
        span: SourceSpan { start: 0, end: 0 },
    });

    // Constructor stub.
    p.fns.push(Function {
        id: ctor_id,
        name: "__Researcher::__new".into(),
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

    // main() — spawn the agent.
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

    // set_client(handle: i32) — writes the handle into agent.client.
    // The handle is just an i32 value — the host-side JS shim uses
    // it as a resource-table key.
    p.fns.push(Function {
        id: IrFnId(2),
        name: "set_client".into(),
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

    // get_client() -> i32 — reads agent.client back as an i32 handle.
    p.fns.push(Function {
        id: IrFnId(3),
        name: "get_client".into(),
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
                        field: 0,
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
    });

    p
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
// Test 1: an agent declaration carrying an opaque-ADT field compiles
// cleanly to wasm32-web (no `WasmError::Unsupported`).
// -----------------------------------------------------------------
#[test]
fn agent_with_llm_handle_field_compiles_to_web() {
    let prog = program_with_llm_field_agent();
    let bytes =
        compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile must not error out");
    // Non-empty wasm payload.
    assert!(
        bytes.len() > 8,
        "compiled wasm bytes too small: {} bytes",
        bytes.len()
    );
    // Magic header check — `\0asm` (0x00 0x61 0x73 0x6d).
    assert_eq!(&bytes[..4], b"\0asm", "wasm magic header missing");
}

// -----------------------------------------------------------------
// Test 2: reading the handle field lowers as I32Load against the
// agent's computed offset. We verify by instantiating + driving
// the exported `set_client` / `get_client` callbacks and checking
// the round-trip identity: the handle written in one callback is
// the handle read back in another.
// -----------------------------------------------------------------
#[test]
fn agent_handle_field_loads_as_i32() {
    let prog = program_with_llm_field_agent();
    let (mut store, instance) = instantiate_web_core(&prog);

    // Spawn the agent first (so the region exists in linear memory).
    let main = instance
        .get_typed_func::<(), ()>(&mut store, "main")
        .expect("main export");
    main.call(&mut store, ()).expect("main call");

    // Write a handle.
    let set_client = instance
        .get_typed_func::<i32, ()>(&mut store, "set_client")
        .expect("set_client export");
    let expected_handle: i32 = 0x1234_ABCD_u32 as i32;
    set_client
        .call(&mut store, expected_handle)
        .expect("set_client call");

    // Read it back.
    let get_client = instance
        .get_typed_func::<(), i32>(&mut store, "get_client")
        .expect("get_client export");
    let got = get_client.call(&mut store, ()).expect("get_client call");

    assert_eq!(
        got, expected_handle,
        "opaque-ADT handle field round-trip mismatch (i32 load vs store)"
    );
}

// -----------------------------------------------------------------
// Test 3: the handle persists across callback re-entries. v0.26
// Track D pinned the underlying agent persistence (linear memory
// survives between exported-fn invocations); this test confirms
// the property holds for opaque-handle fields specifically.
// -----------------------------------------------------------------
#[test]
fn agent_handle_field_persists_across_callbacks() {
    let prog = program_with_llm_field_agent();
    let (mut store, instance) = instantiate_web_core(&prog);

    let main = instance
        .get_typed_func::<(), ()>(&mut store, "main")
        .expect("main export");
    let set_client = instance
        .get_typed_func::<i32, ()>(&mut store, "set_client")
        .expect("set_client export");
    let get_client = instance
        .get_typed_func::<(), i32>(&mut store, "get_client")
        .expect("get_client export");

    main.call(&mut store, ()).expect("main");

    // Sequence: write H1, read (must equal H1), write H2, read (must
    // equal H2 — proves the slot was actually overwritten, not just
    // sticky from H1).
    let h1: i32 = 42;
    set_client.call(&mut store, h1).expect("set H1");
    let r1 = get_client.call(&mut store, ()).expect("get after H1");
    assert_eq!(r1, h1, "first round-trip");

    let h2: i32 = 137;
    set_client.call(&mut store, h2).expect("set H2");
    let r2 = get_client.call(&mut store, ()).expect("get after H2");
    assert_eq!(r2, h2, "second round-trip after overwrite");

    // Read again WITHOUT writing in between — handle survives
    // the no-op gap (the headline persistence property).
    let r3 = get_client.call(&mut store, ()).expect("get repeat");
    assert_eq!(r3, h2, "handle survives idle callback re-entry");
}

// -----------------------------------------------------------------
// Test 4: agent region constants are sane. Mirrors the same sanity
// asserts as `wasm32_web_agent_persistence.rs` but pinned next to
// the handle-specific tests so a regression on either side surfaces
// here.
// -----------------------------------------------------------------
#[test]
fn handle_field_region_layout_constants_well_formed() {
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(AGENT_REGION_PER_AGENT_BYTES >= 1024, "per-agent < 1 KiB");
    }
    assert_eq!(
        AGENT_REGION_PER_AGENT_BYTES % 65536,
        0,
        "per-agent not a wasm-page multiple"
    );
    assert_eq!(AGENT_REGION_BASE % 65536, 0, "region base not page-aligned");
    assert!(
        agent_region_base(1) > agent_region_base(0),
        "agent_region_base not strictly monotonic"
    );
}
