# `format!()` builtin macro — v0.24 Track B notes

Track B for v0.24 ships the `format!()` string-interpolation macro. The
gap closed is Track D's #2 from v0.23: today `let s = format!("score:
{}", score)` emits `MT6001 unknown macro: format`. The agent's
workaround was hand-rolled `"prefix: " + n.to_str()` chains, which got
unwieldy fast.

## What ships in v0.24

The `format!` call expands at compile time (HIR preprocessor pass) into
a Mighty source snippet that the next preprocess pass re-parses:

```mty
format!("count: {} of {}", x, total)
//   ↓ expands to ↓
("" + "count: " + (x).to_str() + " of " + (total).to_str())
```

### Supported format-spec subset

| Spec      | Conversion method      | Notes                              |
|-----------|------------------------|------------------------------------|
| `{}`      | `.to_str()`            | positional, default conversion     |
| `{name}`  | `(name).to_str()`      | named-arg passthrough (in scope)   |
| `{:x}`    | `.to_hex_str()`        | positional, lowercase hex          |
| `{:X}`    | `.to_hex_upper_str()`  | positional, uppercase hex          |
| `{:?}`    | `.to_debug_str()`      | positional, debug rendering        |
| `{n:x}`   | `(n).to_hex_str()`     | named-arg with conversion          |
| `{n:X}`   | `(n).to_hex_upper_str()`| named-arg with conversion         |
| `{n:?}`   | `(n).to_debug_str()`   | named-arg with debug               |
| `{{` / `}}` | literal `{` / `}`    | escape                             |

The Rust convention applies: `{x}` is a *named-arg passthrough* to the
in-scope identifier `x`, NOT positional hex. The hex sigil only fires
through the `:x` / `:X` spec form.

### Diagnostics

| Code     | Trigger                                                      |
|----------|--------------------------------------------------------------|
| MT6002   | template arg-count ≠ count of positional placeholders         |
| MT6009   | malformed template (unbalanced braces, non-literal first arg)|
| MT6010   | format spec not yet supported (width/precision/align)         |

MT6001 (`unknown macro`) is suppressed for `format!` even when no
declarative `macro format(...) => { ... }` is in scope — the
preprocessor recognises it via `mty_macros::stdlib::BUILTIN_MACRO_NAMES`.

### Builtin macro shadowing rule

A user-defined `macro format(...) => { ... }` declaration takes priority
over the builtin. The HIR preprocessor checks the declarative registry
first; only when the registry has no `format` entry does the builtin
fire. This keeps the door open for downstream projects that want
domain-specific `format!` semantics without losing back-compat.

## v0.25 follow-ups (deferred specs)

The parser deliberately rejects the following with MT6010 so callers
get a clear error rather than silent miscompilation:

- **Width / zero-padding** — `{:5}`, `{:05}`, `{:>5}`, etc.
- **Precision** — `{:.3}`, `{:.*}` (float decimal places, string length cap)
- **Alignment / fill** — `{:>10}`, `{:<10}`, `{:^10}`, `{:*<10}`
- **Sign flags** — `{:+}`, `{:#x}` (prefix hex with `0x`)
- **Argument-index reuse** — `{0} {1} {0}` (Rust supports indexed positional)
- **Dynamic width/precision args** — `{:1$}`, `{:.0$}`

These need three new runtime methods (sketched, not implemented):

```mty
to_str_pad(width: USize, fill: Char, align: Align) -> String
to_str_precision(places: USize) -> String   // floats only
to_str_radix(radix: U8, upper: Bool) -> String  // generalised hex
```

## Implementation map

The slice is intentionally additive — no existing files were rewritten,
only extended.

### New

- `crates/mty-macros/src/stdlib/format.rs` — template parser, expander,
  `FormatExpandError` enum, integration test surface
- `crates/mty-macros/tests/format_macro.rs` — 22 integration tests
- `crates/mty-stdlib/src/fmt.rs` — runtime-contract docs + canonical
  method-name constants for codegen backends
- `tests/conformance/macros/format_basic/` — positive `run` fixture
- `tests/conformance/macros/format_unsupported_spec/` — MT6010 negative
- `tests/conformance/macros/format_arity/` — MT6002 negative

### Extended

- `crates/mty-macros/src/stdlib.rs` — `pub mod format` + `is_builtin_macro`,
  `expand_builtin_macro`, `BUILTIN_MACRO_NAMES`
- `crates/mty-macros/src/lib.rs` — re-export the builtin-macro helpers
- `crates/mty-macros/src/diag.rs` — `MACRO_FORMAT_BAD_TEMPLATE` (MT6009)
  and `MACRO_FORMAT_UNSUPPORTED_SPEC` (MT6010) re-exports
- `crates/mty-diagnostics/src/codes.rs` — MT6009 / MT6010 catalog
  entries + explain text
- `crates/mty-hir/src/lower/macros.rs` — `collect_builtin_macro_calls`,
  preprocessor rewrite branch, `diag_format_error` mapping
- `crates/mty-types/src/prelude.rs` — `to_hex_str`, `to_hex_upper_str`,
  `to_debug_str` added to the permissive built-in method table
- `crates/mty-ir/src/interp/run.rs` — runtime dispatch for the three new
  conversion methods (delegates to Rust `Display`/`LowerHex`/`UpperHex`)
- `crates/mty-stdlib/src/lib.rs` — `pub mod fmt;` registration

## Why not a declarative `macro format(...) => { ... }`?

Tried that first. The format template has its own grammar
(`{}`/`{x}`/`{name:?}`/`{{`/...) that interleaves with the trailing
positional argument list. Pure token substitution can splice an
argument into a position the template *picks*, but it can't walk the
template to do the picking. A "declarative macro that takes a string
literal and rewrites it" needs ad-hoc parser logic — exactly what
[`fmt_macro::parse_template`] supplies. Routing through a code-driven
builtin keeps the existing declarative-macro infra unchanged and gives
us a single, testable, swappable expander.

## Runtime fast-path note

The conversion methods are dispatched by name in the SIR interpreter
(`mty-ir/src/interp/run.rs`, the big match in `eval_method`). The
typechecker accepts them on any receiver via the permissive
`builtin_methods` table in `mty-types::prelude`, returning a fresh type
variable — same shape as the existing `to_str`/`as_str`/`len` family.

When the wasm32-web / cranelift backends grow real codegen for these
methods, they should pattern-match against `mty_stdlib::fmt::FORMAT_CONV_METHODS`
to ensure the four method names stay in sync. The CLI / WIT story is
out of scope for v0.24 — `mty-stdlib::fmt` is documentation + constants
only, not WIT-imported.
