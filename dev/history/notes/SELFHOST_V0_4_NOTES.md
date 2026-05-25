# Self-hosting v0.4 — language gap catalog

This file is the running log of v0.3 language gaps discovered while
porting the lexer to Mighty source. Every entry has:

- a minimal reproducer
- the v0.3 behavior
- the expected v0.5+ behavior
- the workaround used in `selfhost/lexer/lexer.sd`

See [`docs/internals/self-hosting.md`](docs/internals/self-hosting.md)
for the v0.4 status overview and roadmap, and
[`selfhost/README.md`](selfhost/README.md) for how to run the
bootstrap test.

## 1. Loops are single-iteration in the interpreter (DOMINANT BLOCKER)

**Reproducer:**

```sd
fn main() effect io {
  let mut i = 0
  loop {
    log("iter")
    i = i + 1
    if i >= 5 { break }
    log("notbreak")
  }
  log("done")
}
```

**v0.3 output:**

```
iter
notbreak
done
```

(One iteration, then unconditional fall-through to the loop exit.)

**Root cause:** `crates/mty-sir/src/lower/exprs.rs` lowers `while`,
`loop`, and `for` with their body's terminator set to
`Term::Goto(exit)`, not back to the loop header. The lowerer's
comment is explicit:

```rust
// Slice 6: avoid infinite loops at run time by jumping straight to
// exit. The borrow checker proved the loop terminates structurally;
// the interpreter relies on body side-effects making `cond` go false.
fb.set_term(Term::Goto(exit));
```

(`lower_while`, `lower_loop`, and `lower_for` all do the same thing.)

**Workaround in lexer.sd:** none — the lexer source uses honest `loop`
+ `break` constructs because that is the correct semantic shape. The
v0.4 bootstrap test asserts the first token round-trips and defers the
full diff to v0.5.

**v0.5 fix:** real Goto(header) at the loop tail + a step budget to
cap pathological runs. The native and Wasm backends already do this
correctly; only the tree-walking interpreter cuts the back-edge.

## 2. `!fn(args)` triggers MT2008 "value of type Bool is not callable"

**Reproducer:**

```sd
fn is_space(b: U32) -> Bool { b == 32 }

fn main() {
  let b = 32
  if !is_space(b) { log("not space") }
}
```

**v0.3 output:** `MT2008 Error: value of type Bool is not callable`

**Root cause:** unary `!` binds tighter than postfix `(args)`. So
`!is_space(b)` parses as `(!is_space)(b)`. The type checker sees the
`!` applied to a function value (which it lowers to `Bool`), then sees
the call `(Bool)(b)` and flags MT2008.

**Workaround:** introduce the call into a let binding first.

```sd
let space = is_space(b)
if space == false { log("not space") }
```

`lexer.sd` uses this pattern uniformly (`let space = is_space(b); if
space == false { break }`).

**v0.5 fix:** the parser should treat `unary_op call_expr` as
`unary_op(call_expr)` — same precedence rule as Rust, Swift, JS, etc.

## 3. `extern { fn ... }` short-circuits to `return Unit`

**Reproducer:**

```sd
extern {
  fn host_call(s: Str) -> Unit
}

fn main() {
  host_call("hello")
}
```

**v0.3 behavior:** `host_call("hello")` produces no host-visible side
effect. The Rust-side `Host::extern_call` is never invoked.

**Root cause:** `crates/mty-sir/src/lower/items.rs` (around line 269)
turns extern fns with no body into trivial-return MtyIR functions:

```rust
let body = match &f.body {
    Some(b) => *b,
    None => {
        // Extern / trait-method-without-body: emit a trivial return.
        let unit = Operand::Const(Const::Unit);
        fb.set_term(Term::Return(unit));
        install_fn(ctx, sir_id, fb, &f.name, ret_ty, f.span.clone(), Some(hir_id));
        return;
    }
};
```

So the call resolves to a real user fn (`FnRef::User(sir_id)`) whose
body is a single `return Unit`. The `BuiltinId::Extern(name)` path —
which DOES dispatch through `Host::extern_call` — only fires for
*unresolved* fn paths.

**Workaround in lexer.sd:** route the bridge through the registered
prelude module `std.io`. The HIR -> MtyIR lowerer (`receiver_module_path`
in `crates/mty-sir/src/lower/exprs.rs`) recognises module-typed
receivers and rewrites `std.io.<method>(args)` into
`Stmt::EffectInvoke { effect: io, op: GenericCall { path, method }, args }`
which `Host::effect_call` services.

**v0.5 fix:** lower `extern { fn name(...) }` bodies into a
synthetic `BuiltinId::Extern("name")` call so the host extern table
sees real invocations.

## 4. No cross-file module resolution

**Reproducer:** put `pub enum SyntaxKind { ... }` in `a.sd`, write
`fn classify(k: SyntaxKind) -> Str { ... }` in `b.sd`, run
`mty check b.sd`.

**v0.3 output:** `MT2002 Error: cannot find type SyntaxKind in scope`

**Root cause:** the v0.3 driver compiles a single file at a time. The
`use a.SyntaxKind` mechanism is parsed but not yet wired through to
`def_map` cross-file. `mty-pkg` is the staging ground for v0.5
module resolution.

**Workaround in lexer.sd:** consolidate every type and helper the
lexer needs into one file (`lexer.sd`). The decomposed shape lives in
`lib.sd` + `syntax_kind.sd` as the v0.5 spec.

**v0.5 fix:** wire `mty-pkg`'s module table into the resolver so
`use selfhost_lexer.SyntaxKind` resolves transparently.

## 5. Permissive `Str` method stubs

**Reproducer:**

```sd
fn main() {
  let s = "hello"
  if s.contains("ell") { log("yes") } else { log("no") }
}
```

**v0.3 output:** `no` (whereas the correct output is `yes`).

**Root cause:** `crates/mty-sir/src/interp/run.rs` `eval_method`:

```rust
"contains" | "starts_with" | "ends_with" => Bool(false),
```

A literal blanket false. Same goes for many other names in the
"permissive defaults" table.

**Workaround in lexer.sd:** the lexer never calls `.contains`,
`.starts_with`, or `.ends_with`. Keyword recognition is done via a
full `match` on the captured identifier string (which DOES work —
the `match` on `Str` literal patterns compiles and routes correctly
in the interpreter). Byte-level peeking is done through the host
bridge (`std.io.lex_byte_at(i) -> U32`).

**v0.5 fix:** real `Str` methods backed by interpreter intrinsics that
actually call into the underlying Rust `String` impls.

## 6. `if x { y } else { z }` as a value in `let mut` declaration

**Reproducer:**

```sd
fn main() {
  let mut b1: U32 = if true { 1 } else { 256 }
}
```

**v0.3 status:** _not directly tested in v0.4_ — the `lexer.sd` source
splits this into

```sd
let mut b1: U32 = 256
if start + 1 < n { b1 = host.lex_byte_at(start + 1) }
```

for clarity and to avoid the if-as-expression-with-typed-let path
which the interpreter's `Stmt::Assign` lowering hasn't been hardened
against.

**v0.5 fix:** test + harden the lowering.

## 7. `return` from inside `if` inside `loop`

The punctuation-scan ladder in `lexer.sd` uses

```sd
if b0 == 46 && b1 == 46 && b2 == 61 {
  emit("DOT_DOT_EQ", start, start + 3)
  return start + 3
}
```

`return` from inside an `if` inside a `pub fn` works (type-checks +
MtyIR-lowers cleanly). Listed here because the pattern is load-bearing
for the lexer and we want it on the watch-list for v0.5 regression
testing.

## How to add a gap

When extending the self-hosting effort, if you hit a v0.3 limitation:

1. Reduce it to a 5-line reproducer in
   `selfhost/lexer/_probe.sd` (gitignored).
2. Document the v0.3 output, the root cause file, and the workaround
   you used.
3. Add an entry here with the next sequential number.
4. If the gap blocks a runtime behavior, gate the related bootstrap
   test with `#[ignore = "v0.5 — <one-line summary>"]`.
