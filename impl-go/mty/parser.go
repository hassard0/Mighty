package mty

import "fmt"

// Node is the common interface for AST nodes. Children() lets generic
// walkers descend the tree without knowing the concrete types.
//
// Derivation: v1.0-RC §4+. The parser builds a tree of these nodes and
// surfaces diagnostics for malformed input. Pratt precedence is used for
// expressions; recursive descent everywhere else.
type Node interface {
	NodeKind() string
	Span() Span
	Children() []Node
}

// File is the root: a list of top-level items.
type File struct {
	Items []Node
	SpanV Span
}

func (n *File) NodeKind() string { return "File" }
func (n *File) Span() Span       { return n.SpanV }
func (n *File) Children() []Node { return n.Items }

// PackageDecl is `package name`.
type PackageDecl struct {
	Name  string
	SpanV Span
}

func (n *PackageDecl) NodeKind() string { return "PackageDecl" }
func (n *PackageDecl) Span() Span       { return n.SpanV }
func (n *PackageDecl) Children() []Node { return nil }

// UseDecl is `use path`.
type UseDecl struct {
	Path  []string
	SpanV Span
}

func (n *UseDecl) NodeKind() string { return "UseDecl" }
func (n *UseDecl) Span() Span       { return n.SpanV }
func (n *UseDecl) Children() []Node { return nil }

// ModDecl is `mod name;` or `mod name { ... }`.
type ModDecl struct {
	Name  string
	Body  *Block // nil for `mod name;`
	SpanV Span
}

func (n *ModDecl) NodeKind() string { return "ModDecl" }
func (n *ModDecl) Span() Span       { return n.SpanV }
func (n *ModDecl) Children() []Node {
	if n.Body == nil {
		return nil
	}
	return []Node{n.Body}
}

// FnDecl is `fn name[gens](params) -> ret { body }`.
type FnDecl struct {
	Name    string
	Params  []*Param
	Generics []string
	RetType Node // may be nil
	Body    Node // *Block or expression (for `= <expr>` shorthand) or nil
	IsPub   bool
	IsConst bool
	IsAsync bool
	IsUnsafe bool
	IsExport bool
	Effects []string // names from `effect a, b, c` clause
	Requires []Node  // requires <expr> clauses
	SpanV   Span
}

func (n *FnDecl) NodeKind() string { return "FnDecl" }
func (n *FnDecl) Span() Span       { return n.SpanV }
func (n *FnDecl) Children() []Node {
	out := make([]Node, 0, len(n.Params)+2)
	for _, p := range n.Params {
		out = append(out, p)
	}
	if n.RetType != nil {
		out = append(out, n.RetType)
	}
	if n.Body != nil {
		out = append(out, n.Body)
	}
	return out
}

// Param is a function/method parameter `name: Type`.
type Param struct {
	Name  string
	Type  Node // may be nil for inferred
	SpanV Span
}

func (n *Param) NodeKind() string { return "Param" }
func (n *Param) Span() Span       { return n.SpanV }
func (n *Param) Children() []Node {
	if n.Type == nil {
		return nil
	}
	return []Node{n.Type}
}

// StructDecl is `struct Name { field: T, ... }`.
type StructDecl struct {
	Name   string
	Fields []*Field
	IsPub  bool
	SpanV  Span
}

func (n *StructDecl) NodeKind() string { return "StructDecl" }
func (n *StructDecl) Span() Span       { return n.SpanV }
func (n *StructDecl) Children() []Node {
	out := make([]Node, len(n.Fields))
	for i, f := range n.Fields {
		out[i] = f
	}
	return out
}

// Field is a struct field declaration.
type Field struct {
	Name  string
	Type  Node
	SpanV Span
}

func (n *Field) NodeKind() string { return "Field" }
func (n *Field) Span() Span       { return n.SpanV }
func (n *Field) Children() []Node { return []Node{n.Type} }

// EnumDecl is `enum Name { Variant(T), Variant { ... } }`.
type EnumDecl struct {
	Name     string
	Variants []*Variant
	IsPub    bool
	SpanV    Span
}

func (n *EnumDecl) NodeKind() string { return "EnumDecl" }
func (n *EnumDecl) Span() Span       { return n.SpanV }
func (n *EnumDecl) Children() []Node {
	out := make([]Node, len(n.Variants))
	for i, v := range n.Variants {
		out[i] = v
	}
	return out
}

// Variant is one enum variant.
type Variant struct {
	Name   string
	Tuple  []Node    // payload types if `Foo(A, B)`
	Fields []*Field  // record-style payload if `Foo { x: A, y: B }`
	SpanV  Span
}

func (n *Variant) NodeKind() string { return "Variant" }
func (n *Variant) Span() Span       { return n.SpanV }
func (n *Variant) Children() []Node {
	out := append([]Node{}, n.Tuple...)
	for _, f := range n.Fields {
		out = append(out, f)
	}
	return out
}

// TypeAlias is `type Name = T`.
type TypeAlias struct {
	Name   string
	Target Node
	SpanV  Span
}

func (n *TypeAlias) NodeKind() string { return "TypeAlias" }
func (n *TypeAlias) Span() Span       { return n.SpanV }
func (n *TypeAlias) Children() []Node { return []Node{n.Target} }

// ConstDecl is `const NAME: T = expr`.
type ConstDecl struct {
	Name  string
	Type  Node
	Value Node
	SpanV Span
}

func (n *ConstDecl) NodeKind() string { return "ConstDecl" }
func (n *ConstDecl) Span() Span       { return n.SpanV }
func (n *ConstDecl) Children() []Node {
	out := []Node{}
	if n.Type != nil {
		out = append(out, n.Type)
	}
	if n.Value != nil {
		out = append(out, n.Value)
	}
	return out
}

// TraitDecl is `trait Name { fn ... }`.
type TraitDecl struct {
	Name  string
	Items []Node
	IsPub bool
	SpanV Span
}

func (n *TraitDecl) NodeKind() string { return "TraitDecl" }
func (n *TraitDecl) Span() Span       { return n.SpanV }
func (n *TraitDecl) Children() []Node { return n.Items }

// ImplDecl is `impl Trait for Type { ... }` or `impl Type { ... }`.
type ImplDecl struct {
	TraitName string // empty for inherent impl
	TargetType Node
	Items     []Node
	SpanV     Span
}

func (n *ImplDecl) NodeKind() string { return "ImplDecl" }
func (n *ImplDecl) Span() Span       { return n.SpanV }
func (n *ImplDecl) Children() []Node {
	out := []Node{}
	if n.TargetType != nil {
		out = append(out, n.TargetType)
	}
	return append(out, n.Items...)
}

// AgentDecl is `agent Name(ctorArgs): Proto1, Proto2 { ... }`.
type AgentDecl struct {
	Name      string
	CtorArgs  []*Param
	Protocols []string
	Items     []Node
	SpanV     Span
}

func (n *AgentDecl) NodeKind() string { return "AgentDecl" }
func (n *AgentDecl) Span() Span       { return n.SpanV }
func (n *AgentDecl) Children() []Node {
	out := []Node{}
	for _, p := range n.CtorArgs {
		out = append(out, p)
	}
	return append(out, n.Items...)
}

// ProtocolDecl is `protocol Name { Msg(args) -> Ret }`.
type ProtocolDecl struct {
	Name     string
	Messages []*ProtoMsg
	SpanV    Span
}

func (n *ProtocolDecl) NodeKind() string { return "ProtocolDecl" }
func (n *ProtocolDecl) Span() Span       { return n.SpanV }
func (n *ProtocolDecl) Children() []Node {
	out := make([]Node, len(n.Messages))
	for i, m := range n.Messages {
		out[i] = m
	}
	return out
}

// ProtoMsg is one protocol message slot.
type ProtoMsg struct {
	Name    string
	Params  []*Param
	RetType Node
	SpanV   Span
}

func (n *ProtoMsg) NodeKind() string { return "ProtoMsg" }
func (n *ProtoMsg) Span() Span       { return n.SpanV }
func (n *ProtoMsg) Children() []Node {
	out := []Node{}
	for _, p := range n.Params {
		out = append(out, p)
	}
	if n.RetType != nil {
		out = append(out, n.RetType)
	}
	return out
}

// SupervisorDecl is `supervisor Name(strategy: x) { ... }`.
type SupervisorDecl struct {
	Name   string
	Args   []*Param
	Items  []Node
	SpanV  Span
}

func (n *SupervisorDecl) NodeKind() string { return "SupervisorDecl" }
func (n *SupervisorDecl) Span() Span       { return n.SpanV }
func (n *SupervisorDecl) Children() []Node {
	out := []Node{}
	for _, p := range n.Args {
		out = append(out, p)
	}
	return append(out, n.Items...)
}

// ExternBlock is `extern { ... }` or `extern c { ... }` etc.
type ExternBlock struct {
	ABI   string // empty, "c", "js", "component"
	Items []Node
	SpanV Span
}

func (n *ExternBlock) NodeKind() string { return "ExternBlock" }
func (n *ExternBlock) Span() Span       { return n.SpanV }
func (n *ExternBlock) Children() []Node { return n.Items }

// MacroDecl is `macro name(params) => { body }` or `macro name(params) { body }`.
type MacroDecl struct {
	Name   string
	Params []string
	Body   Node // expression or block
	IsProc bool
	IsPub  bool
	SpanV  Span
}

func (n *MacroDecl) NodeKind() string { return "MacroDecl" }
func (n *MacroDecl) Span() Span       { return n.SpanV }
func (n *MacroDecl) Children() []Node {
	if n.Body == nil {
		return nil
	}
	return []Node{n.Body}
}

// OnHandler is `on MsgName(args) { ... }` or `on MsgName(args) -> expr`.
type OnHandler struct {
	MsgName string
	Params  []*Param
	RetType Node
	Body    Node
	SpanV   Span
}

func (n *OnHandler) NodeKind() string { return "OnHandler" }
func (n *OnHandler) Span() Span       { return n.SpanV }
func (n *OnHandler) Children() []Node {
	out := []Node{}
	for _, p := range n.Params {
		out = append(out, p)
	}
	if n.RetType != nil {
		out = append(out, n.RetType)
	}
	if n.Body != nil {
		out = append(out, n.Body)
	}
	return out
}

// AgentStateField is `name = expr` or `name: T = expr` inside an agent body.
type AgentStateField struct {
	Name  string
	Type  Node // optional
	Value Node
	SpanV Span
}

func (n *AgentStateField) NodeKind() string { return "AgentStateField" }
func (n *AgentStateField) Span() Span       { return n.SpanV }
func (n *AgentStateField) Children() []Node {
	out := []Node{}
	if n.Type != nil {
		out = append(out, n.Type)
	}
	if n.Value != nil {
		out = append(out, n.Value)
	}
	return out
}

// SupervisorClause covers `child name = expr`, `on_fail(name) { ... }`,
// `restart up_to N in DUR`, `backoff lo..hi`.
type SupervisorClause struct {
	Kind  string // "child" | "on_fail" | "restart" | "backoff"
	Name  string
	Items []Node
	Args  []Node
	SpanV Span
}

func (n *SupervisorClause) NodeKind() string { return "SupervisorClause:" + n.Kind }
func (n *SupervisorClause) Span() Span       { return n.SpanV }
func (n *SupervisorClause) Children() []Node { return append(append([]Node{}, n.Args...), n.Items...) }

// Attribute is `#[derive(Trait)]` or any `#[name(...)]`.
type Attribute struct {
	Name  string
	Args  []string
	SpanV Span
}

func (n *Attribute) NodeKind() string { return "Attribute" }
func (n *Attribute) Span() Span       { return n.SpanV }
func (n *Attribute) Children() []Node { return nil }

// ----- Types -----

// PathType is a type-position path like `Vec[T]` or `mod.Type[K, V]`.
type PathType struct {
	Segments []string
	Generics []Node
	SpanV    Span
}

func (n *PathType) NodeKind() string { return "PathType" }
func (n *PathType) Span() Span       { return n.SpanV }
func (n *PathType) Children() []Node { return n.Generics }

// RefType is `&T` or `&mut T`.
type RefType struct {
	Mut   bool
	Inner Node
	SpanV Span
}

func (n *RefType) NodeKind() string { return "RefType" }
func (n *RefType) Span() Span       { return n.SpanV }
func (n *RefType) Children() []Node { return []Node{n.Inner} }

// PtrType is `*T` or `*mut T`.
type PtrType struct {
	Mut   bool
	Inner Node
	SpanV Span
}

func (n *PtrType) NodeKind() string { return "PtrType" }
func (n *PtrType) Span() Span       { return n.SpanV }
func (n *PtrType) Children() []Node { return []Node{n.Inner} }

// TupleType is `(A, B, C)`.
type TupleType struct {
	Elements []Node
	SpanV    Span
}

func (n *TupleType) NodeKind() string { return "TupleType" }
func (n *TupleType) Span() Span       { return n.SpanV }
func (n *TupleType) Children() []Node { return n.Elements }

// ArrayType is `[T; N]` or `[T]`.
type ArrayType struct {
	Elem  Node
	Size  Node // optional
	SpanV Span
}

func (n *ArrayType) NodeKind() string { return "ArrayType" }
func (n *ArrayType) Span() Span       { return n.SpanV }
func (n *ArrayType) Children() []Node {
	out := []Node{n.Elem}
	if n.Size != nil {
		out = append(out, n.Size)
	}
	return out
}

// FnType is `fn(A, B) -> C`.
type FnType struct {
	Params []Node
	Ret    Node
	SpanV  Span
}

func (n *FnType) NodeKind() string { return "FnType" }
func (n *FnType) Span() Span       { return n.SpanV }
func (n *FnType) Children() []Node {
	out := append([]Node{}, n.Params...)
	if n.Ret != nil {
		out = append(out, n.Ret)
	}
	return out
}

// BangType is the `T!E` Result sugar or `T!{A, B}` anonymous-union sugar.
type BangType struct {
	OK    Node
	Err   Node   // nil if Errs non-empty
	Errs  []Node // non-nil for `T!{A, B}`
	SpanV Span
}

func (n *BangType) NodeKind() string { return "BangType" }
func (n *BangType) Span() Span       { return n.SpanV }
func (n *BangType) Children() []Node {
	out := []Node{n.OK}
	if n.Err != nil {
		out = append(out, n.Err)
	}
	out = append(out, n.Errs...)
	return out
}

// ----- Expressions -----

// Block is `{ stmt; stmt; expr }`.
type Block struct {
	Stmts []Node
	SpanV Span
}

func (n *Block) NodeKind() string { return "Block" }
func (n *Block) Span() Span       { return n.SpanV }
func (n *Block) Children() []Node { return n.Stmts }

// LetStmt is `let pat: T = expr`.
type LetStmt struct {
	Pattern Node
	Type    Node
	Value   Node
	IsMut   bool
	SpanV   Span
}

func (n *LetStmt) NodeKind() string { return "LetStmt" }
func (n *LetStmt) Span() Span       { return n.SpanV }
func (n *LetStmt) Children() []Node {
	out := []Node{n.Pattern}
	if n.Type != nil {
		out = append(out, n.Type)
	}
	if n.Value != nil {
		out = append(out, n.Value)
	}
	return out
}

// ExprStmt wraps an expression used as a statement.
type ExprStmt struct {
	Expr  Node
	SpanV Span
}

func (n *ExprStmt) NodeKind() string { return "ExprStmt" }
func (n *ExprStmt) Span() Span       { return n.SpanV }
func (n *ExprStmt) Children() []Node { return []Node{n.Expr} }

// LitExpr is a literal expression (int, float, string, char, bool, unit).
type LitExpr struct {
	Kind  string // "int" | "float" | "string" | "char" | "bool" | "duration" | "size" | "html"
	Text  string
	SpanV Span
}

func (n *LitExpr) NodeKind() string { return "Lit:" + n.Kind }
func (n *LitExpr) Span() Span       { return n.SpanV }
func (n *LitExpr) Children() []Node { return nil }

// PathExpr is `a.b.c` (segments) or with turbofish `a::[T, U]`.
type PathExpr struct {
	Segments []string
	Generics []Node
	SpanV    Span
}

func (n *PathExpr) NodeKind() string { return "PathExpr" }
func (n *PathExpr) Span() Span       { return n.SpanV }
func (n *PathExpr) Children() []Node { return n.Generics }

// BinExpr is `lhs op rhs`.
type BinExpr struct {
	Op    string
	LHS   Node
	RHS   Node
	SpanV Span
}

func (n *BinExpr) NodeKind() string { return "Bin:" + n.Op }
func (n *BinExpr) Span() Span       { return n.SpanV }
func (n *BinExpr) Children() []Node { return []Node{n.LHS, n.RHS} }

// UnaryExpr is `op expr` (prefix).
type UnaryExpr struct {
	Op    string
	Inner Node
	SpanV Span
}

func (n *UnaryExpr) NodeKind() string { return "Unary:" + n.Op }
func (n *UnaryExpr) Span() Span       { return n.SpanV }
func (n *UnaryExpr) Children() []Node { return []Node{n.Inner} }

// PostfixExpr is `expr op` for `?` / `!`.
type PostfixExpr struct {
	Op    string
	Inner Node
	SpanV Span
}

func (n *PostfixExpr) NodeKind() string { return "Post:" + n.Op }
func (n *PostfixExpr) Span() Span       { return n.SpanV }
func (n *PostfixExpr) Children() []Node { return []Node{n.Inner} }

// CallExpr is `callee(arg, arg)`.
type CallExpr struct {
	Callee Node
	Args   []Node
	SpanV  Span
}

func (n *CallExpr) NodeKind() string { return "Call" }
func (n *CallExpr) Span() Span       { return n.SpanV }
func (n *CallExpr) Children() []Node { return append([]Node{n.Callee}, n.Args...) }

// IndexExpr is `receiver[index]`.
type IndexExpr struct {
	Receiver Node
	Index    Node
	SpanV    Span
}

func (n *IndexExpr) NodeKind() string { return "Index" }
func (n *IndexExpr) Span() Span       { return n.SpanV }
func (n *IndexExpr) Children() []Node { return []Node{n.Receiver, n.Index} }

// FieldExpr is `receiver.field`.
type FieldExpr struct {
	Receiver Node
	Field    string
	SpanV    Span
}

func (n *FieldExpr) NodeKind() string { return "Field:" + n.Field }
func (n *FieldExpr) Span() Span       { return n.SpanV }
func (n *FieldExpr) Children() []Node { return []Node{n.Receiver} }

// SendExpr is `target!Msg(args)` (A12).
type SendExpr struct {
	Target Node
	Msg    string
	Args   []Node
	SpanV  Span
}

func (n *SendExpr) NodeKind() string { return "Send:" + n.Msg }
func (n *SendExpr) Span() Span       { return n.SpanV }
func (n *SendExpr) Children() []Node { return append([]Node{n.Target}, n.Args...) }

// AskExpr is `target?Msg(args)` (A12).
type AskExpr struct {
	Target Node
	Msg    string
	Args   []Node
	SpanV  Span
}

func (n *AskExpr) NodeKind() string { return "Ask:" + n.Msg }
func (n *AskExpr) Span() Span       { return n.SpanV }
func (n *AskExpr) Children() []Node { return append([]Node{n.Target}, n.Args...) }

// DeadlineExpr is `expr @duration`.
type DeadlineExpr struct {
	Inner    Node
	Duration Node
	SpanV    Span
}

func (n *DeadlineExpr) NodeKind() string { return "Deadline" }
func (n *DeadlineExpr) Span() Span       { return n.SpanV }
func (n *DeadlineExpr) Children() []Node { return []Node{n.Inner, n.Duration} }

// IfExpr is `if cond { ... } else { ... }`. `IsLet` flips to `if let pat = expr`.
type IfExpr struct {
	IsLet   bool
	Pattern Node // when IsLet
	Cond    Node
	Then    Node
	Else    Node // optional
	SpanV   Span
}

func (n *IfExpr) NodeKind() string { return "If" }
func (n *IfExpr) Span() Span       { return n.SpanV }
func (n *IfExpr) Children() []Node {
	out := []Node{}
	if n.Pattern != nil {
		out = append(out, n.Pattern)
	}
	out = append(out, n.Cond, n.Then)
	if n.Else != nil {
		out = append(out, n.Else)
	}
	return out
}

// MatchExpr is `match scrut { arms }`.
type MatchExpr struct {
	Scrut Node
	Arms  []*MatchArm
	SpanV Span
}

func (n *MatchExpr) NodeKind() string { return "Match" }
func (n *MatchExpr) Span() Span       { return n.SpanV }
func (n *MatchExpr) Children() []Node {
	out := []Node{n.Scrut}
	for _, a := range n.Arms {
		out = append(out, a)
	}
	return out
}

// MatchArm is one `Pat => body` arm.
type MatchArm struct {
	Pattern Node
	Body    Node
	SpanV   Span
}

func (n *MatchArm) NodeKind() string { return "MatchArm" }
func (n *MatchArm) Span() Span       { return n.SpanV }
func (n *MatchArm) Children() []Node { return []Node{n.Pattern, n.Body} }

// LoopExpr is `loop { ... }`.
type LoopExpr struct {
	Body  *Block
	SpanV Span
}

func (n *LoopExpr) NodeKind() string { return "Loop" }
func (n *LoopExpr) Span() Span       { return n.SpanV }
func (n *LoopExpr) Children() []Node { return []Node{n.Body} }

// WhileExpr is `while cond { ... }`.
type WhileExpr struct {
	Cond  Node
	Body  *Block
	SpanV Span
}

func (n *WhileExpr) NodeKind() string { return "While" }
func (n *WhileExpr) Span() Span       { return n.SpanV }
func (n *WhileExpr) Children() []Node { return []Node{n.Cond, n.Body} }

// ForExpr is `for pat in iter { ... }`.
type ForExpr struct {
	Pattern Node
	Iter    Node
	Body    *Block
	SpanV   Span
}

func (n *ForExpr) NodeKind() string { return "For" }
func (n *ForExpr) Span() Span       { return n.SpanV }
func (n *ForExpr) Children() []Node { return []Node{n.Pattern, n.Iter, n.Body} }

// ReturnExpr is `return expr?`.
type ReturnExpr struct {
	Value Node
	SpanV Span
}

func (n *ReturnExpr) NodeKind() string { return "Return" }
func (n *ReturnExpr) Span() Span       { return n.SpanV }
func (n *ReturnExpr) Children() []Node {
	if n.Value == nil {
		return nil
	}
	return []Node{n.Value}
}

// BreakExpr is `break expr?`.
type BreakExpr struct {
	Value Node
	SpanV Span
}

func (n *BreakExpr) NodeKind() string { return "Break" }
func (n *BreakExpr) Span() Span       { return n.SpanV }
func (n *BreakExpr) Children() []Node {
	if n.Value == nil {
		return nil
	}
	return []Node{n.Value}
}

// ContinueExpr is `continue`.
type ContinueExpr struct{ SpanV Span }

func (n *ContinueExpr) NodeKind() string { return "Continue" }
func (n *ContinueExpr) Span() Span       { return n.SpanV }
func (n *ContinueExpr) Children() []Node { return nil }

// SpawnExpr is `spawn AgentName(args)`.
type SpawnExpr struct {
	Inner Node // a call or path expression
	SpanV Span
}

func (n *SpawnExpr) NodeKind() string { return "Spawn" }
func (n *SpawnExpr) Span() Span       { return n.SpanV }
func (n *SpawnExpr) Children() []Node { return []Node{n.Inner} }

// ArenaExpr is `arena LABEL { body }` or `arena LABEL: expr`.
type ArenaExpr struct {
	Label string // optional
	Body  Node   // *Block or expression
	SpanV Span
}

func (n *ArenaExpr) NodeKind() string { return "Arena" }
func (n *ArenaExpr) Span() Span       { return n.SpanV }
func (n *ArenaExpr) Children() []Node { return []Node{n.Body} }

// UnsafeExpr is `unsafe { body }`.
type UnsafeExpr struct {
	Body  *Block
	SpanV Span
}

func (n *UnsafeExpr) NodeKind() string { return "Unsafe" }
func (n *UnsafeExpr) Span() Span       { return n.SpanV }
func (n *UnsafeExpr) Children() []Node { return []Node{n.Body} }

// BudgetExpr is `budget { entries... } run { body }` or `... run expr`.
type BudgetExpr struct {
	Entries []*BudgetEntry
	Body    Node
	SpanV   Span
}

func (n *BudgetExpr) NodeKind() string { return "Budget" }
func (n *BudgetExpr) Span() Span       { return n.SpanV }
func (n *BudgetExpr) Children() []Node {
	out := []Node{}
	for _, e := range n.Entries {
		out = append(out, e)
	}
	if n.Body != nil {
		out = append(out, n.Body)
	}
	return out
}

// BudgetEntry is `key = value` or `key value` (§16 RC3 clarification).
type BudgetEntry struct {
	Key   string
	Value Node
	SpanV Span
}

func (n *BudgetEntry) NodeKind() string { return "BudgetEntry:" + n.Key }
func (n *BudgetEntry) Span() Span       { return n.SpanV }
func (n *BudgetEntry) Children() []Node { return []Node{n.Value} }

// SandboxExpr is `sandbox Name with { entries } { body }`.
type SandboxExpr struct {
	Name    string
	Entries []*BudgetEntry
	Body    Node
	SpanV   Span
}

func (n *SandboxExpr) NodeKind() string { return "Sandbox:" + n.Name }
func (n *SandboxExpr) Span() Span       { return n.SpanV }
func (n *SandboxExpr) Children() []Node {
	out := []Node{}
	for _, e := range n.Entries {
		out = append(out, e)
	}
	if n.Body != nil {
		out = append(out, n.Body)
	}
	return out
}

// RunExpr is the top-level `run <expr>` keyword (A5).
type RunExpr struct {
	Inner Node
	SpanV Span
}

func (n *RunExpr) NodeKind() string { return "Run" }
func (n *RunExpr) Span() Span       { return n.SpanV }
func (n *RunExpr) Children() []Node { return []Node{n.Inner} }

// TupleExpr is `(a, b, c)` (zero or 2+ elements; single-element with comma
// is a tuple, without is just a parenthesised expression).
type TupleExpr struct {
	Elements []Node
	SpanV    Span
}

func (n *TupleExpr) NodeKind() string { return "Tuple" }
func (n *TupleExpr) Span() Span       { return n.SpanV }
func (n *TupleExpr) Children() []Node { return n.Elements }

// ArrayExpr is `[a, b, c]` or `[expr; count]`.
type ArrayExpr struct {
	Elements []Node
	SpanV    Span
}

func (n *ArrayExpr) NodeKind() string { return "Array" }
func (n *ArrayExpr) Span() Span       { return n.SpanV }
func (n *ArrayExpr) Children() []Node { return n.Elements }

// StructExpr is `Path { f: v, g: w }`.
type StructExpr struct {
	Path   Node
	Fields []*FieldInit
	SpanV  Span
}

func (n *StructExpr) NodeKind() string { return "Struct" }
func (n *StructExpr) Span() Span       { return n.SpanV }
func (n *StructExpr) Children() []Node {
	out := []Node{n.Path}
	for _, f := range n.Fields {
		out = append(out, f)
	}
	return out
}

// FieldInit is `name: expr` in a struct constructor.
type FieldInit struct {
	Name  string
	Value Node
	SpanV Span
}

func (n *FieldInit) NodeKind() string { return "FieldInit:" + n.Name }
func (n *FieldInit) Span() Span       { return n.SpanV }
func (n *FieldInit) Children() []Node {
	if n.Value == nil {
		return nil
	}
	return []Node{n.Value}
}

// ClosureExpr is `fn(params) -> ret { body }` (anonymous).
type ClosureExpr struct {
	Params []*Param
	Ret    Node
	Body   Node
	SpanV  Span
}

func (n *ClosureExpr) NodeKind() string { return "Closure" }
func (n *ClosureExpr) Span() Span       { return n.SpanV }
func (n *ClosureExpr) Children() []Node {
	out := []Node{}
	for _, p := range n.Params {
		out = append(out, p)
	}
	if n.Ret != nil {
		out = append(out, n.Ret)
	}
	if n.Body != nil {
		out = append(out, n.Body)
	}
	return out
}

// RangeExpr is `a..b` or `a..=b`.
type RangeExpr struct {
	From      Node // optional
	To        Node // optional
	Inclusive bool
	SpanV     Span
}

func (n *RangeExpr) NodeKind() string {
	if n.Inclusive {
		return "RangeInclusive"
	}
	return "Range"
}
func (n *RangeExpr) Span() Span { return n.SpanV }
func (n *RangeExpr) Children() []Node {
	out := []Node{}
	if n.From != nil {
		out = append(out, n.From)
	}
	if n.To != nil {
		out = append(out, n.To)
	}
	return out
}

// MacroCall is `name!(args)` (A90).
type MacroCall struct {
	Name  string
	Args  []Node
	SpanV Span
}

func (n *MacroCall) NodeKind() string { return "Macro:" + n.Name }
func (n *MacroCall) Span() Span       { return n.SpanV }
func (n *MacroCall) Children() []Node { return n.Args }

// ----- Patterns -----

// WildPat is `_`.
type WildPat struct{ SpanV Span }

func (n *WildPat) NodeKind() string { return "Pat:_" }
func (n *WildPat) Span() Span       { return n.SpanV }
func (n *WildPat) Children() []Node { return nil }

// IdentPat is `name` (optionally `mut name` or `ref name`).
type IdentPat struct {
	Name  string
	IsMut bool
	IsRef bool
	SpanV Span
}

func (n *IdentPat) NodeKind() string { return "Pat:" + n.Name }
func (n *IdentPat) Span() Span       { return n.SpanV }
func (n *IdentPat) Children() []Node { return nil }

// LitPat is a literal pattern.
type LitPat struct {
	Lit   *LitExpr
	SpanV Span
}

func (n *LitPat) NodeKind() string { return "PatLit" }
func (n *LitPat) Span() Span       { return n.SpanV }
func (n *LitPat) Children() []Node { return []Node{n.Lit} }

// PathPat is `Foo.Bar(args)` or `Foo { f, g }`.
type PathPat struct {
	Path   []string
	Tuple  []Node    // when constructor-form
	Fields []*FieldInit // when record-form
	IsRecord bool
	SpanV  Span
}

func (n *PathPat) NodeKind() string { return "PathPat" }
func (n *PathPat) Span() Span       { return n.SpanV }
func (n *PathPat) Children() []Node {
	out := append([]Node{}, n.Tuple...)
	for _, f := range n.Fields {
		out = append(out, f)
	}
	return out
}

// RangePat is `lo..hi` or `lo..=hi` in pattern position.
type RangePat struct {
	From      Node
	To        Node
	Inclusive bool
	SpanV     Span
}

func (n *RangePat) NodeKind() string { return "PatRange" }
func (n *RangePat) Span() Span       { return n.SpanV }
func (n *RangePat) Children() []Node { return []Node{n.From, n.To} }

// ======================================================================
//  Parser
// ======================================================================

// Parser is a hand-rolled recursive-descent parser with a Pratt expression
// loop. Tokens are pre-lexed into a slice; trivia is skipped at construction.
type Parser struct {
	toks  []Token
	pos   int
	diags []Diagnostic
	src   string
}

// NewParser constructs a parser from the source (it runs the lexer too).
func NewParser(src string) *Parser {
	tks, ds := Lex(src)
	// Drop trivia tokens (we don't need them for AST construction).
	out := make([]Token, 0, len(tks))
	for _, t := range tks {
		if t.Kind == TK_WHITESPACE || t.Kind == TK_LINE_COMMENT ||
			t.Kind == TK_BLOCK_COMMENT || t.Kind == TK_DOC_COMMENT {
			continue
		}
		out = append(out, t)
	}
	return &Parser{toks: out, src: src, diags: ds}
}

// Parse runs the parser to completion.
func (p *Parser) Parse() (*File, []Diagnostic) {
	file := &File{}
	startSpan := Span{Start: 0, End: 0}
	if len(p.toks) > 0 {
		startSpan.Start = p.toks[0].Start
	}
	for !p.atEOF() {
		item := p.parseItem()
		if item != nil {
			file.Items = append(file.Items, item)
		}
	}
	if len(p.toks) > 0 {
		startSpan.End = p.toks[len(p.toks)-1].End
	}
	file.SpanV = startSpan
	return file, p.diags
}

// ----- token helpers -----

func (p *Parser) atEOF() bool {
	return p.pos >= len(p.toks) || p.toks[p.pos].Kind == TK_EOF
}

func (p *Parser) peek() Token {
	if p.pos < len(p.toks) {
		return p.toks[p.pos]
	}
	return Token{Kind: TK_EOF}
}

func (p *Parser) peekN(n int) Token {
	if p.pos+n < len(p.toks) {
		return p.toks[p.pos+n]
	}
	return Token{Kind: TK_EOF}
}

func (p *Parser) next() Token {
	t := p.peek()
	p.pos++
	return t
}

func (p *Parser) at(k TokenKind) bool {
	return p.peek().Kind == k
}

func (p *Parser) eat(k TokenKind) bool {
	if p.at(k) {
		p.pos++
		return true
	}
	return false
}

func (p *Parser) expect(k TokenKind) Token {
	if p.at(k) {
		return p.next()
	}
	t := p.peek()
	p.diag("MT1001", fmt.Sprintf("expected %s, found %s", k.Name(), t.Kind.Name()), t.Start, t.End)
	return Token{}
}

func (p *Parser) diag(code, msg string, start, end int) {
	p.diags = append(p.diags, Diagnostic{Code: code, Severity: SevError, Message: msg, Span: Span{Start: start, End: end}})
}

// ----- items -----

// parseItem reads one top-level item.
func (p *Parser) parseItem() Node {
	startTok := p.peek()

	// Attributes (#[...]).
	for p.at(TK_HASH) {
		p.parseAttribute()
	}

	// `pub` qualifier.
	isPub := p.eat(TK_KW_PUB)
	if isPub && p.at(TK_LPAREN) {
		// pub(crate) etc — consume to matching ')' .
		p.skipBalanced(TK_LPAREN, TK_RPAREN)
	}

	// `export` qualifier on fn (e.g. `export fn mount(...)`).
	isExport := false
	if p.at(TK_KW_EXPORT) {
		isExport = true
		p.next()
		// `export c fn ...` — accept optional ABI tag (IDENT).
		if p.at(TK_IDENT) {
			p.next() // discard
		}
	}

	// `unsafe` may precede `fn`.
	isUnsafe := p.eat(TK_KW_UNSAFE)

	// `proc macro` opener.
	if p.at(TK_IDENT) && p.peek().Text == "proc" && p.peekN(1).Kind == TK_KW_MACRO {
		return p.parseMacroDecl(isPub, true, startTok.Start)
	}

	switch p.peek().Kind {
	case TK_KW_PACKAGE:
		return p.parsePackage(startTok.Start)
	case TK_KW_USE, TK_KW_IMPORT:
		return p.parseUse(startTok.Start)
	case TK_KW_MOD:
		return p.parseMod(startTok.Start)
	case TK_KW_FN:
		return p.parseFn(isPub, isExport, isUnsafe, false, startTok.Start)
	case TK_KW_CONST:
		// `const fn` vs `const NAME`.
		if p.peekN(1).Kind == TK_KW_FN {
			p.next() // const
			return p.parseFn(isPub, isExport, isUnsafe, true, startTok.Start)
		}
		return p.parseConst(startTok.Start)
	case TK_KW_STRUCT:
		return p.parseStruct(isPub, startTok.Start)
	case TK_KW_ENUM:
		return p.parseEnum(isPub, startTok.Start)
	case TK_KW_TYPE:
		return p.parseTypeAlias(startTok.Start)
	case TK_KW_TRAIT:
		return p.parseTrait(isPub, startTok.Start)
	case TK_KW_IMPL:
		return p.parseImpl(startTok.Start)
	case TK_KW_AGENT:
		return p.parseAgent(startTok.Start)
	case TK_KW_PROTOCOL:
		return p.parseProtocol(startTok.Start)
	case TK_KW_SUP:
		return p.parseSupervisor(startTok.Start)
	case TK_KW_EXTERN:
		return p.parseExtern(startTok.Start)
	case TK_KW_MACRO:
		return p.parseMacroDecl(isPub, false, startTok.Start)
	case TK_IDENT:
		// Contextual: `supervisor` as keyword alias for `sup`.
		if p.peek().Text == "supervisor" {
			p.next()
			// Reuse parseSupervisor body but with the keyword already consumed.
			return p.parseSupervisorTail(startTok.Start)
		}
	}

	// Skip unknown token to recover.
	tok := p.next()
	p.diag("MT1002", fmt.Sprintf("unexpected token %s in item position", tok.Kind.Name()), tok.Start, tok.End)
	return nil
}

// parseAttribute consumes `#[name(...)]`.
func (p *Parser) parseAttribute() *Attribute {
	start := p.peek().Start
	p.expect(TK_HASH)
	p.expect(TK_LBRACK)
	name := p.expect(TK_IDENT).Text
	args := []string{}
	if p.eat(TK_LPAREN) {
		for !p.at(TK_RPAREN) && !p.atEOF() {
			args = append(args, p.peek().Text)
			p.next()
			p.eat(TK_COMMA)
		}
		p.expect(TK_RPAREN)
	}
	end := p.expect(TK_RBRACK).End
	return &Attribute{Name: name, Args: args, SpanV: Span{Start: start, End: end}}
}

func (p *Parser) parsePackage(start int) Node {
	p.expect(TK_KW_PACKAGE)
	parts := []string{}
	if p.at(TK_IDENT) {
		parts = append(parts, p.next().Text)
		for p.eat(TK_DOT) {
			parts = append(parts, p.expect(TK_IDENT).Text)
		}
	}
	end := p.peek().Start
	return &PackageDecl{Name: joinDots(parts), SpanV: Span{Start: start, End: end}}
}

func (p *Parser) parseUse(start int) Node {
	p.next() // use / import
	parts := []string{p.expect(TK_IDENT).Text}
	for p.eat(TK_DOT) {
		if p.at(TK_IDENT) {
			parts = append(parts, p.next().Text)
		} else if p.eat(TK_STAR) {
			parts = append(parts, "*")
		} else {
			break
		}
	}
	// Optional `:colon-separated` for WIT-style imports — left for v1.1 (skip silently).
	end := p.peek().Start
	return &UseDecl{Path: parts, SpanV: Span{Start: start, End: end}}
}

func (p *Parser) parseMod(start int) Node {
	p.expect(TK_KW_MOD)
	name := p.expect(TK_IDENT).Text
	if p.eat(TK_SEMI) {
		return &ModDecl{Name: name, SpanV: Span{Start: start, End: p.peek().Start}}
	}
	body := p.parseBlock()
	return &ModDecl{Name: name, Body: body, SpanV: Span{Start: start, End: body.SpanV.End}}
}

// parseFn handles `fn name [gens] (params) -> ret  effect e, e  requires e  { body }`
// and the `= <expr>` body-shorthand variant.
func (p *Parser) parseFn(isPub, isExport, isUnsafe, isConst bool, start int) Node {
	p.expect(TK_KW_FN)
	name := p.expect(TK_IDENT).Text
	gens := p.parseOptionalGenerics()
	params := p.parseFnParams()
	var ret Node
	if p.eat(TK_ARROW) {
		ret = p.parseType()
	}

	fn := &FnDecl{
		Name:     name,
		Params:   params,
		Generics: gens,
		RetType:  ret,
		IsPub:    isPub,
		IsConst:  isConst,
		IsUnsafe: isUnsafe,
		IsExport: isExport,
	}

	// Optional `effect a, b, c` clause.
	if p.at(TK_KW_EFFECT) {
		p.next()
		fn.Effects = p.parseEffectList()
	}

	// Optional `requires <expr>` clauses (any count).
	for p.at(TK_KW_REQUIRES) {
		p.next()
		fn.Requires = append(fn.Requires, p.parseExpr(0))
	}

	switch {
	case p.at(TK_LBRACE):
		fn.Body = p.parseBlock()
	case p.eat(TK_EQ):
		fn.Body = p.parseExpr(0)
	default:
		// Forward declaration (e.g. `pub unsafe fn _foo(...) requires ...`).
	}
	fn.SpanV = Span{Start: start, End: p.peek().Start}
	return fn
}

func (p *Parser) parseEffectList() []string {
	out := []string{}
	for {
		// Effect names tolerate keyword tokens (§3.3.4).
		t := p.peek()
		if t.Kind == TK_IDENT || isKeywordTokenKind(t.Kind) {
			out = append(out, t.Text)
			p.next()
		} else {
			break
		}
		if !p.eat(TK_COMMA) {
			break
		}
	}
	return out
}

// parseOptionalGenerics consumes `[T, U]` if present.
func (p *Parser) parseOptionalGenerics() []string {
	out := []string{}
	if !p.eat(TK_LBRACK) {
		return out
	}
	for !p.at(TK_RBRACK) && !p.atEOF() {
		t := p.expect(TK_IDENT)
		out = append(out, t.Text)
		// Skip optional bound `T: Trait`.
		if p.eat(TK_COLON) {
			p.parseType()
			for p.eat(TK_PLUS) {
				p.parseType()
			}
		}
		if !p.eat(TK_COMMA) {
			break
		}
	}
	p.expect(TK_RBRACK)
	return out
}

// parseFnParams reads `( name: Type, ... )`.
func (p *Parser) parseFnParams() []*Param {
	out := []*Param{}
	if !p.eat(TK_LPAREN) {
		return out
	}
	for !p.at(TK_RPAREN) && !p.atEOF() {
		out = append(out, p.parseParam())
		if !p.eat(TK_COMMA) {
			break
		}
	}
	p.expect(TK_RPAREN)
	return out
}

func (p *Parser) parseParam() *Param {
	start := p.peek().Start
	// allow `mut name`, `&self`, `self`, `mut self`.
	if p.at(TK_KW_MUT) {
		p.next()
	}
	if p.eat(TK_AMP) {
		// `&self` or `&mut self`
		if p.eat(TK_KW_MUT) {
			// ok
		}
		if p.at(TK_KW_SELF) {
			t := p.next()
			return &Param{Name: "self", SpanV: Span{Start: start, End: t.End}}
		}
	}
	if p.at(TK_KW_SELF) {
		t := p.next()
		return &Param{Name: "self", SpanV: Span{Start: start, End: t.End}}
	}
	name := ""
	if p.at(TK_IDENT) {
		name = p.next().Text
	} else if p.eat(TK_UNDERSCORE) {
		name = "_"
	}
	var typ Node
	if p.eat(TK_COLON) {
		typ = p.parseType()
	}
	return &Param{Name: name, Type: typ, SpanV: Span{Start: start, End: p.peek().Start}}
}

func (p *Parser) parseConst(start int) Node {
	p.expect(TK_KW_CONST)
	name := p.expect(TK_IDENT).Text
	var typ Node
	if p.eat(TK_COLON) {
		typ = p.parseType()
	}
	var val Node
	if p.eat(TK_EQ) {
		val = p.parseExpr(0)
	}
	return &ConstDecl{Name: name, Type: typ, Value: val, SpanV: Span{Start: start, End: p.peek().Start}}
}

// parseStruct accepts both comma-separated and newline-separated field lists
// (RC3 §4 clarification: both accepted).
func (p *Parser) parseStruct(isPub bool, start int) Node {
	p.expect(TK_KW_STRUCT)
	name := p.expect(TK_IDENT).Text
	p.parseOptionalGenerics()
	fields := []*Field{}
	if p.eat(TK_LBRACE) {
		for !p.at(TK_RBRACE) && !p.atEOF() {
			fStart := p.peek().Start
			fn := p.expect(TK_IDENT).Text
			p.expect(TK_COLON)
			ft := p.parseType()
			fields = append(fields, &Field{Name: fn, Type: ft, SpanV: Span{Start: fStart, End: p.peek().Start}})
			// Accept comma OR newline separator (both tokenise away, so we
			// just look for comma or break on `}`).
			if !p.eat(TK_COMMA) {
				if p.at(TK_RBRACE) {
					break
				}
				// allow implicit separator (newline already stripped as WS)
			}
		}
		p.expect(TK_RBRACE)
	}
	return &StructDecl{Name: name, Fields: fields, IsPub: isPub, SpanV: Span{Start: start, End: p.peek().Start}}
}

func (p *Parser) parseEnum(isPub bool, start int) Node {
	p.expect(TK_KW_ENUM)
	name := p.expect(TK_IDENT).Text
	p.parseOptionalGenerics()
	variants := []*Variant{}
	if p.eat(TK_LBRACE) {
		for !p.at(TK_RBRACE) && !p.atEOF() {
			variants = append(variants, p.parseVariant())
			p.eat(TK_COMMA)
		}
		p.expect(TK_RBRACE)
	}
	return &EnumDecl{Name: name, Variants: variants, IsPub: isPub, SpanV: Span{Start: start, End: p.peek().Start}}
}

func (p *Parser) parseVariant() *Variant {
	start := p.peek().Start
	name := p.expect(TK_IDENT).Text
	v := &Variant{Name: name}
	if p.eat(TK_LPAREN) {
		for !p.at(TK_RPAREN) && !p.atEOF() {
			v.Tuple = append(v.Tuple, p.parseType())
			if !p.eat(TK_COMMA) {
				break
			}
		}
		p.expect(TK_RPAREN)
	} else if p.eat(TK_LBRACE) {
		for !p.at(TK_RBRACE) && !p.atEOF() {
			fn := p.expect(TK_IDENT).Text
			p.expect(TK_COLON)
			ft := p.parseType()
			v.Fields = append(v.Fields, &Field{Name: fn, Type: ft})
			p.eat(TK_COMMA)
		}
		p.expect(TK_RBRACE)
	}
	v.SpanV = Span{Start: start, End: p.peek().Start}
	return v
}

func (p *Parser) parseTypeAlias(start int) Node {
	p.expect(TK_KW_TYPE)
	name := p.expect(TK_IDENT).Text
	p.parseOptionalGenerics()
	p.expect(TK_EQ)
	target := p.parseType()
	return &TypeAlias{Name: name, Target: target, SpanV: Span{Start: start, End: p.peek().Start}}
}

func (p *Parser) parseTrait(isPub bool, start int) Node {
	p.expect(TK_KW_TRAIT)
	name := p.expect(TK_IDENT).Text
	p.parseOptionalGenerics()
	items := []Node{}
	if p.eat(TK_LBRACE) {
		for !p.at(TK_RBRACE) && !p.atEOF() {
			it := p.parseItem()
			if it != nil {
				items = append(items, it)
			}
		}
		p.expect(TK_RBRACE)
	}
	return &TraitDecl{Name: name, Items: items, IsPub: isPub, SpanV: Span{Start: start, End: p.peek().Start}}
}

func (p *Parser) parseImpl(start int) Node {
	p.expect(TK_KW_IMPL)
	p.parseOptionalGenerics()
	// Parse type/path, then if `for` follows, it was the trait name.
	first := p.parseType()
	traitName := ""
	target := first
	if p.eat(TK_KW_FOR) {
		// extract trait-name from first if PathType
		if pt, ok := first.(*PathType); ok {
			traitName = joinDots(pt.Segments)
		}
		target = p.parseType()
	}
	items := []Node{}
	if p.eat(TK_LBRACE) {
		for !p.at(TK_RBRACE) && !p.atEOF() {
			it := p.parseItem()
			if it != nil {
				items = append(items, it)
			}
		}
		p.expect(TK_RBRACE)
	}
	return &ImplDecl{TraitName: traitName, TargetType: target, Items: items, SpanV: Span{Start: start, End: p.peek().Start}}
}

func (p *Parser) parseAgent(start int) Node {
	p.expect(TK_KW_AGENT)
	name := p.expect(TK_IDENT).Text
	ag := &AgentDecl{Name: name}
	// Optional ctor args.
	if p.at(TK_LPAREN) {
		ag.CtorArgs = p.parseFnParams()
	}
	// Optional `: Proto1, Proto2`.
	if p.eat(TK_COLON) {
		for {
			name := p.parseDottedIdent()
			if name == "" {
				break
			}
			ag.Protocols = append(ag.Protocols, name)
			if !p.eat(TK_COMMA) {
				break
			}
		}
	}
	if p.eat(TK_LBRACE) {
		for !p.at(TK_RBRACE) && !p.atEOF() {
			ag.Items = append(ag.Items, p.parseAgentItem())
		}
		p.expect(TK_RBRACE)
	}
	ag.SpanV = Span{Start: start, End: p.peek().Start}
	return ag
}

func (p *Parser) parseAgentItem() Node {
	startTok := p.peek()
	switch p.peek().Kind {
	case TK_KW_ON:
		return p.parseOnHandler(startTok.Start)
	case TK_KW_FN:
		return p.parseFn(false, false, false, false, startTok.Start)
	case TK_KW_PUB:
		p.next()
		return p.parseFn(true, false, false, false, startTok.Start)
	case TK_KW_STATE:
		// `state: T = expr` — treat as a field with `state` keyword as field name.
		p.next()
		p.expect(TK_COLON)
		typ := p.parseType()
		var val Node
		if p.eat(TK_EQ) {
			val = p.parseExpr(0)
		}
		return &AgentStateField{Name: "state", Type: typ, Value: val, SpanV: Span{Start: startTok.Start, End: p.peek().Start}}
	case TK_IDENT:
		// `name = expr` or `name: T = expr`  (state field shorthand).
		nameTok := p.next()
		var typ Node
		if p.eat(TK_COLON) {
			typ = p.parseType()
		}
		var val Node
		if p.eat(TK_EQ) {
			val = p.parseExpr(0)
		}
		return &AgentStateField{Name: nameTok.Text, Type: typ, Value: val, SpanV: Span{Start: startTok.Start, End: p.peek().Start}}
	}
	// Skip unknown.
	t := p.next()
	p.diag("MT1003", fmt.Sprintf("unexpected token %s in agent body", t.Kind.Name()), t.Start, t.End)
	return nil
}

func (p *Parser) parseOnHandler(start int) Node {
	p.expect(TK_KW_ON)
	msgName := p.expect(TK_IDENT).Text
	params := p.parseFnParams()
	var ret Node
	if p.eat(TK_ARROW) {
		// Could be a type OR a shorthand body (`-> expr`).
		// Peek: if we see `{` after parsing a type, it was a type. We try
		// a lookahead: if the next token after `->` looks like an expression
		// starter (literal / identifier path leading to a non-type form),
		// we parse it as a body. The RC3-amended §13.1 allows the
		// shorthand body form for handlers. Heuristic: if we encounter
		// `(`, `{`, or a literal token immediately, parse as expression.
		switch p.peek().Kind {
		case TK_LBRACE:
			body := p.parseBlock()
			return &OnHandler{MsgName: msgName, Params: params, Body: body, SpanV: Span{Start: start, End: body.SpanV.End}}
		case TK_INT, TK_FLOAT, TK_STRING, TK_CHAR, TK_KW_TRUE, TK_KW_FALSE,
			TK_LPAREN, TK_AMP, TK_KW_IF, TK_KW_MATCH, TK_KW_RETURN, TK_KW_LET,
			TK_KW_LOOP, TK_KW_WHILE, TK_KW_FOR, TK_KW_SPAWN, TK_KW_ARENA, TK_KW_BUDGET,
			TK_KW_UNSAFE, TK_KW_SANDBOX, TK_LBRACK:
			body := p.parseExpr(0)
			return &OnHandler{MsgName: msgName, Params: params, Body: body, SpanV: Span{Start: start, End: p.peek().Start}}
		case TK_IDENT:
			// Could be `-> Str` (type) or `-> msg` (expr). Both valid in handlers.
			// We treat as expression — this is the RC3 amendment.
			body := p.parseExpr(0)
			return &OnHandler{MsgName: msgName, Params: params, Body: body, SpanV: Span{Start: start, End: p.peek().Start}}
		default:
			ret = p.parseType()
		}
	}
	var body Node
	if p.at(TK_LBRACE) {
		body = p.parseBlock()
	}
	return &OnHandler{MsgName: msgName, Params: params, RetType: ret, Body: body, SpanV: Span{Start: start, End: p.peek().Start}}
}

func (p *Parser) parseProtocol(start int) Node {
	p.expect(TK_KW_PROTOCOL)
	name := p.expect(TK_IDENT).Text
	// Optional contextual version tag `v0`, `v1`, etc.
	if p.at(TK_IDENT) && len(p.peek().Text) > 1 && p.peek().Text[0] == 'v' {
		// looks like version — peek deeper before consuming
		tail := p.peek().Text[1:]
		isVer := true
		for _, c := range tail {
			if c < '0' || c > '9' {
				isVer = false
				break
			}
		}
		if isVer {
			p.next()
		}
	}
	proto := &ProtocolDecl{Name: name}
	if p.eat(TK_LBRACE) {
		for !p.at(TK_RBRACE) && !p.atEOF() {
			// Optional `msg` prefix per RC3 §13.1.
			if p.at(TK_IDENT) && p.peek().Text == "msg" {
				p.next()
			}
			mStart := p.peek().Start
			mn := p.expect(TK_IDENT).Text
			params := p.parseFnParams()
			var ret Node
			if p.eat(TK_ARROW) {
				ret = p.parseType()
			}
			proto.Messages = append(proto.Messages, &ProtoMsg{
				Name: mn, Params: params, RetType: ret,
				SpanV: Span{Start: mStart, End: p.peek().Start},
			})
		}
		p.expect(TK_RBRACE)
	}
	proto.SpanV = Span{Start: start, End: p.peek().Start}
	return proto
}

func (p *Parser) parseSupervisor(start int) Node {
	p.expect(TK_KW_SUP)
	return p.parseSupervisorTail(start)
}

func (p *Parser) parseSupervisorTail(start int) Node {
	name := p.expect(TK_IDENT).Text
	sup := &SupervisorDecl{Name: name}
	if p.at(TK_LPAREN) {
		sup.Args = p.parseFnParams()
	}
	// Optional `strategy IDENT` shorthand for one-arg strategy specifier.
	if p.at(TK_IDENT) && p.peek().Text == "strategy" {
		p.next()
		// consume strategy identifier
		if p.at(TK_IDENT) {
			p.next()
		}
	}
	if p.eat(TK_LBRACE) {
		for !p.at(TK_RBRACE) && !p.atEOF() {
			sup.Items = append(sup.Items, p.parseSupervisorClause())
		}
		p.expect(TK_RBRACE)
	}
	sup.SpanV = Span{Start: start, End: p.peek().Start}
	return sup
}

func (p *Parser) parseSupervisorClause() Node {
	startTok := p.peek()
	switch p.peek().Kind {
	case TK_KW_CHILD:
		p.next()
		name := p.expect(TK_IDENT).Text
		p.expect(TK_EQ)
		val := p.parseExpr(0)
		return &SupervisorClause{Kind: "child", Name: name, Args: []Node{val}, SpanV: Span{Start: startTok.Start, End: p.peek().Start}}
	case TK_KW_ON_FAIL:
		p.next()
		p.expect(TK_LPAREN)
		name := p.expect(TK_IDENT).Text
		p.expect(TK_RPAREN)
		body := p.parseBlock()
		return &SupervisorClause{Kind: "on_fail", Name: name, Items: []Node{body}, SpanV: Span{Start: startTok.Start, End: body.SpanV.End}}
	case TK_KW_RESTART:
		p.next()
		// `restart` may be followed by `up_to N in DUR` or be a bare keyword.
		args := []Node{}
		if p.eat(TK_KW_UP_TO) {
			args = append(args, p.parseExpr(0))
			if p.at(TK_IDENT) && p.peek().Text == "in" {
				p.next()
				args = append(args, p.parseExpr(0))
			} else if p.eat(TK_KW_IN) {
				args = append(args, p.parseExpr(0))
			}
		}
		return &SupervisorClause{Kind: "restart", Args: args, SpanV: Span{Start: startTok.Start, End: p.peek().Start}}
	case TK_KW_BACKOFF:
		p.next()
		// `backoff lo..hi`
		r := p.parseExpr(0)
		return &SupervisorClause{Kind: "backoff", Args: []Node{r}, SpanV: Span{Start: startTok.Start, End: p.peek().Start}}
	}
	t := p.next()
	p.diag("MT1004", fmt.Sprintf("unexpected token %s in supervisor body", t.Kind.Name()), t.Start, t.End)
	return nil
}

// parseExtern handles `extern { ... }` and `extern c { ... }` etc.
func (p *Parser) parseExtern(start int) Node {
	p.expect(TK_KW_EXTERN)
	abi := ""
	if p.at(TK_IDENT) && p.peekN(1).Kind == TK_LBRACE {
		abi = p.next().Text
	} else if p.at(TK_STRING) {
		abi = p.next().Text
	}
	items := []Node{}
	if p.eat(TK_LBRACE) {
		for !p.at(TK_RBRACE) && !p.atEOF() {
			it := p.parseItem()
			if it != nil {
				items = append(items, it)
			}
		}
		p.expect(TK_RBRACE)
	}
	return &ExternBlock{ABI: abi, Items: items, SpanV: Span{Start: start, End: p.peek().Start}}
}

// parseMacroDecl handles `macro name(p1, p2) => { body }`,
// `macro name(p1, p2) { body }`, and `proc macro name(...) -> Ret { body }`.
func (p *Parser) parseMacroDecl(isPub, isProc bool, start int) Node {
	if isProc {
		// consume `proc macro`
		p.next() // proc IDENT
		p.expect(TK_KW_MACRO)
	} else {
		p.expect(TK_KW_MACRO)
	}
	name := p.expect(TK_IDENT).Text
	params := []string{}
	if p.eat(TK_LPAREN) {
		for !p.at(TK_RPAREN) && !p.atEOF() {
			// each param: ident or ident: TokenStream — for our purposes just
			// capture the ident.
			if p.at(TK_IDENT) {
				params = append(params, p.next().Text)
				if p.eat(TK_COLON) {
					p.parseType()
				}
			}
			if !p.eat(TK_COMMA) {
				break
			}
		}
		p.expect(TK_RPAREN)
	}
	if p.eat(TK_ARROW) {
		p.parseType() // return type for proc macro
	}
	// `=>` body indicator (declarative macro), optional.
	p.eat(TK_FATARROW)
	var body Node
	if p.at(TK_LBRACE) {
		body = p.parseBlock()
	}
	return &MacroDecl{Name: name, Params: params, Body: body, IsProc: isProc, IsPub: isPub, SpanV: Span{Start: start, End: p.peek().Start}}
}

// skipBalanced consumes from current pos up to and including the matching close.
func (p *Parser) skipBalanced(open, close TokenKind) {
	if !p.eat(open) {
		return
	}
	depth := 1
	for !p.atEOF() && depth > 0 {
		k := p.peek().Kind
		if k == open {
			depth++
		} else if k == close {
			depth--
		}
		p.next()
	}
}

// parseDottedIdent reads `a.b.c` and returns the dotted name (empty if none).
func (p *Parser) parseDottedIdent() string {
	if !p.at(TK_IDENT) {
		return ""
	}
	parts := []string{p.next().Text}
	for p.eat(TK_DOT) {
		if !p.at(TK_IDENT) {
			break
		}
		parts = append(parts, p.next().Text)
	}
	return joinDots(parts)
}

// ----- types -----

func (p *Parser) parseType() Node {
	start := p.peek().Start
	var base Node
	switch p.peek().Kind {
	case TK_AMP:
		p.next()
		mut := p.eat(TK_KW_MUT)
		inner := p.parseType()
		base = &RefType{Mut: mut, Inner: inner, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_STAR:
		p.next()
		mut := p.eat(TK_KW_MUT)
		// `*const T` shape.
		if p.at(TK_KW_CONST) {
			p.next()
		}
		inner := p.parseType()
		base = &PtrType{Mut: mut, Inner: inner, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_LPAREN:
		p.next()
		elements := []Node{}
		for !p.at(TK_RPAREN) && !p.atEOF() {
			elements = append(elements, p.parseType())
			if !p.eat(TK_COMMA) {
				break
			}
		}
		p.expect(TK_RPAREN)
		if len(elements) == 1 {
			base = elements[0]
		} else {
			base = &TupleType{Elements: elements, SpanV: Span{Start: start, End: p.peek().Start}}
		}
	case TK_LBRACK:
		p.next()
		elem := p.parseType()
		var sz Node
		if p.eat(TK_SEMI) {
			sz = p.parseExpr(0)
		}
		p.expect(TK_RBRACK)
		base = &ArrayType{Elem: elem, Size: sz, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_KW_FN:
		p.next()
		p.expect(TK_LPAREN)
		params := []Node{}
		for !p.at(TK_RPAREN) && !p.atEOF() {
			params = append(params, p.parseType())
			if !p.eat(TK_COMMA) {
				break
			}
		}
		p.expect(TK_RPAREN)
		var ret Node
		if p.eat(TK_ARROW) {
			ret = p.parseType()
		}
		base = &FnType{Params: params, Ret: ret, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_KW_DYN:
		p.next()
		base = p.parsePathType()
	default:
		base = p.parsePathType()
	}

	// Postfix `!E` / `!{A, B}` sugar (A11).
	if p.at(TK_BANG) {
		p.next()
		if p.eat(TK_LBRACE) {
			errs := []Node{}
			for !p.at(TK_RBRACE) && !p.atEOF() {
				errs = append(errs, p.parseType())
				if !p.eat(TK_COMMA) {
					break
				}
			}
			p.expect(TK_RBRACE)
			base = &BangType{OK: base, Errs: errs, SpanV: Span{Start: start, End: p.peek().Start}}
		} else {
			err := p.parseType()
			base = &BangType{OK: base, Err: err, SpanV: Span{Start: start, End: p.peek().Start}}
		}
	}
	return base
}

func (p *Parser) parsePathType() Node {
	start := p.peek().Start
	if !p.at(TK_IDENT) && !p.at(TK_KW_SELF) {
		t := p.next()
		p.diag("MT1005", fmt.Sprintf("expected type, found %s", t.Kind.Name()), t.Start, t.End)
		return &PathType{SpanV: Span{Start: start, End: t.End}}
	}
	segs := []string{p.next().Text}
	for {
		if p.at(TK_DOT) && p.peekN(1).Kind == TK_IDENT {
			p.next()
			segs = append(segs, p.next().Text)
			continue
		}
		break
	}
	gens := []Node{}
	if p.at(TK_LBRACK) {
		// Distinguish from array type? In type position, `Vec[T]` is generic.
		p.next()
		for !p.at(TK_RBRACK) && !p.atEOF() {
			gens = append(gens, p.parseType())
			if !p.eat(TK_COMMA) {
				break
			}
		}
		p.expect(TK_RBRACK)
	}
	return &PathType{Segments: segs, Generics: gens, SpanV: Span{Start: start, End: p.peek().Start}}
}

// ----- blocks -----

func (p *Parser) parseBlock() *Block {
	start := p.peek().Start
	if !p.eat(TK_LBRACE) {
		t := p.peek()
		p.diag("MT1006", fmt.Sprintf("expected '{', found %s", t.Kind.Name()), t.Start, t.End)
		return &Block{SpanV: Span{Start: start, End: t.End}}
	}
	stmts := []Node{}
	for !p.at(TK_RBRACE) && !p.atEOF() {
		stmts = append(stmts, p.parseStmt())
		p.eat(TK_SEMI)
	}
	end := p.expect(TK_RBRACE).End
	return &Block{Stmts: stmts, SpanV: Span{Start: start, End: end}}
}

func (p *Parser) parseStmt() Node {
	startTok := p.peek()
	if p.at(TK_KW_LET) {
		return p.parseLetStmt(startTok.Start)
	}
	expr := p.parseExpr(0)
	return &ExprStmt{Expr: expr, SpanV: Span{Start: startTok.Start, End: p.peek().Start}}
}

func (p *Parser) parseLetStmt(start int) Node {
	p.expect(TK_KW_LET)
	mut := p.eat(TK_KW_MUT)
	pat := p.parsePattern()
	var typ Node
	if p.eat(TK_COLON) {
		typ = p.parseType()
	}
	var val Node
	if p.eat(TK_EQ) {
		val = p.parseExpr(0)
	}
	return &LetStmt{Pattern: pat, Type: typ, Value: val, IsMut: mut, SpanV: Span{Start: start, End: p.peek().Start}}
}

// ----- patterns -----

func (p *Parser) parsePattern() Node {
	start := p.peek().Start
	switch p.peek().Kind {
	case TK_UNDERSCORE:
		t := p.next()
		return &WildPat{SpanV: Span{Start: t.Start, End: t.End}}
	case TK_IDENT:
		// could be `_`, `name`, or `Path.Variant(args)`.
		name := p.peek().Text
		if name == "_" {
			t := p.next()
			return &WildPat{SpanV: Span{Start: t.Start, End: t.End}}
		}
		// Look ahead: if first letter is uppercase, treat as path pattern.
		first := name[0]
		if first >= 'A' && first <= 'Z' {
			return p.parsePathPattern(start)
		}
		// otherwise identifier binding.
		p.next()
		return &IdentPat{Name: name, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_KW_MUT:
		p.next()
		name := p.expect(TK_IDENT).Text
		return &IdentPat{Name: name, IsMut: true, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_KW_REF:
		p.next()
		p.eat(TK_KW_MUT)
		name := p.expect(TK_IDENT).Text
		return &IdentPat{Name: name, IsRef: true, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_INT, TK_FLOAT, TK_STRING, TK_CHAR, TK_KW_TRUE, TK_KW_FALSE:
		lit := p.parseLiteralExpr().(*LitExpr)
		// range pattern?
		if p.at(TK_DOTDOT) || p.at(TK_DOTDOTEQ) {
			inclusive := p.peek().Kind == TK_DOTDOTEQ
			p.next()
			to := p.parseLiteralExpr()
			return &RangePat{From: lit, To: to, Inclusive: inclusive, SpanV: Span{Start: start, End: p.peek().Start}}
		}
		return &LitPat{Lit: lit, SpanV: lit.SpanV}
	case TK_LPAREN:
		// tuple pattern
		p.next()
		elements := []Node{}
		for !p.at(TK_RPAREN) && !p.atEOF() {
			elements = append(elements, p.parsePattern())
			if !p.eat(TK_COMMA) {
				break
			}
		}
		p.expect(TK_RPAREN)
		// Wrap in a synthetic PathPat with empty path for tuple-pat
		return &PathPat{Tuple: elements, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_AMP:
		p.next()
		p.eat(TK_KW_MUT)
		inner := p.parsePattern()
		// represent as IdentPat with IsRef? We'll wrap as PathPat with single tuple
		_ = inner
		return inner
	case TK_MINUS:
		// negative literal pattern.
		p.next()
		lit := p.parseLiteralExpr().(*LitExpr)
		lit.Text = "-" + lit.Text
		return &LitPat{Lit: lit, SpanV: lit.SpanV}
	}
	t := p.next()
	p.diag("MT1007", fmt.Sprintf("unexpected token %s in pattern", t.Kind.Name()), t.Start, t.End)
	return &WildPat{SpanV: Span{Start: start, End: t.End}}
}

func (p *Parser) parsePathPattern(start int) Node {
	segs := []string{p.next().Text}
	for p.eat(TK_DOT) {
		if !p.at(TK_IDENT) {
			break
		}
		segs = append(segs, p.next().Text)
	}
	pat := &PathPat{Path: segs, SpanV: Span{Start: start, End: p.peek().Start}}
	if p.eat(TK_LPAREN) {
		for !p.at(TK_RPAREN) && !p.atEOF() {
			pat.Tuple = append(pat.Tuple, p.parsePattern())
			if !p.eat(TK_COMMA) {
				break
			}
		}
		p.expect(TK_RPAREN)
	} else if p.eat(TK_LBRACE) {
		pat.IsRecord = true
		for !p.at(TK_RBRACE) && !p.atEOF() {
			fn := p.expect(TK_IDENT).Text
			var val Node
			if p.eat(TK_COLON) {
				val = p.parsePattern()
			}
			pat.Fields = append(pat.Fields, &FieldInit{Name: fn, Value: val})
			if !p.eat(TK_COMMA) {
				break
			}
		}
		p.expect(TK_RBRACE)
	}
	pat.SpanV = Span{Start: start, End: p.peek().Start}
	return pat
}

// ======================================================================
//   Expression parsing (Pratt)
// ======================================================================

// The precedence table below is the parser's derivation of v1.0-RC §11.1.1
// (which itself is referenced as "new normative" in the spec's preface but
// is missing from the §11 body in RC3 — flagged as FINDING #1 in GO_IMPL
// notes). Levels chosen by mirroring Rust + the example corpus.
//
//   30 . :: postfix call/index    (handled in postfix loop)
//   25 unary  - ! & *             (handled in prefix)
//   22 as                         (cast)
//   20 * / %
//   18 + -
//   16 << >>
//   14 &
//   12 ^
//   10 |
//    8 == != < <= > >=
//    6 &&
//    5 ||
//    4 ..  ..=                    (range)
//    3 = += -= *= /= %= &= |= ^=  (assignment, right-assoc)

const (
	precNone = 0
	precAssign = 3
	precRange = 4
	precOr     = 5
	precAnd    = 6
	precCmp    = 8
	precBitOr  = 10
	precBitXor = 12
	precBitAnd = 14
	precShift  = 16
	precAdd    = 18
	precMul    = 20
	precAs     = 22
)

// infixPrec returns the precedence of the current token in infix position,
// and a boolean for right-associativity.
func infixPrec(k TokenKind) (int, bool) {
	switch k {
	case TK_EQ, TK_PLUSEQ, TK_MINUSEQ, TK_STAREQ, TK_SLASHEQ, TK_PERCENTEQ,
		TK_AMPEQ, TK_PIPEEQ, TK_CARETEQ:
		return precAssign, true
	case TK_DOTDOT, TK_DOTDOTEQ:
		return precRange, false
	case TK_PIPEPIPE:
		return precOr, false
	case TK_AMPAMP:
		return precAnd, false
	case TK_EQEQ, TK_NE, TK_LT, TK_LE, TK_GT, TK_GE:
		return precCmp, false
	case TK_PIPE:
		return precBitOr, false
	case TK_CARET:
		return precBitXor, false
	case TK_AMP:
		return precBitAnd, false
	case TK_SHL, TK_SHR:
		return precShift, false
	case TK_PLUS, TK_MINUS:
		return precAdd, false
	case TK_STAR, TK_SLASH, TK_PERCENT:
		return precMul, false
	case TK_KW_AS:
		return precAs, false
	}
	return precNone, false
}

func opText(k TokenKind) string {
	if n, ok := tokenKindNames[k]; ok {
		return n
	}
	return "?"
}

// parseExpr is the Pratt loop driver. minPrec is the minimum precedence
// that the loop will keep consuming infix operators for.
func (p *Parser) parseExpr(minPrec int) Node {
	lhs := p.parsePrefix()
	for !p.atEOF() {
		lhs = p.tryPostfix(lhs)
		prec, rightAssoc := infixPrec(p.peek().Kind)
		if prec == precNone || prec < minPrec {
			break
		}
		op := p.next()
		nextMin := prec + 1
		if rightAssoc {
			nextMin = prec
		}
		// Range with optional RHS.
		if op.Kind == TK_DOTDOT || op.Kind == TK_DOTDOTEQ {
			var to Node
			if p.canStartExpr() {
				to = p.parseExpr(nextMin)
			}
			lhs = &RangeExpr{From: lhs, To: to, Inclusive: op.Kind == TK_DOTDOTEQ, SpanV: Span{Start: lhs.Span().Start, End: p.peek().Start}}
			continue
		}
		rhs := p.parseExpr(nextMin)
		lhs = &BinExpr{Op: opText(op.Kind), LHS: lhs, RHS: rhs, SpanV: Span{Start: lhs.Span().Start, End: rhs.Span().End}}
	}
	return lhs
}

func (p *Parser) canStartExpr() bool {
	switch p.peek().Kind {
	case TK_RPAREN, TK_RBRACE, TK_RBRACK, TK_COMMA, TK_SEMI, TK_FATARROW, TK_EOF:
		return false
	}
	return true
}

// parsePrefix handles prefix forms and primaries.
func (p *Parser) parsePrefix() Node {
	start := p.peek().Start
	switch p.peek().Kind {
	case TK_MINUS, TK_BANG, TK_STAR:
		op := p.next()
		inner := p.parseExpr(precMul) // unary binds tighter than most binary
		return &UnaryExpr{Op: opText(op.Kind), Inner: inner, SpanV: Span{Start: start, End: inner.Span().End}}
	case TK_AMP:
		p.next()
		isMut := p.eat(TK_KW_MUT)
		inner := p.parseExpr(precMul)
		op := "&"
		if isMut {
			op = "&mut"
		}
		return &UnaryExpr{Op: op, Inner: inner, SpanV: Span{Start: start, End: inner.Span().End}}
	case TK_DOTDOT, TK_DOTDOTEQ:
		// Prefix range `..hi` or `..=hi`.
		op := p.next()
		var to Node
		if p.canStartExpr() {
			to = p.parseExpr(precRange + 1)
		}
		return &RangeExpr{To: to, Inclusive: op.Kind == TK_DOTDOTEQ, SpanV: Span{Start: start, End: p.peek().Start}}
	}
	return p.parseAtom()
}

// parseAtom: literal / path / parens / block / control flow / spawn / arena ...
func (p *Parser) parseAtom() Node {
	start := p.peek().Start
	switch p.peek().Kind {
	case TK_INT, TK_FLOAT, TK_STRING, TK_RAW_STRING, TK_BYTE_STRING, TK_CHAR,
		TK_KW_TRUE, TK_KW_FALSE, TK_HTML_STRING, TK_DURATION, TK_SIZE:
		return p.parseLiteralExpr()
	case TK_LPAREN:
		p.next()
		if p.eat(TK_RPAREN) {
			// unit
			return &TupleExpr{SpanV: Span{Start: start, End: p.peek().Start}}
		}
		first := p.parseExpr(0)
		if p.eat(TK_COMMA) {
			elements := []Node{first}
			for !p.at(TK_RPAREN) && !p.atEOF() {
				elements = append(elements, p.parseExpr(0))
				if !p.eat(TK_COMMA) {
					break
				}
			}
			p.expect(TK_RPAREN)
			return &TupleExpr{Elements: elements, SpanV: Span{Start: start, End: p.peek().Start}}
		}
		p.expect(TK_RPAREN)
		return first
	case TK_LBRACK:
		p.next()
		elements := []Node{}
		if !p.at(TK_RBRACK) {
			elements = append(elements, p.parseExpr(0))
			// `[expr; count]` form
			if p.eat(TK_SEMI) {
				count := p.parseExpr(0)
				p.expect(TK_RBRACK)
				return &ArrayExpr{Elements: []Node{elements[0], count}, SpanV: Span{Start: start, End: p.peek().Start}}
			}
			for p.eat(TK_COMMA) && !p.at(TK_RBRACK) {
				elements = append(elements, p.parseExpr(0))
			}
		}
		p.expect(TK_RBRACK)
		return &ArrayExpr{Elements: elements, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_LBRACE:
		return p.parseBlock()
	case TK_KW_IF:
		return p.parseIfExpr()
	case TK_KW_MATCH:
		return p.parseMatchExpr()
	case TK_KW_LOOP:
		p.next()
		body := p.parseBlock()
		return &LoopExpr{Body: body, SpanV: Span{Start: start, End: body.SpanV.End}}
	case TK_KW_WHILE:
		p.next()
		cond := p.parseExprNoStruct()
		body := p.parseBlock()
		return &WhileExpr{Cond: cond, Body: body, SpanV: Span{Start: start, End: body.SpanV.End}}
	case TK_KW_FOR:
		p.next()
		pat := p.parsePattern()
		p.expect(TK_KW_IN)
		iter := p.parseExprNoStruct()
		body := p.parseBlock()
		return &ForExpr{Pattern: pat, Iter: iter, Body: body, SpanV: Span{Start: start, End: body.SpanV.End}}
	case TK_KW_RETURN:
		p.next()
		var v Node
		if p.canStartExpr() {
			v = p.parseExpr(0)
		}
		return &ReturnExpr{Value: v, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_KW_BREAK:
		p.next()
		var v Node
		if p.canStartExpr() {
			v = p.parseExpr(0)
		}
		return &BreakExpr{Value: v, SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_KW_CONTINUE:
		p.next()
		return &ContinueExpr{SpanV: Span{Start: start, End: p.peek().Start}}
	case TK_KW_SPAWN:
		p.next()
		inner := p.parseExpr(precMul) // tight to receiver+call
		return &SpawnExpr{Inner: inner, SpanV: Span{Start: start, End: inner.Span().End}}
	case TK_KW_ARENA:
		return p.parseArenaExpr(start)
	case TK_KW_UNSAFE:
		p.next()
		body := p.parseBlock()
		return &UnsafeExpr{Body: body, SpanV: Span{Start: start, End: body.SpanV.End}}
	case TK_KW_BUDGET:
		return p.parseBudgetExpr(start)
	case TK_KW_SANDBOX:
		return p.parseSandboxExpr(start)
	case TK_KW_RUN:
		p.next()
		inner := p.parseExpr(0)
		return &RunExpr{Inner: inner, SpanV: Span{Start: start, End: inner.Span().End}}
	case TK_KW_FN:
		return p.parseClosureExpr(start)
	case TK_KW_LET:
		// `let ... = ...` is also a statement at block level; here in expr
		// position we recover by delegating to let-stmt.
		return p.parseLetStmt(start)
	case TK_KW_MOVE:
		p.next()
		// `move fn(...) { }` closure or `move { }` block.
		inner := p.parseExpr(0)
		return inner
	case TK_KW_SELF, TK_IDENT:
		return p.parsePathExpr(start)
	}
	t := p.next()
	p.diag("MT1010", fmt.Sprintf("unexpected token %s in expression", t.Kind.Name()), t.Start, t.End)
	return &LitExpr{Kind: "invalid", SpanV: Span{Start: start, End: t.End}}
}

// parseExprNoStruct is used in places where `{` after the expression would
// start a block rather than a struct literal (e.g. `while cond { ... }`).
// For v1.0 we treat *all* `{` as block starters in this context; struct
// literals must be parenthesised inside loop/if heads. This matches Rust.
func (p *Parser) parseExprNoStruct() Node {
	// We currently use a heuristic: the parsePathExpr struct-literal
	// detection looks for `Path {` followed by `ident :` or `..`. For the
	// example corpus this is sufficient because all loop heads in the
	// corpus are bare names or method calls.
	return p.parseExpr(0)
}

// parseLiteralExpr captures one literal token as a LitExpr.
func (p *Parser) parseLiteralExpr() Node {
	t := p.next()
	kind := "int"
	switch t.Kind {
	case TK_FLOAT:
		kind = "float"
	case TK_STRING, TK_RAW_STRING:
		kind = "string"
	case TK_BYTE_STRING:
		kind = "bytestring"
	case TK_CHAR:
		kind = "char"
	case TK_KW_TRUE, TK_KW_FALSE:
		kind = "bool"
	case TK_DURATION:
		kind = "duration"
	case TK_SIZE:
		kind = "size"
	case TK_HTML_STRING:
		kind = "html"
	}
	return &LitExpr{Kind: kind, Text: t.Text, SpanV: Span{Start: t.Start, End: t.End}}
}

// parsePathExpr handles a path-leading expression (a.b.c, `Foo::[T](x)`, etc.).
func (p *Parser) parsePathExpr(start int) Node {
	first := p.next()
	segs := []string{first.Text}
	var generics []Node

	// Loop over `::` (turbofish) and `.` (field access) — but `.` is also a
	// postfix operator on any expression. To keep parsing deterministic we
	// only consume leading-dot segments here when followed by ident AND we
	// haven't yet seen a turbofish. The postfix loop in tryPostfix handles
	// the general `.field` / `.method(...)` case.

	// Turbofish: `::[T1, T2]`.
	if p.at(TK_DCOLON) && p.peekN(1).Kind == TK_LBRACK {
		p.next() // ::
		p.next() // [
		for !p.at(TK_RBRACK) && !p.atEOF() {
			generics = append(generics, p.parseType())
			if !p.eat(TK_COMMA) {
				break
			}
		}
		p.expect(TK_RBRACK)
	}

	// Multi-segment path with `::` (e.g. `Vec::new`).
	for p.at(TK_DCOLON) && p.peekN(1).Kind == TK_IDENT {
		p.next()
		segs = append(segs, p.next().Text)
		// nested turbofish per segment
		if p.at(TK_DCOLON) && p.peekN(1).Kind == TK_LBRACK {
			p.next()
			p.next()
			for !p.at(TK_RBRACK) && !p.atEOF() {
				generics = append(generics, p.parseType())
				if !p.eat(TK_COMMA) {
					break
				}
			}
			p.expect(TK_RBRACK)
		}
	}

	// Multi-segment with `.` for module paths (use sites): `std.http.serve`.
	// We only consume here if the dot-suffix looks like a path continuation
	// (next is ident and ident is followed by `(`, `.`, `?`, `!`, `[`, etc.).
	// To stay simple and match the corpus, consume dot-segments greedily.
	for p.at(TK_DOT) && p.peekN(1).Kind == TK_IDENT {
		// stop if `.field` would also be the postfix-loop's job — but for
		// path-position prefixes (lowercase initial), we treat dot as path.
		// Heuristic: only continue if no parens/braces follow first segment.
		// Simpler: don't consume dots here; let postfix loop handle them.
		break
	}

	path := &PathExpr{Segments: segs, Generics: generics, SpanV: Span{Start: start, End: p.peek().Start}}

	// Struct-literal opportunity: `Path { f: v, ... }` (only if next is `{`
	// and the form looks like a field list).
	if p.at(TK_LBRACE) && p.looksLikeStructLiteral() {
		return p.parseStructLiteral(path, start)
	}
	return path
}

// looksLikeStructLiteral peeks past the brace to decide if it's a struct
// literal or a block. Conservative: requires `ident :` or `..` inside,
// otherwise leave to caller.
func (p *Parser) looksLikeStructLiteral() bool {
	// We're at `{`. Look at the next non-trivia token.
	if p.peekN(1).Kind == TK_RBRACE {
		return true // empty struct literal e.g. Map::[..]{}
	}
	if p.peekN(1).Kind == TK_IDENT && p.peekN(2).Kind == TK_COLON {
		return true
	}
	if p.peekN(1).Kind == TK_DOTDOT {
		return true
	}
	return false
}

func (p *Parser) parseStructLiteral(path Node, start int) Node {
	p.expect(TK_LBRACE)
	fields := []*FieldInit{}
	for !p.at(TK_RBRACE) && !p.atEOF() {
		if p.eat(TK_DOTDOT) {
			p.parseExpr(0) // spread base — discard
			break
		}
		fn := p.expect(TK_IDENT).Text
		var val Node
		if p.eat(TK_COLON) {
			val = p.parseExpr(0)
		}
		fields = append(fields, &FieldInit{Name: fn, Value: val})
		if !p.eat(TK_COMMA) {
			break
		}
	}
	end := p.expect(TK_RBRACE).End
	return &StructExpr{Path: path, Fields: fields, SpanV: Span{Start: start, End: end}}
}

// tryPostfix repeatedly applies postfix forms (., (call), [index], ?, !Msg, ?Msg, @D).
func (p *Parser) tryPostfix(lhs Node) Node {
	for {
		switch p.peek().Kind {
		case TK_DOT:
			// .field or .method(args)
			p.next()
			// allow keyword-tolerant `.method` (§3.3.4) — any keyword token.
			if p.at(TK_INT) {
				// tuple-index `.0`
				idx := p.next()
				lhs = &FieldExpr{Receiver: lhs, Field: idx.Text, SpanV: Span{Start: lhs.Span().Start, End: idx.End}}
				continue
			}
			t := p.peek()
			if t.Kind == TK_IDENT || isKeywordTokenKind(t.Kind) {
				name := p.next().Text
				lhs = &FieldExpr{Receiver: lhs, Field: name, SpanV: Span{Start: lhs.Span().Start, End: p.peek().Start}}
				continue
			}
			// recovery
			p.diag("MT1011", "expected field or method name after '.'", p.peek().Start, p.peek().End)
			return lhs
		case TK_LPAREN:
			args := p.parseCallArgs()
			lhs = &CallExpr{Callee: lhs, Args: args, SpanV: Span{Start: lhs.Span().Start, End: p.peek().Start}}
		case TK_LBRACK:
			p.next()
			idx := p.parseExpr(0)
			p.expect(TK_RBRACK)
			lhs = &IndexExpr{Receiver: lhs, Index: idx, SpanV: Span{Start: lhs.Span().Start, End: p.peek().Start}}
		case TK_QUESTION:
			// `expr?` propagate OR `expr?Msg(...)` ask sugar (A12).
			// The Msg must follow on the same source line to disambiguate.
			// Simpler heuristic: if next token after `?` is IDENT starting
			// with uppercase and is followed by `(`, it's an ask sugar.
			if p.peekN(1).Kind == TK_IDENT && p.peekN(2).Kind == TK_LPAREN {
				name := p.peekN(1).Text
				if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
					p.next() // ?
					p.next() // Ident
					args := p.parseCallArgs()
					lhs = &AskExpr{Target: lhs, Msg: name, Args: args, SpanV: Span{Start: lhs.Span().Start, End: p.peek().Start}}
					continue
				}
			}
			p.next()
			lhs = &PostfixExpr{Op: "?", Inner: lhs, SpanV: Span{Start: lhs.Span().Start, End: p.peek().Start}}
		case TK_BANG:
			// `expr!Msg(...)` send sugar OR macro call `name!(...)`.
			if p.peekN(1).Kind == TK_LPAREN {
				// macro call shape — but only if lhs is a PathExpr of a single segment.
				if pe, ok := lhs.(*PathExpr); ok && len(pe.Segments) == 1 {
					p.next() // !
					args := p.parseCallArgs()
					lhs = &MacroCall{Name: pe.Segments[0], Args: args, SpanV: Span{Start: lhs.Span().Start, End: p.peek().Start}}
					continue
				}
			}
			if p.peekN(1).Kind == TK_IDENT && p.peekN(2).Kind == TK_LPAREN {
				name := p.peekN(1).Text
				if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
					p.next() // !
					p.next() // Ident
					args := p.parseCallArgs()
					lhs = &SendExpr{Target: lhs, Msg: name, Args: args, SpanV: Span{Start: lhs.Span().Start, End: p.peek().Start}}
					continue
				}
			}
			// Bare `expr!` is the boolean-not in postfix position — but the
			// canonical `!` is prefix. We treat bare postfix-! as a parse
			// error/no-op.
			return lhs
		case TK_AT:
			// `expr @ duration` deadline form.
			p.next()
			dur := p.parseExpr(precMul) // duration literal or expression
			lhs = &DeadlineExpr{Inner: lhs, Duration: dur, SpanV: Span{Start: lhs.Span().Start, End: dur.Span().End}}
		default:
			return lhs
		}
	}
}

func (p *Parser) parseCallArgs() []Node {
	p.expect(TK_LPAREN)
	args := []Node{}
	for !p.at(TK_RPAREN) && !p.atEOF() {
		args = append(args, p.parseExpr(0))
		if !p.eat(TK_COMMA) {
			break
		}
	}
	p.expect(TK_RPAREN)
	return args
}

func (p *Parser) parseIfExpr() Node {
	start := p.peek().Start
	p.expect(TK_KW_IF)
	if p.eat(TK_KW_LET) {
		pat := p.parsePattern()
		p.expect(TK_EQ)
		cond := p.parseExprNoStruct()
		then := p.parseBlock()
		var els Node
		if p.eat(TK_KW_ELSE) {
			if p.at(TK_KW_IF) {
				els = p.parseIfExpr()
			} else {
				els = p.parseBlock()
			}
		}
		return &IfExpr{IsLet: true, Pattern: pat, Cond: cond, Then: then, Else: els, SpanV: Span{Start: start, End: p.peek().Start}}
	}
	cond := p.parseExprNoStruct()
	then := p.parseBlock()
	var els Node
	if p.eat(TK_KW_ELSE) {
		if p.at(TK_KW_IF) {
			els = p.parseIfExpr()
		} else {
			els = p.parseBlock()
		}
	}
	return &IfExpr{Cond: cond, Then: then, Else: els, SpanV: Span{Start: start, End: p.peek().Start}}
}

func (p *Parser) parseMatchExpr() Node {
	start := p.peek().Start
	p.expect(TK_KW_MATCH)
	scrut := p.parseExprNoStruct()
	p.expect(TK_LBRACE)
	arms := []*MatchArm{}
	for !p.at(TK_RBRACE) && !p.atEOF() {
		armStart := p.peek().Start
		pat := p.parsePattern()
		p.expect(TK_FATARROW)
		body := p.parseExpr(0)
		arms = append(arms, &MatchArm{Pattern: pat, Body: body, SpanV: Span{Start: armStart, End: p.peek().Start}})
		p.eat(TK_COMMA)
	}
	end := p.expect(TK_RBRACE).End
	return &MatchExpr{Scrut: scrut, Arms: arms, SpanV: Span{Start: start, End: end}}
}

func (p *Parser) parseArenaExpr(start int) Node {
	p.expect(TK_KW_ARENA)
	label := ""
	if p.at(TK_IDENT) {
		label = p.next().Text
	}
	// `arena LABEL : <expr>` inline shorthand per RC3 §10.1.
	if p.eat(TK_COLON) {
		body := p.parseExpr(0)
		return &ArenaExpr{Label: label, Body: body, SpanV: Span{Start: start, End: p.peek().Start}}
	}
	body := p.parseBlock()
	return &ArenaExpr{Label: label, Body: body, SpanV: Span{Start: start, End: body.SpanV.End}}
}

func (p *Parser) parseBudgetExpr(start int) Node {
	p.expect(TK_KW_BUDGET)
	entries := p.parseConfigBlock()
	var body Node
	if p.eat(TK_KW_RUN) {
		if p.at(TK_LBRACE) {
			body = p.parseBlock()
		} else {
			body = p.parseExpr(0)
		}
	}
	return &BudgetExpr{Entries: entries, Body: body, SpanV: Span{Start: start, End: p.peek().Start}}
}

func (p *Parser) parseSandboxExpr(start int) Node {
	p.expect(TK_KW_SANDBOX)
	name := ""
	if p.at(TK_IDENT) {
		name = p.next().Text
	}
	// Optional `with` keyword.
	p.eat(TK_KW_WITH)
	entries := p.parseConfigBlock()
	var body Node
	if p.at(TK_LBRACE) {
		body = p.parseBlock()
	}
	return &SandboxExpr{Name: name, Entries: entries, Body: body, SpanV: Span{Start: start, End: p.peek().Start}}
}

// parseConfigBlock reads `{ key value, key = value, key.sub = value, ... }`.
func (p *Parser) parseConfigBlock() []*BudgetEntry {
	if !p.eat(TK_LBRACE) {
		return nil
	}
	out := []*BudgetEntry{}
	for !p.at(TK_RBRACE) && !p.atEOF() {
		eStart := p.peek().Start
		// key may be a dotted ident: `fs.read`.
		keyParts := []string{}
		if p.at(TK_IDENT) || isKeywordTokenKind(p.peek().Kind) {
			keyParts = append(keyParts, p.next().Text)
			for p.eat(TK_DOT) {
				if p.at(TK_IDENT) || isKeywordTokenKind(p.peek().Kind) {
					keyParts = append(keyParts, p.next().Text)
				} else {
					break
				}
			}
		}
		// Optional `=`.
		p.eat(TK_EQ)
		// Value: any expression until comma/`}`.
		val := p.parseExpr(0)
		out = append(out, &BudgetEntry{Key: joinDots(keyParts), Value: val, SpanV: Span{Start: eStart, End: p.peek().Start}})
		p.eat(TK_COMMA)
		// also allow `;` or newline-only separators (newline is trivia).
		p.eat(TK_SEMI)
	}
	p.expect(TK_RBRACE)
	return out
}

func (p *Parser) parseClosureExpr(start int) Node {
	p.expect(TK_KW_FN)
	params := p.parseFnParams()
	var ret Node
	if p.eat(TK_ARROW) {
		ret = p.parseType()
	}
	var body Node
	if p.at(TK_LBRACE) {
		body = p.parseBlock()
	} else {
		body = p.parseExpr(0)
	}
	return &ClosureExpr{Params: params, Ret: ret, Body: body, SpanV: Span{Start: start, End: p.peek().Start}}
}

// ----- helpers -----

func joinDots(parts []string) string {
	out := ""
	for i, p := range parts {
		if i > 0 {
			out += "."
		}
		out += p
	}
	return out
}

func isKeywordTokenKind(k TokenKind) bool {
	return k >= TK_KW_AGENT && k <= TK_KW_YIELD
}

// Parse is a convenience wrapper.
func Parse(src string) (*File, []Diagnostic) {
	return NewParser(src).Parse()
}
