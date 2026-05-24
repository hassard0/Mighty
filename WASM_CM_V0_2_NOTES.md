# Wasm Component Model — v0.2 notes

Tracking doc for the v0.2 wave-2 work that closes amendment **A47**:
"full Component Model output is no longer deferred." This file logs the
interpretation calls we made while shipping `mty-codegen-wasm` v0.2 so
later slices can revisit them without spelunking through commits.

## What shipped

- `crates/mty-codegen-wasm/src/wit.rs` — emits a textual `.wit`
  document for every Mighty package being compiled to wasm.
- `crates/mty-codegen-wasm/src/component.rs` — wraps the existing
  slice-8 core module + the new WIT contract into a Component Model
  component via `wit_component::ComponentEncoder`.
- `compile_program_to_file_with_options` — new top-level entry point
  that emits a component by default, or a bare core module when
  `BuildOptions::core_only` is set.
- CLI flag `--no-component` (default = component output) — plumbed
  through `mty-driver::build_wasm` so existing `mty build --target
  wasm32-*` invocations now produce Component Model output.

## Interpretation calls

### 1. WIT is generated, not authored

We do **not** consume a user-supplied `.wit` file in v0.2. Instead, every
build derives a WIT document from the MtyIR program:

| MtyIR shape | WIT shape |
|-----------|-----------|
| Top-level `fn foo(...) -> T` (non-`_` prefixed) | inline `export foo: func(...) -> T;` inside the world |
| `struct Point { x, y }` | `record point { x: ..., y: ... }` |
| `enum Color { Red, Green, Blue }` (no payloads) | `enum color { red, green, blue }` |
| `enum Shape { Circle(f64), Square(f64) }` | `variant shape { circle(f64), square(f64) }` |
| Capability-typed param (`Fs`, `Net`, …) | `import mighty:caps/<family>;` |
| `effects` annotation | informational `// effects: Net, Time` comment in the world |

User-supplied WIT is a v0.3 task. The hooks are there
(`WitDocument::resolve`) but no CLI surface reads it yet.

### 2. Functions are exported inline, not via an interface

The world declaration uses:

```wit
world hello-world {
  import wasi:cli/log;
  export main: func();
}
```

…instead of:

```wit
interface lib { main: func(); }
world hello-world {
  import wasi:cli/log;
  export lib;
}
```

Both are legal but the inline form keeps the core-wasm export name
simple — `main` instead of the canonical-mangled
`mighty:hello/lib#main`. The slice-8 lowerer emits exports with
their bare MtyIR fn-name, so the inline form is a one-for-one match
without us having to teach the lowerer the component-model name
mangling. This is a v0.2 simplicity call; later slices can switch
to the interface-export form if we ever need multiple per-package
interfaces.

### 3. Host stubs are baked into every emitted document

`wit_parser::Resolve` rejects packages whose imports point at
unknown packages, but we don't want to require every build to have
the upstream WASI / mighty:web WIT on disk. So
`append_host_stubs` appends nested-package declarations for
`wasi:cli`, `mighty:web`, and `mighty:caps` to the same string
the `Resolve` parses. The stubs ship the minimum surface the v0.2
backend actually imports:

- `wasi:cli/log` — `log: func(msg: string)`
- `mighty:web/log` — same shape, browser-side
- `mighty:web/dom` — `get-element-by-id`, `set-text` (placeholders)
- `mighty:caps/{fs,net,clock,dom,model}` — minimal method surface

When the real upstream packages drift, we'll need to bump these
stubs. They're not validated against any external schema.

### 4. Capability *family*, not capability *instance*

WIT can express "an interface" but not "a narrowed capability". A
Mighty `Fs<Path("/data")>` and a bare `Fs` both turn into the
same `import mighty:caps/fs;`. Constraint narrowing stays a
runtime concern (the runtime's cap broker honors the MtyIR
`CapConstraint`). Documented as a deliberate omission.

### 5. Effects live in a comment

The Component Model has no first-class concept of an algebraic
effect. We emit the per-function effect set as an
informational comment line on the world:

```wit
world hello-world {
  ...
  // effects: Net, Time
}
```

Downstream tooling (e.g. a future `mty pkg verify`) can lex this
out without breaking other WIT consumers.

### 6. The slice-8 core lowerer's `unreachable` fallback is preserved

Whenever the lowerer can't translate a MtyIR shape, it emits a
single `unreachable` for that fn body so the module still
validates. The component wrapper inherits this — meaning an
emitted component can validate at link time but trap at runtime
when a user actually calls the unsupported function. This is the
same behavior slice 8 shipped; the component layer doesn't change
it.

### 7. Pinned to `wit-component = 0.225` / `wit-parser = 0.225`

The workspace `Cargo.toml` had `0.225` from the v0.2 wave-2 prep
commit. We kept it. Notes on API churn:

- `Resolve::push_str` requires exactly one **top-level** package
  declaration per call. Nested-package syntax (`package foo:bar { ...
  }`) is only legal for additional packages in the same file.
- `ComponentEncoder::module()` reads back the `component-type`
  custom section that `embed_component_metadata` writes — there is
  no separate `with_wit()` API. Embed first, then encode.
- The component preamble is `\0asm` + `[0x0d, 0x00, 0x01, 0x00]`
  (version=13, layer=1), not `[0x01, 0x00, 0x0d, 0x00]` as some
  earlier draft specs suggested.

If `wit-component` ever drops `embed_component_metadata` in favor
of a direct API, swap `component::wrap_as_component` accordingly.

### 8. Canonical core-wasm import names

The slice-8 lowerer originally emitted `(import "mighty" "log"
...)`. To let `ComponentEncoder` match those to the WIT world's
imports, we changed the emitter to use the canonical
component-model name pair:

- `wasm32-wasi` target: `(import "wasi:cli/log" "log" ...)`
- `wasm32-web` target: `(import "mighty:web/log" "log" ...)`

This is a behavior change for callers that pre-existed the
component wrapper. The legacy `compile_program_to_file` entry
point still emits a bare core module (no component) but it now
imports under the new names. If a downstream runtime hardcoded
`(import "mighty" "log" ...)` it'll need to switch to the new
shape.

## Tests

`crates/mty-codegen-wasm/tests/`:

- `wit_generation.rs` (8 tests) — empty + ADT + fn roundtrip
- `component_validate.rs` (3 tests) — emit → wasmparser-validate
- `roundtrip_core_module.rs` (3 tests) — `--no-component` path
- `target_imports.rs` (3 tests) — wasi:* vs mighty:web:*
- `common.rs` — fixture helpers
- `sourcemap.rs` — added by the parallel mty-debuginfo agent

## Post-v0.2 backlog

- **Full WASI Preview 2** — we stub `wasi:cli/log` only. Real builds
  on Preview 2 need `wasi:io/streams`, `wasi:filesystem/types`, etc.
  Wire `wasi-cli`/`wasi-io`/`wasi-filesystem` proper packages once the
  Mighty stdlib needs them.
- **User-authored WIT** — accept an `--wit <file>` flag or a
  `wit/` directory in the package and *merge* the user's exports
  with our generated ones (or override entirely).
- **Resource types** — Mighty agents map cleanly to component
  resources. v0.2 lowers agents to opaque `i32` handles; v0.3 should
  emit `resource agent { ... }` and the borrow/own discipline.
- **Component linking** — `wit_component::Linker` could let us
  produce single-file fat components from a Mighty package with
  pkg dependencies. v0.2 emits one component per source file.
- **`cabi_realloc` / shadow stack** — emitted by the lowerer? Right
  now we don't, and the canonical ABI can't lower returning-strings
  without it. Fine for v0.2 (no fn returns a string yet); blocks
  v0.3 stdlib bindings.
- **jco / wit-bindgen polish** — confirm the emitted component
  works under jco for browser deploys and wasmtime for native.
  Smoke test scripts live in v0.3 backlog.
