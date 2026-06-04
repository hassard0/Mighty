# `mty run`

Compile a Mighty source file and execute it.

**Slice 7 (`v0.7.0-runtime`):** `mty run` defaults to the slice-7
runtime, a tokio-backed concurrent executor with mailboxes,
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
6. Runtime execution: build a `Runtime`, run `main` on the evaluator
   inside `tokio::block_on`, and run spawned agents on the runtime's
   per-agent task loops.

If any earlier stage reports an error, `mty run` prints diagnostics
with source spans and exits 1.

Otherwise execution starts at the fn named `main`. `main` takes zero
arguments today and may return:

- `()` / `Unit` - exit 0
- `I32` or another integer - that value as the exit code
- `Result::Ok(...)` - exit 0
- `Result::Err(...)` - exit 1, with the err payload printed

A runtime trap (`panic(msg)`, divide-by-zero, missing handler, and so
on) prints `trap SD5xxx: message` to stderr and exits 1.

If the program has no `fn main`, `mty run` returns the runtime to shut
down all agents and exits 0. This matches examples 07, 08, and 10,
which lack a `main` in their canonical form.

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
| `MTY_DET_SEED=N`          | reserved seed for deterministic mode   |
| `MTY_HTTP_MOCK=1`         | reserved skip-TCP-bind flag for tests  |

The legacy `STARDUST_*` spellings (`STARDUST_TRACE`,
`STARDUST_RUNTIME_THREADS`, and so on) are still honoured for
back-compat with v0.6-era deployments; the first lookup that falls
through to a `STARDUST_*` name emits a one-shot deprecation warning on
stderr.

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

`mty run` JIT-compiles with Cranelift and falls back to the interpreter
per-program for any stdlib surface that does not yet have native
codegen. The fall-back is transparent: the program runs and produces
the same result as `--legacy-interp` — you never see a crash for an
unimplemented surface.

**Lowered natively** (run on the Cranelift path):

- `std.fs.*` — `read`, `read_to_string`, `read_dir` / `list_dir`,
  `write`, `exists`, `metadata`, the `read_dir` iterator, and their
  documented aliases, via the runtime's native ABI symbols.
- `std.crypto` digests — `sha256`, `sha512`, `blake3`, `hmac_sha256`.
- `std.encoding` encoders — `hex.encode`, `base64.encode`,
  `base64.encode_url_no_pad`.

**Interpreter-hosted** (the whole program transparently falls back to
the interpreter when one of these is called): `std.url`, `std.uuid`,
`std.regex`, `std.crypto` AEAD (`aes_gcm`, `chacha20_poly1305`) and
`random_bytes`, the `std.encoding` decoders, and other not-yet-native
stdlib modules. These run with the same capability checks and behaviour
as `--legacy-interp`; native codegen for them is incremental
(see `dev/history/releases/`).

## Related commands

- `mty check <file>` - parse + lower + type-check without executing
- `mty dump --sir <file>` - print the MtyIR program
- `mty dump --hir <file>` - print the lowered HIR
- `mty dump --ast <file>` - print the AST item summary
- `mty explain SD5xxx` - explain a runtime diagnostic

## Future work

- Forward command-line args to `main(args: &[Str])`
- Honour environment variables when the program reads `std.env`
- Wire automatic supervisor restart; the policy is in place, and the
  orchestrator lands with codegen
- Native capability ABI for host-backed stdlib calls in JIT/AOT output
