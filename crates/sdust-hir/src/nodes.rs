use crate::ids::*;
use la_arena::Arena;

#[derive(Debug, Clone)]
pub struct SourceSpan {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone)]
pub enum Item {
    Fn(FnId),
    Struct(StructId),
    Enum(EnumId),
    TypeAlias(TypeAliasId),
    Agent(AgentId),
    Protocol(ProtocolId),
    Supervisor(SupervisorId),
    Use(HirUse),
    Mod(HirMod),
    ExternBlock(HirExternBlock),
    ExportDecl(HirExportDecl),
    Macro(HirMacro),
    Impl(HirImpl),
    Trait(HirTrait),
    Const(HirConst),
    /// Slice 5: top-level `sandbox Name with { entries } { body }`.
    Sandbox(HirTopSandbox),
}

/// Top-level sandbox declaration (spec §16.1). Same fields as the
/// expression-position sandbox in `HirExpr::Sandbox`, plus a name.
#[derive(Debug, Clone)]
pub struct HirTopSandbox {
    pub name: String,
    pub entries: Vec<(Vec<String>, ExprId)>,
    pub body: BlockId,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct HirFn {
    pub name: String,
    pub is_pub: bool,
    pub is_unsafe: bool,
    pub generics: Vec<String>,
    pub params: Vec<HirParam>,
    pub ret: Option<TypeId>,
    pub effects: Vec<String>,
    pub body: Option<BlockId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct HirParam {
    pub name: String,
    pub ty: Option<TypeId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct HirStruct {
    pub name: String,
    pub is_pub: bool,
    pub generics: Vec<String>,
    pub fields: Vec<HirStructField>,
    /// Slice 5: derived trait names (`Copy`, `Hash`, `Eq`).
    pub derives: Vec<String>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirStructField {
    pub name: String,
    pub ty: TypeId,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct HirEnum {
    pub name: String,
    pub is_pub: bool,
    pub generics: Vec<String>,
    pub variants: Vec<HirEnumVariant>,
    /// Slice 5: derived trait names (`Copy`, `Hash`, `Eq`).
    pub derives: Vec<String>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirEnumVariant {
    pub name: String,
    pub payload: Vec<TypeId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct HirTypeAlias {
    pub name: String,
    pub is_pub: bool,
    pub generics: Vec<String>,
    pub ty: TypeId,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct HirAgent {
    pub name: String,
    pub ctor_params: Vec<String>,
    pub protocols: Vec<TypeId>,
    pub state: Vec<HirAgentState>,
    pub handlers: Vec<HirOnHandler>,
    pub methods: Vec<FnId>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirAgentState {
    pub name: String,
    pub ty: Option<TypeId>,
    pub init: Option<ExprId>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirOnHandler {
    pub message: String,
    pub params: Vec<String>,
    pub body: BlockId,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct HirProtocol {
    pub name: String,
    pub is_pub: bool,
    pub version: Option<u32>,
    pub composition: Option<Vec<TypeId>>, // for `protocol Web = A + B + C`
    pub messages: Vec<HirProtocolMsg>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirProtocolMsg {
    pub name: String,
    pub params: Vec<HirParam>,
    pub reply: Option<TypeId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct HirSupervisor {
    pub name: String,
    pub strategy: String,
    pub children: Vec<(String, ExprId)>,
    pub on_fail: Vec<(String, Vec<HirSupAction>)>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub enum HirSupAction {
    Restart {
        up_to: Option<u32>,
        in_dur: Option<ExprId>,
    },
    Backoff {
        lo: ExprId,
        hi: ExprId,
    },
}

#[derive(Debug, Clone)]
pub struct HirUse {
    pub path: Vec<String>,
    pub alias: Option<String>,
    pub leaves: Vec<(String, Option<String>)>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirMod {
    pub path: Vec<String>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirExternBlock {
    pub abi: Option<String>,
    pub fns: Vec<FnId>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirExportDecl {
    pub abi: Option<String>,
    pub item: Box<Item>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirMacro {
    pub name: String,
    pub params: Vec<String>,
    pub body_tokens: String,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirImpl {
    pub trait_for: Option<TypeId>,
    pub self_ty: TypeId,
    pub methods: Vec<FnId>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirTrait {
    pub name: String,
    pub is_pub: bool,
    pub generics: Vec<String>,
    pub methods: Vec<FnId>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirConst {
    pub name: String,
    pub is_pub: bool,
    pub ty: TypeId,
    pub value: ExprId,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub enum HirType {
    Path {
        segments: Vec<String>,
        generics: Vec<TypeId>,
    },
    Borrow {
        mutable: bool,
        inner: TypeId,
    },
    Tuple(Vec<TypeId>),
    Array {
        elem: TypeId,
        len: Option<ExprId>,
    },
    Fn {
        params: Vec<TypeId>,
        ret: Option<TypeId>,
    },
    /// Sugar: T!E desugared to Result[T, E]; we preserve original for fmt.
    Result {
        ok: TypeId,
        err: TypeId,
    },
    /// T!{A,B} desugared to Result[T, A|B]
    Union(Vec<TypeId>),
    /// `dyn Trait` — dynamic dispatch type. Slice 5 keeps the trait
    /// name as a single identifier (no generic args on the trait).
    Dyn {
        trait_name: String,
    },
    Unit,
    Unknown,
}

#[derive(Debug, Clone)]
pub enum HirExpr {
    Literal(HirLiteral),
    Path(Vec<String>),
    Call {
        callee: ExprId,
        args: Vec<HirArg>,
    },
    MethodCall {
        receiver: ExprId,
        method: String,
        args: Vec<HirArg>,
    },
    Field {
        receiver: ExprId,
        name: String,
    },
    Index {
        receiver: ExprId,
        idx: ExprId,
    },
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Unary {
        op: UnOp,
        rhs: ExprId,
    },
    If {
        cond: ExprId,
        then: BlockId,
        else_: Option<ExprId>,
    },
    Match {
        scrutinee: ExprId,
        arms: Vec<HirMatchArm>,
    },
    For {
        pat: PatId,
        iter: ExprId,
        body: BlockId,
    },
    While {
        cond: ExprId,
        body: BlockId,
    },
    Loop {
        body: BlockId,
    },
    Return(Option<ExprId>),
    Block(BlockId),
    Tuple(Vec<ExprId>),
    Array(Vec<ExprId>),
    Struct {
        path: Vec<String>,
        fields: Vec<(String, ExprId)>,
    },
    Map(Vec<(ExprId, ExprId)>),
    /// `target!Msg(args)`
    Send {
        target: ExprId,
        msg: String,
        args: Vec<HirArg>,
    },
    /// `target?Msg(args)`
    Ask {
        target: ExprId,
        msg: String,
        args: Vec<HirArg>,
    },
    /// `expr @ duration`
    Deadline {
        inner: ExprId,
        dur: ExprId,
    },
    Question(ExprId),
    Move(ExprId),
    Borrow {
        mutable: bool,
        inner: ExprId,
    },
    Spawn {
        is_task: bool,
        inner: ExprId,
    },
    Detach(ExprId),
    Join(ExprId),
    HtmlTemplate(String),
    Unsafe(BlockId),
    Arena {
        name: String,
        body: ExprId,
    },
    TaskScope {
        deadline: Option<ExprId>,
        body: BlockId,
    },
    Budget {
        entries: Vec<(String, ExprId)>,
        body: ExprId,
    },
    Sandbox {
        name: String,
        entries: Vec<(Vec<String>, ExprId)>,
        body: BlockId,
    },
    Cast {
        lhs: ExprId,
        ty: TypeId,
    },
    /// `fn(params) -> ret { body }` lambda. Distinct from `HirFn` (item-level).
    Lambda {
        params: Vec<HirParam>,
        ret: Option<TypeId>,
        body: BlockId,
    },
    /// `if let Pattern = scrutinee { then } else { else_ }`. Slice 2 keeps
    /// this as its own variant rather than desugaring to `match` so the
    /// formatter (which works off CST) and the future type checker can
    /// reason about it directly.
    IfLet {
        pat: PatId,
        scrutinee: ExprId,
        then: BlockId,
        else_: Option<ExprId>,
    },
    /// `run <expr>` — leading-keyword expression used in sandbox bodies
    /// (spec §16.1). Slice 2 does not constrain where it can appear;
    /// slice 3's type checker will restrict it.
    Run(ExprId),
    /// `Path::[T1, T2]` (turbofish) in expression position. The generics
    /// apply to the final segment. Distinct from the plain `Path` variant
    /// so the type checker can resolve the explicit type arguments.
    PathGeneric {
        segments: Vec<String>,
        generics: Vec<TypeId>,
    },
    Error,
}

#[derive(Debug, Clone)]
pub struct HirArg {
    pub name: Option<String>,
    pub value: ExprId,
}

#[derive(Debug, Clone)]
pub enum HirLiteral {
    Int(i128, Option<String>), // value + optional type suffix
    Float(f64, Option<String>),
    Str(String),
    Char(char),
    Bool(bool),
    Duration { value: u64, unit: String },
    Size { value: u64, unit: String },
}

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
    Range,
    RangeEq,
    Assign,
    AssignAdd,
    AssignSub,
    AssignMul,
    AssignDiv,
    AssignRem,
    AssignBitAnd,
    AssignBitOr,
    AssignBitXor,
    AssignShl,
    AssignShr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Deref,
}

#[derive(Debug, Clone)]
pub enum HirPat {
    Wildcard,
    Literal(HirLiteral),
    Binding {
        name: String,
        sub: Option<PatId>,
    },
    Ref {
        mutable: bool,
        inner: PatId,
    },
    Tuple(Vec<PatId>),
    Struct {
        path: Vec<String>,
        fields: Vec<(String, Option<PatId>)>,
    },
    Enum {
        path: Vec<String>,
        args: Vec<PatId>,
    },
    Range {
        lo: PatId,
        hi: PatId,
        inclusive: bool,
    },
}

#[derive(Debug, Clone)]
pub struct HirMatchArm {
    pub pat: PatId,
    pub guard: Option<ExprId>,
    pub body: ExprId,
}

#[derive(Debug, Clone)]
pub struct HirBlock {
    pub stmts: Vec<HirStmt>,
    pub tail: Option<ExprId>,
}

#[derive(Debug, Clone)]
pub enum HirStmt {
    Let {
        pat: PatId,
        ty: Option<TypeId>,
        init: Option<ExprId>,
        /// `let mut ...` declares the binding(s) as mutable.
        mutable: bool,
    },
    Expr(ExprId),
}

#[derive(Debug, Clone)]
pub struct HirLocal {
    pub name: String,
    pub mutable: bool,
    pub span: SourceSpan,
}

#[derive(Default, Debug)]
pub struct Package {
    pub items: Arena<Item>,
    pub fns: Arena<HirFn>,
    pub structs: Arena<HirStruct>,
    pub enums: Arena<HirEnum>,
    pub type_aliases: Arena<HirTypeAlias>,
    pub agents: Arena<HirAgent>,
    pub protocols: Arena<HirProtocol>,
    pub supervisors: Arena<HirSupervisor>,
    pub exprs: Arena<HirExpr>,
    pub pats: Arena<HirPat>,
    pub types: Arena<HirType>,
    pub blocks: Arena<HirBlock>,
    pub locals: Arena<HirLocal>,
    pub top_level: Vec<ItemId>,
}
