# `sdust run`

Compile a Stardust source file and execute it under the slice-6
interpreter.

## Usage

```
sdust run <file>
```

`<file>` is a single `.sd` source file. Slice 6 does not yet support
package-aware execution; only the items in the named file are visible.

## Process model

`sdust run` performs the full slice-1-through-slice-5 pipeline before
executing:

1. Parse + lower to HIR
2. Type check
3. Effect inference + capability subsumption
4. Borrow check
5. SIR lowering
6. Interpret

If any earlier stage reports an *error*, `sdust run` prints the
diagnostics (Ariadne-style with source spans) and exits **1**.

Otherwise the interpreter starts at the fn named `main`. Slice 6's
`main` takes zero arguments and may return:

- `()` / `Unit` — exit 0
- `I32` (or another integer) — that value as the exit code
- `Result::Ok(...)` — exit 0
- `Result::Err(...)` — exit 1 (printed err payload)

A runtime trap (`panic(msg)`, divide-by-zero, missing handler, …)
prints `trap SD5xxx: message` to stderr and exits **1**.

If the program has no `fn main`, `sdust run` exits **2** with a `NoMain`
status.

## Exit codes

| Code | Meaning                                                    |
|------|------------------------------------------------------------|
| 0    | Normal completion                                          |
| 1    | Compile error, trap, or `Result::Err` from `main`          |
| 2    | No `main` fn in the program                                |
| 3    | Interpreter step budget exceeded (default 1 000 000 steps) |

## Example

```sh
$ cat hello.sd
fn main() {
  log("hello, Stardust")
}

$ sdust run hello.sd
hello, Stardust
$ echo $?
0
```

## Effect handling

Effect calls (`fs.read`, `net.get`, …) are routed through a host
adapter. `sdust run` uses `RealHost`, which prints stdout/stderr to the
real terminal but returns deterministic stub values for every effect.
**Slice 6 does not perform real I/O on the effect surfaces**; that
arrives in slice 7 when the runtime ships.

## Related commands

- `sdust check <file>` — parse + lower + type-check without executing
- `sdust dump --sir <file>` — print the SIR program
- `sdust dump --hir <file>` — print the lowered HIR
- `sdust dump --ast <file>` — print the AST item summary

## Future work (slice 7+)

- Forward command-line args to `main(args: &[Str])`
- Honor environment variables when the program reads `std.env`
- Wire up the work-stealing scheduler and the real mailbox runtime
- Enforce budget/sandbox declarations at run time
