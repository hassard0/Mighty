# Diagnostics

Diagnostics are span-tracked, severity-tagged messages with a stable
`SDxxxx` code. They live in
[`crates/sdust-diagnostics/`](../../crates/sdust-diagnostics/).

## Types

```rust
pub enum Severity { Error, Warning, Note, Help }

pub struct Label {
    pub start: usize,
    pub end: usize,
    pub message: String,
}

pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
    pub primary: Label,
    pub secondary: Vec<Label>,
    pub notes: Vec<String>,
    pub helps: Vec<String>,
}
```

The builder pattern:

```rust
Diagnostic::error(codes::EXPECTED_ITEM, primary_label)
    .with_note("agents must be declared at top level")
    .with_help("did you mean to wrap this in `fn main`?")
    .with_secondary(other_label);
```

## Codes

`DiagCode(pub u16)` with a formatted `as_str()` of `SD{:04}`. All
codes are defined as `pub const` in
[`codes.rs`](../../crates/sdust-diagnostics/src/codes.rs). Once
assigned, a code is never renumbered.

See the [diagnostic registry](../reference/diagnostics.md) for the
canonical table.

## Rendering

```rust
render::ariadne::render(&diagnostic, source_id, source) -> String
render::ariadne::render_all(&diagnostics, source_id, source) -> String
```

The renderer wraps [ariadne](https://docs.rs/ariadne) and produces a
colorized (when stderr is a TTY), span-highlighted report. Severity
maps to ariadne's `ReportKind`; primary labels render red, secondary
yellow.

## Producing diagnostics

Two main pathways:

1. **Parser errors** — `Parser::error_at(message, start, end)` records a
   `ParseError`. The driver converts these into `Diagnostic` values
   with code `UNEXPECTED_TOKEN` (`MT0001`).
2. **HIR lowering** — `LoweringCtx.diagnostics: Vec<Diagnostic>` is
   pushed to directly by lowering submodules with the appropriate
   `SD1xxx` code.

The driver merges both lists into the final diagnostics returned to
the CLI.

## Future

- Source-mapped JSON output for editor / LSP integration.
- Severity policy per profile (warnings-as-errors in strict mode).
- Suggested-edit metadata so `sdust fmt` can apply auto-fixes.
