# Chapter 18 — Taint types

Examples:
[`33_taint_basics.mty`](https://github.com/hassard0/Mighty/blob/main/examples/33_taint_basics.mty),
[`34_taint_untaint.mty`](https://github.com/hassard0/Mighty/blob/main/examples/34_taint_untaint.mty).

This is Mighty's compiler-checked **prompt injection prevention**
mechanism, shipped in v0.30 Track A. The pitch:

> The only language where prompt injection is a compile error.

## The shape of the attack

Every "prompt injection broke our agent" report has the same
shape:

```
[user-controlled input] -> [LLM] -> [reply] -> [exec sink]
                                              ^^^^^^^^^^^^
                                              pwned here
```

The reply travels through normal `Str` plumbing —
concatenations, formats, struct fields, method chains — and
reaches a sink (`fs.write`, `process.Command::arg`,
`sql.execute`, `net.Request::body`) that treats it as code or
trust anchor.

## The model — `Tainted[T]`

`Tainted[T]` is a built-in generic wrapper type. Source can name
it; only stdlib sources mint values.

**Stdlib sources of taint:**

- `LlmProvider::complete()` returns `Tainted[Str]`.
- `McpClient::call_tool()` returns `Tainted[Json]`.
- `std.http::Response::body()` returns `Tainted[Bytes]`.
- `std.env::var()` returns `Option[Tainted[Str]]`.

**Sinks that reject `Tainted`:**

- `std.fs::write(path, data: Bytes)` — `data` must be plain.
- `std.process::Command::arg(s: Str)` — `s` must be plain.
- `std.sql::execute(stmt: Str)` — `stmt` must be plain.
- `std.net::Request::body(b: Bytes)` — `b` must be plain.

The compiler reports **MT4099** at the first call where tainted
data would reach a sink without an approved exit. Any combinator
(`format!`, `concat`, struct-field copy, method chain) that
touches a `Tainted` value produces a `Tainted` result —
propagation is transitive.

## The three exits

Mighty does not pretend you never need to untaint. It forces
you to be explicit about it.

### 1. A sanitiser fn

```mty
fn html_escape(s: Tainted[Str]) -> Str {
  // ... explicit, audited body that produces a clean Str
}
```

The fn signature says *"I take tainted in, I produce clean out"* —
its body is the audit boundary.

### 2. `Untaint::after`

```mty
let cleaned = Untaint::after(reply, "audit: structured-JSON parse below");
```

`Untaint::after(value, justification)` returns the inner `T` with
an audit-string attached to the trace. The justification appears
in the replay log and in `mty inspect --cost`'s span metadata.

### 3. `@taint_transparent`

```mty
@taint_transparent
fn parse_int(s: Tainted[Str]) -> Result[I32, ParseErr] {
  // ... declared to be exhaustively analysed; produces a clean Result
}
```

For fns whose body is small enough to manually audit and whose
return type makes the cleanness obvious (e.g. `Result[I32,
ParseErr]` cannot carry a prompt-injection payload).

## A failing example

```mty
fn handle_request(client: AnthropicClient, q: Str) {
  let reply = client.complete("claude", "", q, 1024);   // Tainted[Str]
  std.fs.write(Path("./out.txt"), reply.into_bytes())   // MT4099
}
```

```
error[MT4099]: tainted data flows into a sink without sanitisation
  --> src/handler.mty:3:3
   |
 3 |   std.fs.write(Path("./out.txt"), reply.into_bytes())
   |                                   ^^^^^^^^^^^^^^^^^^ tainted here
   |
note: value originates at the LLM call on line 2
note: three approved exits: a sanitiser fn, `Untaint::after`, or `@taint_transparent`
```

## Try it

```bash
mty check examples/33_taint_basics.mty   # expected: MT4099 (deliberate negative case)
mty check examples/34_taint_untaint.mty  # expected: ok (positive case via approved exits)
```

`33_taint_basics.mty` carries the `@compile-error` marker: the
file is the **negative case**, intentionally failing with
`MT4099` so readers see what the diagnostic looks like.
`34_taint_untaint.mty` is the **positive case** showing the three
approved exits in action.

## See also

- [`docs/internals/taint-types.md`](../internals/taint-types.md) —
  full implementation walkthrough.
- The
  [SWE-bench harness](https://github.com/hassard0/Mighty/blob/main/bench/swe/README.md)
  uses taint types to prevent a misbehaving LLM from patching
  outside the workspace via `fs.write`.
