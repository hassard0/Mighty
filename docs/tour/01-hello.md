# 01 — Hello, Stardust

Every Stardust program starts with the same shape: a `fn main` that the
host calls. The standard library exposes a `log` function for line-based
host I/O.

## The program

```sd
fn main() {
  log("hello, Stardust")
}
```

## What is interesting

- `fn main()` declares the entry point. The empty parameter list does not
  request any capabilities; a host program that does I/O would take
  capability parameters such as `fs: Fs` or `net: Net` (see
  [chapter 10](10-capabilities.md)).
- `log(...)` is a free function. Stardust does not have implicit `print`;
  side-effecting builtins are named and may carry effect annotations
  (see [the spec §9](../spec/v0.1.md)).
- The body has no trailing semicolon. Stardust uses block-as-expression
  semantics: the last expression of a block is its value. A `Unit`-typed
  body, like this one, just discards the trailing value.

## Run it

```bash
sdust check examples/01_hello.sd
```

Expected output:

```
ok: examples/01_hello.sd
```

You can also see what the compiler builds:

```bash
sdust dump --hir examples/01_hello.sd
```

The HIR dump shows a single `fn main` whose body is a one-statement block
calling `log` with a string literal.

## Type errors you might see

The type checker enforces that `log` takes a single `Str`:

```sd
fn main() { log(42) }   // SD2001 expected `Str`, found `{integer}`
fn main() { log() }     // SD2005 function expects 1 argument(s), got 0
```

## Next

Continue to [02 — Types](02-types.md).
