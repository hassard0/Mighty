# `format!()` extended specs — v0.25 Track D notes

v0.24 Track B shipped the `format!()` macro with the four conversion
sigils (`{}`, `{:x}`, `{:X}`, `{:?}`), named-arg passthrough, and brace
escapes. v0.24 deferred all layout flags to v0.25. Track D closes that
gap.

## What ships in v0.25

Track D extends the spec parser + expander + runtime helpers to support
the canonical Rust layout grammar:

```
format_spec := [[fill]align][sign][#][0][width][.precision][type]
```

### New supported specs

| Spec        | Behaviour                                              |
|-------------|--------------------------------------------------------|
| `{:5}`      | minimum width 5 (right-aligned default for numbers)    |
| `{:05}`     | width 5 + zero-padding                                 |
| `{:<5}`     | left-align to width 5                                  |
| `{:>5}`     | right-align to width 5                                 |
| `{:^5}`     | center-align to width 5                                |
| `{:*<5}`    | fill char `*` + left-align to width 5                  |
| `{:.3}`     | precision 3 (floats: decimal places; strings: max)     |
| `{:+}`      | always show sign for numbers                           |
| `{:#x}`     | alternate hex (prefix `0x`)                            |
| `{:#X}`     | alternate HEX (prefix `0x`)                            |
| `{:#b}`     | alternate binary (prefix `0b`)                         |
| `{:#o}`     | alternate octal (prefix `0o`)                          |
| `{:b}`      | binary (no prefix)                                     |
| `{:o}`      | octal (no prefix)                                      |

### Combined specs

The grammar respects Rust's canonical ordering
`[fill][align][sign][#][0][width][.precision][type]`. Combined examples:

| Spec        | Renders                                              |
|-------------|------------------------------------------------------|
| `{:#05x}`   | `0x0ff` for 0xff (alt + zero + width + hex)          |
| `{:+05}`    | `+0001` for 1 (sign + zero + width)                  |
| `{:>10.3}`  | `"     3.142"` for 3.14159 (align + width + precision) |

## Runtime contract

The macro lowers a spec'd placeholder into one of three Mighty
expression shapes, selected by `is_bare_conversion()`:

1. **Bare conversion** (v0.24 fast path):
   ```
   (x).to_str()              // {}
   (x).to_hex_str()          // {:x}
   (x).to_bin_str()          // {:b}    -- new in v0.25
   (x).to_oct_str()          // {:o}    -- new in v0.25
   ```

2. **Spec-helper only** (sign/alt/precision, no width):
   ```
   (x).to_str_spec(sign_plus: Bool, alternate: Bool, precision: U32)
   ```
   Variants: `to_hex_str_spec`, `to_hex_upper_str_spec`,
   `to_debug_str_spec`, `to_bin_str_spec`, `to_oct_str_spec`.
   `precision = 4294967295` (u32::MAX) is the "no precision" sentinel.

3. **Spec-helper + width pad**:
   ```
   (x).to_hex_str_spec(false, true, 4294967295)
       .pad_str(5, '0', "right")        // {:#05x}
   ```

The runtime impls live in `crates/mty-ir/src/interp/run.rs`:
- `render_display_spec` — `to_str_spec` / `to_debug_str_spec`
- `render_radix_spec`   — `to_hex_str_spec` / `to_hex_upper_str_spec`
  / `to_bin_str_spec` / `to_oct_str_spec`
- `pad_str`             — width padding + sign-aware zero-pad

### Alignment defaults — the `"default"` sentinel

When the user writes `{:5}` (width but no explicit align), the
expander can't know at compile time whether the receiver is a number
or a string. The convention is "right for numbers, left for strings",
so the expander emits `pad_str(5, ' ', "default")` and the runtime
helper resolves the sentinel by sniffing the conv-output string with
`looks_numeric()`.

## Diagnostics

| Code     | Trigger                                                      |
|----------|--------------------------------------------------------------|
| MT6002   | template arg-count ≠ count of positional placeholders         |
| MT6009   | malformed template (unbalanced braces, non-literal first arg)|
| MT6010   | format spec uses indexed positional or dynamic width (v0.26) |
| MT6011   | width digit run cannot parse as `u32` (overflow / non-digit) |
| MT6012   | precision digit run cannot parse as `u32` or is empty (`.}`) |

MT6010 was repurposed in v0.25 — the v0.24 catch-all
("not implemented") narrows to *only* the v0.26 follow-ups, and the
new MT6011 / MT6012 cover bad-format cases the parser can recognise.

## Deferred to v0.26

These shapes still raise MT6010 (`UnsupportedSpec`) so callers get a
clear error rather than silent miscompilation:

- **Indexed positional** — `{0} {1} {0}` (argument reuse by index)
- **Dynamic width via arg** — `{:1$}`, `{:.0$}`
- **Asterisk dynamic** — `{:.*}`, `{:*}`

These need an additional pre-pass that tracks indexed arg consumption
and three more runtime methods (`to_*_spec_dyn`) — orthogonal to the
v0.25 work but uses the same `pad_str` tail.

## Implementation map

### Extended

- `crates/mty-macros/src/stdlib/format.rs` — spec parser walks the
  full Rust grammar; `FormatSpec` carries all flags; `render_placeholder`
  emits the conv-spec + pad_str chain
- `crates/mty-macros/tests/format_macro.rs` — adds 17 v0.25 tests
  (30 total, all v0.24 baseline tests preserved)
- `crates/mty-macros/src/diag.rs` — re-exports MT6011 / MT6012
- `crates/mty-diagnostics/src/codes.rs` — MT6011 / MT6012 const + explain
  text; MT6010 explain text rewritten to point at v0.26
- `crates/mty-stdlib/src/fmt.rs` — adds METHOD_BIN / METHOD_OCT,
  `METHOD_*_SPEC`, `METHOD_PAD_STR`, `PRECISION_NONE`, and updated
  `FORMAT_ALL_METHODS` list
- `crates/mty-types/src/prelude.rs` — adds the 9 new method names to
  the permissive built-in method table
- `crates/mty-ir/src/interp/run.rs` — adds dispatch for the 9 new
  method names + the `render_display_spec`/`render_radix_spec`/
  `pad_str`/`looks_numeric`/`split_numeric_prefix`/`format_radix_u128`
  helpers

### New

- `tests/conformance/macros/format_width/` — width + zero-pad fixture
- `tests/conformance/macros/format_precision/` — precision fixture (float)
- `tests/conformance/macros/format_align/` — left/right/center/fill fixture
- `dev/history/notes/FORMAT_EXTENDED_V0_25_NOTES.md` — this file

### Repurposed

- `tests/conformance/macros/format_unsupported_spec/` — input.mty
  updated from `{:05}` (now supported) to `{0}` (still deferred to
  v0.26) so MT6010 still has an exercise fixture.

## Why a `_spec` method per kind, not one polymorphic helper?

Tried a single `to_str_with_spec(kind: Str, ...)` first. The kind
arg is always a literal at the call site, so dispatching by method
name is just as compact and lets the SIR interp pattern-match the
correct radix without parsing the kind string at runtime. Codegen
backends will get a cleaner lowering too — `to_hex_str_spec` is a
direct WIT import target.

## Why does precision use `u32::MAX` instead of `Option[U32]`?

Mighty's `format!` macro emits Rust source-snippet text that gets
re-parsed by the HIR preprocessor. `Option::Some(3)` would either
require importing `Option` at the call site or wrapping in a sentinel
struct that the parser doesn't know about. A bare integer literal
sidesteps both — `u32::MAX` is a known-safe sentinel because no
realistic format precision exceeds even 100.

## Test count delta

- v0.24 baseline: 24 tests
- v0.25 added: 17 v0.25 tests + 4 bonus coverage = 21 net new
- Total integration tests: 30 (matches the 30 listed in the file
  docstring)
- Plus 22 inline `#[cfg(test)] mod tests` cases (v0.24: 22, v0.25: net
  +9) in `format.rs` itself
- Plus 3 new conformance fixtures (`format_width`, `format_precision`,
  `format_align`)

## What v0.25 SHIPPED that v0.24 deferred

All bullets from the v0.24 §"Deferred to v0.25" list:

- [x] Width / zero-padding — `{:5}`, `{:05}`, `{:>5}`, `{:*<5}`
- [x] Precision — `{:.3}` (floats and strings)
- [x] Alignment / fill — `{:>10}`, `{:<10}`, `{:^10}`, `{:*<10}`
- [x] Sign flags — `{:+}`, `{:#x}` (alternate-form prefix)
- [x] Bonus: binary `{:b}`, octal `{:o}`, plus their alt-form variants

## What v0.25 deferred to v0.26

- [ ] Argument-index reuse — `{0} {1} {0}`
- [ ] Dynamic width/precision args — `{:1$}`, `{:.0$}`, `{:.*}`
