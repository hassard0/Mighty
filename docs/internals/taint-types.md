# Taint Types — Compiler-Checked Prompt Injection Prevention (v0.30)

> "The only language where prompt injection is a compile error."

This document describes Mighty's **taint type system**, shipped in
v0.30 Track A. Taint types let the compiler statically reject programs
that pipe untrusted data (LLM responses, MCP tool results, HTTP
bodies, environment variables) into sensitive sinks (file writes,
process arguments, SQL queries, network request bodies) without
explicit sanitisation.

## Why

Every credible report of "prompt injection broke our agent" has the
same shape:

```text
[user-controlled input] ---> [LLM] ---> [reply] ---> [exec sink]
                                                     ^^^^^^^^^^^^
                                                     pwned here
```

The reply travels through normal `Str` plumbing — concatenations,
formats, struct fields, method chains — and reaches a sink (`fs.write`,
`process.Command::arg`, `sql.execute`, `net.Request::body`) that
treats it as code or as a trust anchor. Manual code review is the only
defence today, and it consistently misses one chain in fifty.

Mighty's taint type system makes the compiler do the chain analysis.

## The model

### `Tainted[T]`

`Tainted[T]` is a built-in generic wrapper type, registered in
`crates/mty-types/src/prelude.rs`. It is *not* a user-definable type:
no `struct Tainted[T] { ... }` declaration in source code can shadow
the prelude entry. Only stdlib sources mint `Tainted[T]` values.

Source code can name `Tainted[T]` in type annotations:

```mty
let user_input: Tainted[Str] = agent.ask("hi")
let mcp_result: Tainted[Value] = mcp_client.call_tool("search", args)
```

### Sources

Specific stdlib calls return `Tainted[T]`:

| Surface                              | Return type        |
|--------------------------------------|--------------------|
| `Member.ask(prompt)`                 | `Tainted[Str]`     |
| `client.messages(req)` (anthropic)   | `Tainted[Message]` |
| `client.responses(req)` (openai)     | `Tainted[Message]` |
| `client.generate_content(req)`       | `Tainted[Message]` |
| `client.converse(req)` (bedrock)     | `Tainted[Message]` |
| `mcp::Client::call_tool(name, args)` | `Tainted[Value]`   |
| `std.http.get(url).body`             | `Tainted[Str]`     |
| `std.http.post(url, body).body`      | `Tainted[Str]`     |
| `std.env.var(name)`                  | `Tainted[Option[Str]]` |
| `std.env.args()`                     | `Tainted[List[Str]]` |
| `std.fs.read_to_string(path)`        | `Tainted[Str]` when `path` is tainted (transitive) |

### Propagation

Any operation that mixes a `Tainted[T]` with anything else produces
`Tainted[U]`:

- `format!("{} {}", clean, dirty)` → `Tainted[Str]`
- `clean + dirty` (string concat) → `Tainted[Str]`
- `dirty.to_string()` → `Tainted[Str]`
- `dirty.field` → `Tainted[U]`
- `if cond { dirty } else { clean }` → `Tainted[Str]`
- `match x { _ => dirty }` → `Tainted[Str]`
- `let y = dirty` → `y: Tainted[Str]`

The propagation lattice is monotonic: once tainted, always tainted
until untainted.

### Sinks

Sinks reject `Tainted[T]` with diagnostic **MT4099**:

| Sink                                  | Sensitive arg |
|---------------------------------------|---------------|
| `std.fs.write(path, contents)`        | `contents` (index 1) |
| `process::Command::arg(arg)`          | every arg (index 0..n) |
| `std.sql.execute(query)`              | `query` (index 0) |
| `net::Request::body(body)`            | `body` (index 0) |

A sink call that receives a tainted value emits:

```text
error[MT4099]: tainted value flows to `std.fs.write` (arg #1) — untaint
               via `.matches_regex(...)`, `.in_allowlist[Enum]()`, or
               `.sanitize_with(HtmlEscape | ShellEscape | SqlEscape |
               PathBoundary(...))`
```

### Untainting

There are exactly three sanctioned ways to convert `Tainted[T]` → `T`:

#### 1. `value.matches_regex(pattern)`

Returns `Option[Str]`. `Some(s)` if the value matched the regex, where
`s` is provably constrained by the regex shape (so e.g. `^[a-z]+$`
guarantees no shell metacharacters). `None` if the value did not
match. The match value is untainted.

```mty
let raw = std.env.var("USER")
let safe = raw.matches_regex("^[a-zA-Z0-9_]+$").unwrap_or("anon")
std.fs.write("/tmp/u.txt", safe)   // OK
```

#### 2. `value.in_allowlist[Enum]()`

Returns `Option[Enum]`. `Some(v)` if the value parses as one of the
enum's declared variant names (e.g. `Verdict::SAFE`). `None`
otherwise. The variant is untainted because the enum's variants are
finite and explicit.

```mty
enum Verdict { SAFE, UNSAFE, UNCLEAR }
let raw = std.env.var("VERDICT")
let v = raw.in_allowlist().unwrap_or("UNCLEAR")
std.fs.write("/tmp/v.txt", v)   // OK
```

#### 3. `value.sanitize_with(sanitiser)`

Returns the sanitised `Str`. Four built-in sanitisers ship with the
stdlib:

- `HtmlEscape`    — escapes `<`, `>`, `&`, `"`, `'`.
- `ShellEscape`   — wraps in single quotes, escapes embedded `'`.
- `SqlEscape`     — escapes `'`, `\`, doubles backslashes.
- `PathBoundary`  — strips `..`, leading `/`, NUL bytes.

```mty
let raw = std.env.var("MSG")
let safe = raw.sanitize_with(HtmlEscape)
std.fs.write("/tmp/msg.html", safe)   // OK
```

User code can add new sanitisers by implementing the `Sanitizer`
trait — see [Sanitizer extension](#sanitizer-extension) below.

### What is NOT an untainting operation

There is **no** `Tainted::unwrap_unchecked()`. This is deliberate:
the marketing claim — "the only language where prompt injection is
a compile error" — depends on the absence of an escape hatch.

The implicit untaint path described in the next section applies only
to non-sink calls (logging / printing); it does not give the
programmer a general way to bypass sink rejection.

### Implicit untaint: logging

`log(...)`, `print(...)`, `eprintln(...)`, `panic(...)`, and `dbg(...)`
implicitly untaint their argument. Printing a tainted value is a
display action — it does not execute the value, so it does not need
explicit sanitisation:

```mty
let raw = std.env.var("X")
log(raw)            // OK — implicit untaint for logging
log(raw.to_string())  // also OK
```

Logger-style methods (`logger.log(...)`, `logger.info(...)`,
`logger.warn(...)`, `logger.error(...)`, `logger.debug(...)`,
`logger.trace(...)`) follow the same rule.

### Effect-row integration

A fn parameter declared `cap: untainted` cannot accept `Tainted[T]`
at any nesting level. This is the "fast path" for fns that don't
deal with external data — the compiler can prove no taint flows
through them.

```mty
fn write_static(path: Path, msg: Str cap: untainted) {
  std.fs.write(path, msg)
}
```

(The `cap: untainted` form is reserved syntax in v0.30; the
machinery is in place but the parser-level surface lands in v0.31.)

## Architecture

### The pass

The taint-flow analysis is a post-type-check pass in
`crates/mty-types/src/taint.rs`. It walks every fn body + agent
handler, maintains a per-binding tainted-flag stack, and reports
MT4099 at sink call sites.

```text
   HIR + TypedPackage
         |
         v
   build_def_map (prelude includes Tainted[T])
         |
         v
   typeck (permissive method dispatch — opaque ADT)
         |
         v
   cap_check  (capability subsumption)
         |
         v
   taint::check   <-- MT4099 emitted here
         |
         v
   borrow check + lowering
```

The pass uses a simple "tainted? yes/no" lattice per `ExprId`
rather than a full `Tainted[T]` type-system threading, because the
permissive method-dispatch path (`synth_method_call`) in the
type-checker returns a fresh inference variable for every method
call on an opaque ADT. Threading a wrapper through every
type-checker call site would touch dozens of files for a v0.30
slice; the dedicated pass achieves the same observable semantics in
one focused module.

### Why a separate pass, not a TyData variant

We deliberately did *not* add `TyData::Tainted(TyId)`. Three reasons:

1. **Permissive method dispatch.** Mighty's stdlib surface uses
   opaque ADTs with permissive method dispatch (`Member.ask`,
   `client.messages`, etc.) — return types are fresh inference
   variables. A type-system-level `Tainted` wrapper would lose the
   tag on every such call and have to be re-introduced by every
   stdlib method. The post-pass walks the HIR directly and can read
   the call shape without needing typeck cooperation.

2. **Backward compat.** Every existing example calls `Member.ask`,
   `std.env.var`, etc. and treats the return as plain `Str`.
   Switching the type would break ~30 existing examples + demos.
   The pass-based design keeps the surface type as `Str` but
   tracks taint as a parallel side-table — examples that don't
   touch sinks (the vast majority) are unaffected.

3. **One marketing line.** "Prompt injection is a compile error"
   needs ONE clear MT4099. A TyData variant would surface as
   MT2001 (type mismatch) at each sink, with the same generic
   "expected `Str`, found `Tainted[Str]`" message. MT4099 lets us
   say exactly what's wrong and how to fix it.

The trade-off: the pass cannot reason about taint through fn
boundaries (a user fn that takes a `Tainted[Str]` and returns it
would need the type-system wrapper to communicate intent across the
call boundary). v0.30 takes the conservative stance: any user fn
that *takes* a tainted arg is assumed to *return* a tainted value.
This is sound (no false negatives at sinks) but adds a small
false-positive surface that v0.31 will narrow.

### Implementation details

The pass is structured as:

```rust
struct TaintCx<'pkg> {
    pkg: &'pkg Package,
    expr_tainted: HashMap<ExprId, bool>,
    scopes: Vec<HashMap<String, bool>>,   // per-binding flag
    ctor_source: Vec<HashMap<String, String>>,  // local -> stdlib ADT name
    diagnostics: Vec<Diagnostic>,
}
```

The `ctor_source` table records "this local was constructed from
`T.ctor(...)` where T is a recognised stdlib ADT name". When a later
method call routes through the local (e.g. `let m = Member.anthropic(...)`
then `m.ask(...)`), the pass looks up the local's source type
(`Member`) to dispatch the source/sink table on
`(receiver_type, method)` rather than on the local's name.

Mighty's HIR lowering emits all dotted-path calls — including the
`receiver.method(...)` sugar — as `HirExpr::Call { callee: Path([...]) }`
rather than `HirExpr::MethodCall`. The pass handles both shapes:

- `Call(Path([Member, anthropic]))` → recognised ctor.
- `Call(Path([m, ask]))` where `m` is a known-ctor local → routes to
  the `Member.ask` source table.
- `Call(Path([std, fs, write]))` → recognised sink callee.
- `MethodCall { receiver, method, .. }` → handled by the explicit
  method-call arm (used for chained calls like `(x + y).method()`).

## Sanitizer extension

User code can add new sanitisers by implementing the `Sanitizer`
trait:

```mty
trait Sanitizer {
  fn sanitize(self, input: Str) -> Str
}

struct UrlEscape
impl Sanitizer for UrlEscape {
  fn sanitize(self, input: Str) -> Str {
    // Replace `&`, `?`, `=`, `#`, space with %xx escapes.
    ...
  }
}

fn main() {
  let raw = std.env.var("PARAM")
  let safe = raw.sanitize_with(UrlEscape)
  // ... use `safe` at a sink ...
}
```

The compiler treats any `Sanitizer`-shaped type as a valid argument to
`sanitize_with`. The taint-flow pass treats `sanitize_with(_)` as an
unconditional untaint, regardless of which sanitiser was supplied:
correctness of the sanitiser is the implementer's responsibility, the
same way `unsafe fn` is the implementer's responsibility.

## Migration

v0.30 introduces taint tracking on top of the existing v0.29 stdlib
surface. The migration story for existing code:

1. Code that doesn't call sinks is **unaffected** — most examples
   and demos fall in this bucket.
2. Code that pipes LLM responses to `log()` is **unaffected** —
   logging is an implicit untaint.
3. Code that pipes LLM responses to `std.fs.write` (or another sink)
   needs to add an explicit untainting call:
   `.sanitize_with(HtmlEscape)` for HTML, `.matches_regex(...)` for
   structured inputs, etc.

We did **not** add a `--allow-tainted-leak` compiler flag that would
downgrade MT4099 to a warning. A flag would dilute the marketing
claim ("compile error, unless you set this flag, in which case
still maybe"). Migration cost: the v0.30 release notes list the
small handful of paths in `examples/` and `demos/` that needed the
explicit untaint addition.

## Future work

- **v0.31 — fn-boundary taint propagation.** Annotate user fns with
  taint-row signatures: `fn process(input: Tainted[Str]) -> Tainted[Str]`
  declares that the fn preserves taint; `fn process(input: Str) -> Str`
  with a body that touches a tainted value is a type error.
- **v0.32 — `Tainted[T]` in protocol signatures.** Agent messages
  can carry tainted payloads explicitly: `protocol P { Process(Tainted[Str]) -> Str }`.
- **v0.33 — Tainted-IR-shape borrow check.** The borrow checker
  treats tainted aliases differently for the purposes of
  Sendable-cross-agent-boundary check (currently every aliased
  Sendable can cross; v0.33 will reject sending a tainted reference
  to an agent that doesn't accept tainted inputs).

## Reference

- Diagnostic: `crates/mty-diagnostics/src/codes.rs` — `MT4099 =
  TAINTED_VALUE_TO_SINK`. `mty explain MT4099` for the full text.
- Pass: `crates/mty-types/src/taint.rs`.
- Tests:
  - `crates/mty-types/tests/taint_basics.rs` — introduction +
    propagation + handler-safe + log-implicit-untaint.
  - `crates/mty-types/tests/taint_sinks.rs` — positive + negative
    per-sink pairs + cross-sink + ctor-source chaining.
  - `crates/mty-types/tests/taint_untaint.rs` — `matches_regex`,
    `in_allowlist`, `sanitize_with` (all four default sanitisers).
  - `crates/mty-types/tests/taint_propagation.rs` — HIR-shape
    propagation (if / match / tuple / struct / field / question).
- Examples:
  - `examples/33_taint_basics.mty` — negative case (compile error).
  - `examples/34_taint_untaint.mty` — positive case (sanitised
    paths).
- Demo: `demos/08_swarm_review/src/main.mty` — the reviewer agent's
  LLM responses are tracked as `Tainted[Str]` and routed through
  `log()` (implicit untaint) for display.
