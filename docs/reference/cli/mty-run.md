# `mty run`

Compile a Mighty source file and execute it.

**Slice 7 (`v0.7.0-runtime`):** `mty run` now defaults to the
slice-7 runtime — a tokio-backed concurrent executor with mailboxes,
supervisors, deadline timers, and budget/sandbox enforcement. Pass
`--legacy-interp` to fall back to the slice-6 synchronous interpreter
for diagnostic comparison.

## Usage

```
mty run [--legacy-interp] <file>
```

`<file>` is a single `.mty` source file. Slice 7 does not yet support
package-aware execution; only the items in the named file are visible.

## Process model

`mty run` performs the full slice-1-through-slice-5 pipeline before
executing:

1. Parse + lower to HIR
2. Type check
3. Effect inference + capability subsumption
4. Borrow check
5. MtyIR lowering
6. **Runtime execution** (slice 7): build a `Runtime`, run `main` on
   the slice-6 evaluator inside `tokio::block_on`; any agents spawned
   during `main` use the runtime's per-agent task loops. Long-running
   services that want explicit shutdown control should instead embed
   via the programmatic `sdust_runtime::Runtime` API.

If any earlier stage reports an *error*, `mty run` prints the
diagnostics (Ariadne-style with source spans) and exits **1**.

Otherwise the interpreter starts at the fn named `main`. Slice 6/7's
`main` takes zero arguments and may return:

- `()` / `Unit` — exit 0
- `I32` (or another integer) — that value as the exit code
- `Result::Ok(...)` — exit 0
- `Result::Err(...)` — exit 1 (printed err payload)

A runtime trap (`panic(msg)`, divide-by-zero, missing handler, …)
prints `trap SD5xxx: message` to stderr and exits **1**.

If the program has no `fn main`, `mty run` returns the runtime to
shut down all agents and exits **0** (this matches example 07/08/10
which lack a `main` in their canonical form).

## Exit codes

| Code | Meaning                                                    |
|------|------------------------------------------------------------|
| 0    | Normal completion                                          |
| 1    | Compile error, trap, or `Result::Err` from `main`          |
| 2    | No `main` fn in a slice-6 (`--legacy-interp`) program      |
| 3    | Interpreter step budget exceeded (default 5 000 000 steps) |

## Runtime environment variables

| Variable                  | Effect                                 |
|---------------------------|----------------------------------------|
| `MTY_TRACE=stderr`        | emit JSON telemetry lines to stderr    |
| `MTY_TRACE=file:/path`    | append JSON telemetry to file          |
| `MTY_RUNTIME_THREADS=N`   | tokio worker thread count (default 1)  |
| `MTY_DET_SEED=N`          | (reserved) seed for deterministic mode |
| `MTY_HTTP_MOCK=1`         | (reserved) skip TCP bind for tests     |

The legacy `STARDUST_*` spellings (`STARDUST_TRACE`,
`STARDUST_RUNTIME_THREADS`, …) are still honoured for back-compat
with v0.6-era deployments; the first lookup that falls through to
a `STARDUST_*` name emits a one-shot deprecation warning on stderr.

## Example

```sh
$ cat hello.mty
fn main() {
  log("hello, Mighty")
}

$ mty run hello.mty
hello, Mighty
$ echo $?
0
```

Spawn + ask an agent:

```sh
$ cat echoer.mty
protocol Echo { Ping(m: Str) -> Str }
agent Echoer: Echo { on Ping(m) -> m }

fn main() {
  let h = spawn Echoer()
  let r = h?Ping("hi")
  log(r)
}

$ mty run echoer.mty
hi
```

## Effect handling

Effect calls (`fs.read`, `net.get`, …) are routed through the
runtime's `StdHost`. Slice 7's host honours sandbox allowlists when
they are set on the active budget (host:port for `net.*`, prefix
match for `fs.*`) but does not yet perform real I/O — the call
returns `Unit` after the check. Real I/O wires in slice 8 along with
the codegen.

## Related commands

- `mty check <file>` — parse + lower + type-check without executing
- `mty dump --sir <file>` — print the MtyIR program
- `mty dump --hir <file>` — print the lowered HIR
- `mty dump --ast <file>` — print the AST item summary
- `mty explain SD5xxx` — explain a runtime diagnostic

## Future work (slice 8+)

- Forward command-line args to `main(args: &[Str])`
- Honour environment variables when the program reads `std.env`
- Wire automatic supervisor restart (the policy is in place; the
  orchestrator lands with codegen)
- Real arena allocator (replaces the slice-7 approximate mem budget)
- Native + Wasm codegen via LLVM / Cranelift
