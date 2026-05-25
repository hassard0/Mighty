# 01 — Hello, Mighty

Every Mighty program starts with the same shape: a `fn main` that the
host calls. The standard library exposes a `log` function for
line-based host I/O.

## The program

```mty
fn main() {
  log("hello, Mighty")
}
```

## What is interesting

- `fn main()` declares the entry point. The empty parameter list does
  not request any capabilities; a host program that does I/O would
  take capability parameters such as `fs: Fs` or `net: Net` (see
  [chapter 10](10-capabilities.md)).
- `log(...)` is a free function. Mighty does not have implicit
  `print`; side-effecting builtins are named and carry effect
  annotations (see [spec §9](../spec/v1.0-rc.md)).
- The body has no trailing semicolon. Mighty uses block-as-expression
  semantics: the last expression of a block is its value. A
  `Unit`-typed body, like this one, just discards the trailing value.

## Try it

```bash
mty check examples/01_hello.mty
mty run   examples/01_hello.mty
```

Expected output:

```
ok: examples/01_hello.mty
hello, Mighty
```

You can also see what the compiler builds:

```bash
mty dump --hir examples/01_hello.mty
```

The HIR dump shows a single `fn main` whose body is a one-statement
block calling `log` with a string literal.

## Type errors you might see

The type checker enforces that `log` takes a single `Str`:

```mty
fn main() { log(42) }   // MT2001 expected `Str`, found `{integer}`
fn main() { log() }     // MT2005 function expects 1 argument(s), got 0
```

Run `mty explain MT2001` for the full Cause/Example/Fix/Spec block.

## Next

Continue to [02 — Types](02-types.md).
