package mty

import (
	"testing"
)

func mustParse(t *testing.T, src string) *File {
	t.Helper()
	f, diags := Parse(src)
	if len(diags) != 0 {
		t.Fatalf("unexpected diags: %v", diags)
	}
	return f
}

func TestParseHello(t *testing.T) {
	src := `fn main() {
  log("hello, Mighty")
}`
	f := mustParse(t, src)
	if len(f.Items) != 1 {
		t.Fatalf("expected 1 item, got %d", len(f.Items))
	}
	fn, ok := f.Items[0].(*FnDecl)
	if !ok {
		t.Fatalf("expected FnDecl, got %T", f.Items[0])
	}
	if fn.Name != "main" {
		t.Fatalf("name = %q, want main", fn.Name)
	}
}

func TestParseStructEnumType(t *testing.T) {
	src := `struct User {
  id: UserId
  name: String
}

enum Shape {
  Circle(F64)
  Rect(F64, F64)
}

type UserId = U64`
	f := mustParse(t, src)
	if len(f.Items) != 3 {
		t.Fatalf("expected 3 items, got %d", len(f.Items))
	}
	if _, ok := f.Items[0].(*StructDecl); !ok {
		t.Fatalf("first item not StructDecl: %T", f.Items[0])
	}
	if _, ok := f.Items[1].(*EnumDecl); !ok {
		t.Fatalf("second item not EnumDecl: %T", f.Items[1])
	}
	if _, ok := f.Items[2].(*TypeAlias); !ok {
		t.Fatalf("third item not TypeAlias: %T", f.Items[2])
	}
}

func TestParseGenericFn(t *testing.T) {
	src := `fn first[T](xs: &[T]) -> Option[&T] {
  if xs.len == 0 { None } else { Some(&xs[0]) }
}`
	f := mustParse(t, src)
	fn := f.Items[0].(*FnDecl)
	if len(fn.Generics) != 1 || fn.Generics[0] != "T" {
		t.Fatalf("generics = %v", fn.Generics)
	}
}

func TestParseResultPropagation(t *testing.T) {
	src := `fn parse(s: Str) -> I32!ParseErr {
  Ok(0)
}

fn load(url: Url) -> Page!{NetErr, ParseErr} {
  let body = fetch(url)?
  parse(body)?
  Ok(Page {})
}`
	mustParse(t, src)
}

func TestParseMatchExpr(t *testing.T) {
	src := `fn _classify(n: I32) -> Str {
  match n {
    0 => "zero"
    1..10 => "small"
    _ => "big"
  }
}

fn main() {
  let _zero = _classify(0)
}`
	mustParse(t, src)
}

func TestParseForWhileLoop(t *testing.T) {
	src := `extern {
  fn _work(item: I32) -> Unit!WorkErr
}

fn _process(items: &[I32]) -> Unit!WorkErr {
  for item in items {
    _work(item)?
  }
  while _ready() {
    _step()
  }
  loop {
    _tick()
  }
}`
	mustParse(t, src)
}

func TestParseProtocolAgent(t *testing.T) {
	src := `protocol Echo {
  Ping(msg: Str) -> Str
}

agent Echoer: Echo {
  on Ping(msg) -> msg
}`
	mustParse(t, src)
}

func TestParseSendAskDeadline(t *testing.T) {
	src := `fn driver(logger: Logger, fetcher: Fetcher, url: Url) -> Page!FetchErr {
  logger!Info("started")
  let page = fetcher?Page(url) @2s?
  Ok(page)
}`
	mustParse(t, src)
}

func TestParseUnsafeExtern(t *testing.T) {
	src := `fn _read_byte(addr: USize) -> U8 {
  unsafe {
    let p = raw_ptr(addr)
    p.read()
  }
}

pub unsafe fn _from_raw(ptr: *U8, len: USize) -> Bytes
  requires ptr != null
  requires valid(ptr, len)

fn main() {
  log("17_unsafe")
}`
	mustParse(t, src)
}

func TestParseAttributes(t *testing.T) {
	src := `#[derive(Copy, Hash)]
struct Point { x: F64, y: F64 }`
	mustParse(t, src)
}

func TestParseTurbofish(t *testing.T) {
	src := `fn main() {
  let m = Map::[Str, Json]{}
  let s = Some::[I32](42)
}`
	mustParse(t, src)
}

func TestParseClosure(t *testing.T) {
	src := `fn main() {
  let f = fn(x) { x + 1 }
}`
	mustParse(t, src)
}

func TestParsePrecedence(t *testing.T) {
	// 1 + 2 * 3 should parse as 1 + (2 * 3).
	src := `fn x() { 1 + 2 * 3 }`
	f := mustParse(t, src)
	fn := f.Items[0].(*FnDecl)
	body := fn.Body.(*Block)
	stmt := body.Stmts[0].(*ExprStmt)
	top, ok := stmt.Expr.(*BinExpr)
	if !ok {
		t.Fatalf("expected BinExpr, got %T", stmt.Expr)
	}
	if top.Op != "+" {
		t.Fatalf("top op = %q", top.Op)
	}
	if _, ok := top.RHS.(*BinExpr); !ok {
		t.Fatalf("RHS should be BinExpr (* binds tighter), got %T", top.RHS)
	}
}

func TestParseAgentStateField(t *testing.T) {
	src := `protocol Count {
  Inc() -> I64
}

agent Counter: Count {
  n = 0
  on Inc() -> { n += 1; n }
}`
	mustParse(t, src)
}

func TestParseSupervisor(t *testing.T) {
	src := `supervisor SearchFlow(strategy: one_for_one) {
  child planner = spawn Planner()
  child fetcher = spawn Fetcher(net)

  on_fail(planner) { restart up_to 3 in 30s }
  on_fail(fetcher) { backoff 100ms..2s; restart }
}`
	mustParse(t, src)
}

func TestParseBudget(t *testing.T) {
	src := `extern {
  fn _job(input: Bytes) -> Unit!RunErr
}

fn _run_job(input: Bytes) -> Unit!RunErr {
  budget {
    cpu 150ms
    wall 2s
    mem 128MiB
    mb 1k
  } run {
    _job(input)?
    Ok(())
  }
}

fn main() {
  log("11_budget_block")
}`
	mustParse(t, src)
}

func TestParseArena(t *testing.T) {
	src := `fn turn(input: Str) -> Lowered!ParseErr {
  arena turn {
    let toks = tokenize(input)
    let ast = parse(toks)?
    lower(ast)
  }
}

fn turn_short(input: Str) -> Lowered!ParseErr {
  arena turn: lower(parse(tokenize(input))?)
}`
	mustParse(t, src)
}

func TestParseSandbox(t *testing.T) {
	src := `fn tool_run(input: Bytes) -> Unit!RunErr {
  sandbox ToolRun with {
    fs.read = ["/models", "/tmp/input.json"]
    fs.write = ["/tmp/out"]
    net = ["api.example.com:443"]
    cpu = 150ms
    wall = 2s
    memory = 128MiB
    mailbox = 1k
  } {
    run job(input)?
  }
}`
	mustParse(t, src)
}

func TestParseMacro(t *testing.T) {
	src := `macro assert_eq(a, b) => {
  if a != b { panic("assert_eq failed") }
}

proc macro identity(input: TokenStream) -> TokenStream { input }

fn main() {
  assert_eq!(1 + 1, 2)
}`
	mustParse(t, src)
}

func TestParseUsePackage(t *testing.T) {
	src := `package search_api

use std.http
use std.json
use std.trace

fn main() {
  log("ok")
}`
	mustParse(t, src)
}
