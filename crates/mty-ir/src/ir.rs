//! Mighty mid-level IR (spec §24.4).
//!
//! Basic-block form with explicit moves, copies, borrows, effect calls,
//! capability values, arena lifetimes, and async-suspension placeholders.
//!
//! Lowered from typed + borrow-checked HIR. Consumed by the interpreter
//! (slice 6) and — post-v0.1 — by LLVM/Cranelift/Wasm backends.

use mty_hir::{FnId, SourceSpan};
use mty_types::{AdtId, CapConstraint, CapFamily, EffectId, FloatKind, IntKind};

// ----- ID newtypes ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IrFnId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Local(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ArenaId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentIrId(pub u32);

// ----- SIR types -----------------------------------------------------------

/// Slimmed-down type representation. The interpreter and dumper use this;
/// it carries enough information to identify ADTs and primitives but
/// elides generics/inference variables (those were resolved upstream).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrTy {
    Bool,
    Int(IntKind),
    Float(FloatKind),
    Char,
    Str,
    String,
    Bytes,
    Unit,
    Never,
    Duration,
    Size,
    Tuple(Vec<IrTy>),
    Array {
        elem: Box<IrTy>,
        len: Option<u64>,
    },
    Ref {
        mutable: bool,
        inner: Box<IrTy>,
    },
    Fn {
        params: Vec<IrTy>,
        ret: Box<IrTy>,
    },
    Adt(AdtId, Vec<IrTy>),
    /// Capability value type (spec §8).
    Cap {
        family: CapFamily,
        constraint: CapConstraint,
    },
    /// `dyn Trait` — opaque to the interpreter.
    Dyn(String),
    /// Raw pointer (slice 6: integer-sized; unsafe-only).
    RawPtr(Box<IrTy>),
    /// Opaque module value (e.g. `std.http`); never appears as a runtime
    /// payload — used purely to keep the lowerer total.
    Module(String),
    /// Generic param placeholder. Slice 6 doesn't monomorphize so we
    /// carry the parameter name through. The interpreter treats values
    /// of `Param` types polymorphically (the original value flows through
    /// unchanged).
    Param(String),
    /// Type poisoned upstream; the lowerer propagates it so we don't
    /// crash on partly-type-checked code.
    Error,
}

impl IrTy {
    pub fn is_unit(&self) -> bool {
        matches!(self, IrTy::Unit)
    }
}

// ----- Local declarations --------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSource {
    /// Function parameter (index = local id in the params region).
    Param,
    /// `let x = ...` binding.
    UserLet,
    /// Compiler-synthesized temporary.
    Temp,
    /// Drop flag (always Bool).
    DropFlag,
    /// Return slot (`_0`).
    Return,
}

#[derive(Debug, Clone)]
pub struct LocalDecl {
    pub name: String,
    pub ty: IrTy,
    pub mutable: bool,
    pub source: LocalSource,
}

// ----- Constants -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Unit,
    Bool(bool),
    Int(i128, IntKind),
    Float(f64, FloatKind),
    Str(String),
    Char(char),
    Duration {
        value: u64,
        unit: String,
    },
    Size {
        value: u64,
        unit: String,
    },
    /// Reference to a fn at the const level (used by lambdas-as-values
    /// and trait dispatch). The interpreter resolves to a `Value::Fn`.
    FnPtr(FnRef),
    /// A null raw pointer (slice 6: numerically `0`).
    NullPtr,
}

// ----- Function references -------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FnRef {
    User(IrFnId),
    Builtin(BuiltinId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BuiltinId {
    Log,
    Print,
    Panic,
    Spawn,
    Move,
    Fetch,
    RawPtr,
    Valid,
    Null,
    /// Externally-declared fn from an `extern { ... }` block. The
    /// interpreter looks up the name in the host's extern table.
    Extern(String),
    /// v0.6 — first-class DOM capability op. Emitted when a method
    /// call's receiver has type `Cap { family: Dom, .. }`. The string
    /// is the bare SIR method name (`set_text`, `get_text`, `on_click`,
    /// `query`, …). The wasm32-web backend routes these through
    /// `emit_dom_call` to the `stardust:web/dom` import set; the SIR
    /// interpreter routes them through the host's extern table as
    /// `dom.<name>` so non-wasm test runs still execute.
    DomOp(String),
}

// ----- Operands + places ---------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// Use-by-copy. The borrow checker has already certified the source.
    Copy(Place),
    /// Use-by-move. The source local must not be touched again on this
    /// control-flow path.
    Move(Place),
    Const(Const),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub local: Local,
    pub proj: Vec<Projection>,
}

impl Place {
    pub fn local(l: Local) -> Self {
        Self {
            local: l,
            proj: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Projection {
    Field(usize),
    TupleIndex(usize),
    Deref,
    Index(Local),
    /// `enum.<variant>.<field_idx>` — slice 6 always pairs this with a
    /// preceding SwitchVariant so we know the variant is correct.
    VariantField(usize, usize),
}

// ----- Effect calls --------------------------------------------------------

/// Slice 6 only models a small set of effect surfaces: anything the
/// 20 examples touch. Each `EffectOp` carries the receiver path so the
/// interpreter can route to the host.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectOp {
    /// Generic effect call `<receiver-path>.<method>`. Receiver path is
    /// split (`["fs", "read"]`, `["net", "get"]`, `["model", "embed"]`,
    /// etc.). Method-bearing capabilities also lower here.
    GenericCall { path: Vec<String>, method: String },
}

// ----- Rvalues -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Rvalue {
    Use(Operand),
    Const(Const),
    BinOp(BinOp, Operand, Operand),
    UnOp(UnOp, Operand),
    /// Take a reference to `place`.
    Ref {
        mutable: bool,
        place: Place,
    },
    /// Dereference.
    Deref(Operand),
    /// Construct an ADT value (struct OR enum). `variant` is `0` for
    /// structs (whose AdtDef has a single variant).
    AdtInit {
        adt: AdtId,
        variant: usize,
        fields: Vec<Operand>,
    },
    TupleInit(Vec<Operand>),
    ArrayInit(Vec<Operand>),
    /// Field by index. Names already resolved at lower-time.
    FieldRead {
        receiver: Place,
        field: usize,
    },
    TupleRead {
        receiver: Place,
        idx: usize,
    },
    /// `receiver[index]`.
    IndexRead {
        receiver: Place,
        index: Operand,
    },
    /// Call a static fn (user or builtin).
    Call {
        func: FnRef,
        args: Vec<Operand>,
    },
    /// Dispatch a method dynamically.
    MethodCall {
        receiver: Operand,
        method: String,
        args: Vec<Operand>,
    },
    /// `spawn AgentName(args)` — synchronous slice-6 form.
    AgentSpawn {
        agent: AgentIrId,
        args: Vec<Operand>,
    },
    /// `target!Msg(args)` — fire-and-forget; result is Unit.
    Send {
        target: Operand,
        msg: String,
        args: Vec<Operand>,
    },
    /// `target?Msg(args) @ deadline?` — synchronous reply.
    Ask {
        target: Operand,
        msg: String,
        args: Vec<Operand>,
        deadline_ms: Option<u64>,
    },
    /// `Cap` literal — appears when a capability flows through the
    /// program (typically via a parameter, but constructors land here
    /// when narrowed).
    CapValue {
        family: CapFamily,
        constraint: CapConstraint,
    },
    /// Numeric / pointer cast.
    Cast {
        src: Operand,
        ty: IrTy,
    },
}

// ----- Statements ----------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `local := rvalue`.
    Assign(Place, Rvalue),
    /// Conceptual drop of an owned value at scope exit.
    Drop(Local),
    /// Liveness markers; the interpreter ignores these but the dump
    /// uses them so SIR snapshots match MIR conventions.
    StorageLive(Local),
    StorageDead(Local),
    ArenaPush(ArenaId),
    ArenaPop(ArenaId),
    /// Direct effect-system invocation. Result placed in `out` if
    /// non-None; otherwise discarded.
    EffectInvoke {
        effect: EffectId,
        op: EffectOp,
        args: Vec<Operand>,
        out: Option<Place>,
    },
    /// No-op kept for terminator-elision passes.
    Nop,
}

// ----- Terminators ---------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Term {
    Goto(BlockId),
    If {
        cond: Operand,
        then: BlockId,
        else_: BlockId,
    },
    SwitchInt {
        discr: Operand,
        arms: Vec<(i128, BlockId)>,
        default: BlockId,
    },
    SwitchVariant {
        discr: Operand,
        adt: AdtId,
        arms: Vec<(usize, BlockId)>,
        default: BlockId,
    },
    Return(Operand),
    Panic {
        msg: Operand,
    },
    Unreachable,
    /// Build `Result::Err(payload)` and return. Used by `?` lowering.
    TryReturnErr(Operand),
    /// Async suspension point — slice-7 runtime resumes execution at
    /// `resume`. Slice 6 traps with MT5009 if hit.
    Suspend {
        resume: BlockId,
    },
}

// ----- Binary / unary ops --------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

// ----- Blocks --------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Block {
    pub id: BlockId,
    pub stmts: Vec<Stmt>,
    pub terminator: Term,
}

// ----- Functions -----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Function {
    pub id: IrFnId,
    pub name: String,
    /// Parameter locals (always the first `params.len()` entries in
    /// `locals`; local `_0` is reserved for the return slot, so the
    /// parameter region runs from `_1..=_params.len()`).
    pub params: Vec<Local>,
    pub locals: Vec<LocalDecl>,
    pub blocks: Vec<Block>,
    pub entry: BlockId,
    pub ret_ty: IrTy,
    pub effects: Vec<EffectId>,
    pub hir_fn: Option<FnId>,
    pub span: SourceSpan,
}

impl Function {
    pub fn block(&self, id: BlockId) -> &Block {
        &self.blocks[id.0 as usize]
    }
    pub fn block_mut(&mut self, id: BlockId) -> &mut Block {
        &mut self.blocks[id.0 as usize]
    }
}

// ----- ADTs ----------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AdtRef {
    pub adt: AdtId,
    pub name: String,
    pub kind: AdtRefKind,
    pub variants: Vec<VariantRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdtRefKind {
    Struct,
    Enum,
    /// Opaque — the prelude built this. The interpreter cannot
    /// construct values of this ADT directly.
    Opaque,
}

#[derive(Debug, Clone)]
pub struct VariantRef {
    pub name: String,
    pub fields: Vec<FieldRef>,
}

#[derive(Debug, Clone)]
pub struct FieldRef {
    /// `None` for tuple variants.
    pub name: Option<String>,
    pub ty: IrTy,
}

// ----- Agents --------------------------------------------------------------

/// Slice-6 agent metadata. Each agent compiles to a backing struct (in
/// `AdtRef` form) plus one constructor fn and one handler fn per
/// declared `on Msg(...)` clause.
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: AgentIrId,
    pub name: String,
    /// Synthesized backing struct: each state field becomes a struct
    /// field. The interpreter mutates these in place.
    pub state_adt: AdtId,
    /// Constructor fn id (zero-arg form returning the initialized
    /// state struct). Slice 6 ignores `ctor_params` — the prelude's
    /// permissive method table handles cap-style constructors.
    pub ctor: IrFnId,
    /// `msg_name -> handler_fn_id`.
    pub handlers: Vec<(String, IrFnId)>,
    pub span: SourceSpan,
}

// ----- Program -------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct Program {
    pub fns: Vec<Function>,
    pub adts: Vec<AdtRef>,
    pub agents: Vec<Agent>,
    /// Lowering errors. The lowerer is *lenient*: rather than panic
    /// on un-lowerable shapes (typically caused by `Error`-typed
    /// expressions) it records a note and substitutes a const Unit.
    /// Run-only pipelines refuse to execute a program with errors.
    pub errors: Vec<String>,
}

impl Program {
    pub fn fn_by_name(&self, name: &str) -> Option<&Function> {
        self.fns.iter().find(|f| f.name == name)
    }
    pub fn fn_by_id(&self, id: IrFnId) -> &Function {
        &self.fns[id.0 as usize]
    }
    pub fn agent_by_id(&self, id: AgentIrId) -> &Agent {
        &self.agents[id.0 as usize]
    }
    pub fn agent_by_name(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.name == name)
    }
    pub fn adt_by_id(&self, id: AdtId) -> Option<&AdtRef> {
        self.adts.iter().find(|a| a.adt == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_program_round_trips() {
        let p = Program::default();
        assert!(p.fns.is_empty());
        assert!(p.adts.is_empty());
        assert!(p.agents.is_empty());
    }

    #[test]
    fn place_local_constructor() {
        let p = Place::local(Local(3));
        assert_eq!(p.local, Local(3));
        assert!(p.proj.is_empty());
    }

    #[test]
    fn const_displayability() {
        // Just exercise the variants so future Display changes don't
        // silently break the enum.
        let _ = Const::Int(42, IntKind::I32);
        let _ = Const::Bool(true);
        let _ = Const::Str("hello".into());
        let _ = Const::Unit;
    }
}
