# LSP (sdust-lsp)

Live editor integration for Stardust. Implements LSP 3.17 over stdio
via [`tower-lsp`](https://crates.io/crates/tower-lsp) 0.20.

This is the **internals** doc. The user-facing CLI reference lives at
[`docs/reference/cli/sdust-lsp.md`](../reference/cli/sdust-lsp.md).

## Crate layout

```
crates/sdust-lsp/
├── Cargo.toml
├── src/
│   ├── lib.rs              # module wiring, re-exports tower_lsp::lsp_types
│   ├── line_index.rs       # UTF-16-aware (line, char) ↔ byte offset
│   ├── conv.rs             # span/severity/diagnostic → lsp-types
│   ├── docs.rs             # DocAnalysis cache + apply_change
│   ├── diagnostics.rs      # build PublishDiagnosticsParams
│   ├── hover.rs            # textDocument/hover
│   ├── definition.rs       # textDocument/definition
│   ├── completion.rs       # textDocument/completion
│   └── server.rs           # Backend impl LanguageServer + run_stdio()
└── tests/
    └── integration.rs      # one test per feature
```

## Pipeline

Every `didOpen` / `didChange` re-runs the full compiler pipeline:

```
source: String
   │
   ▼
parse_source()  ──►  ParsedFile { source, green, parse_errors }
   │
   ▼
lower()         ──►  (Package, lower_diags)
   │
   ▼ (if no lowering errors)
check_package_typed()  ──►  TypedPackage { def_map, ty_arena,
                                           expr_ty, fn_params, fn_ret,
                                           diagnostics }
   │
   ▼
DocAnalysis { source, version, line_index, parsed, package, typed,
              diagnostics }
```

The cached `DocAnalysis` is shared between feature handlers via
`Arc<DocAnalysis>` (so a slow `hover` call doesn't lock the next
`didChange`).

## Borrow check

The v0.2 LSP intentionally skips the borrow check. Two reasons:

1. **Latency.** The borrow checker walks HIR linearly per fn body; it
   roughly doubles per-change analysis time on typical files. For an
   editor that runs the pipeline on every keystroke, that's enough to
   feel sluggish.
2. **Dep weight.** `sdust-borrow` pulls in additional state; the LSP
   crate keeps its dep set lean (only what the in-scope features need).

Borrow-check diagnostics still surface via `sdust check`. A future
amendment will incrementalize the borrow checker so the LSP can include
its output without the latency hit.

## UTF-16 line index

The compiler pipeline uses **UTF-8 byte offsets** for every span. LSP
positions are **UTF-16 code units**. The conversion lives in
`line_index.rs`:

- `LineIndex::new(source)` walks the source once, recording each line's
  starting byte offset.
- `offset_to_position(source, byte) → (line, char)` binary-searches the
  line table, then sums `c.len_utf16()` over the chars from line-start to
  `byte` to produce the column.
- `position_to_offset(source, line, char) → byte` is the inverse:
  walk chars from line-start, accumulating UTF-16 code units until we
  hit `char`, then return the byte index.

Tests cover ASCII round-trip, multibyte UTF-8 (`café`), surrogate-pair
emoji (U+1F600), and out-of-range clamping.

## DocStore

`DocStore` is a `DashMap<Url, Arc<DocAnalysis>>`. Three operations:

- `open(uri, source, version)` — runs the pipeline and inserts.
- `update(uri, source, version)` — replaces the existing entry with a
  freshly-analyzed one. Old `Arc<DocAnalysis>` values may still be held
  by in-flight feature requests; they drop when those finish.
- `close(uri)` — removes the entry.

Incremental edits are applied **before** re-analysis, by
`docs::apply_change`. We don't currently incrementalize the parser
itself (it's fast enough for editor-sized files) — we just re-parse the
new full source after applying the patches.

## Hover

`hover::hover(doc, position)` finds the rowan token at the byte offset
and:

- If the token is an `IDENT` and the name resolves in the DefMap,
  renders a one-line signature (fn signature, struct/enum decl, variant,
  module, or type param).
- Otherwise renders the token's literal text in a code fence.

Every hover also includes the parent CST node kind for debuggability.

## Definition

`definition::definition(uri, doc, position)` finds the identifier at the
cursor and looks it up in the top-level item list. Returns the item's
`SourceSpan` translated to an LSP `Range`.

**v0.2 limitation:** top-level only. The HIR resolve pass produces a
name → DefRef map, but the per-expression resolution side-tables aren't
exposed in a form the LSP can consume. A future amendment will surface
them.

## Completion

`completion::complete(doc, position)` always returns:

1. The full Stardust keyword set (57 entries) tagged
   `CompletionItemKind::KEYWORD`.
2. Every name in `DefMap::by_name` tagged with the appropriate kind
   (Function / Struct / EnumMember / Module / TypeParameter).
3. If the character immediately preceding the cursor is `.`, every
   name in `DefMap::builtin_methods` tagged `Method`.

Real semantic completion (locals-in-scope, fields of the receiver type,
trait methods) is deferred.

## Formatting

`server::Backend::formatting` runs `sdust_fmt::format(green)` against
the cached green tree and returns either:

- `vec![]` if the output equals the current source (no change), or
- a single `TextEdit` covering `(0,0)` to end-of-buffer with the
  formatted text.

This matches the behavior of `sdust fmt --stdin`.

## tower-lsp version pin

`tower-lsp = "0.20"` is pinned in `[workspace.dependencies]`. Internally
it depends on `lsp-types = "0.94"` and re-exports it as
`tower_lsp::lsp_types`. The LSP crate uses that re-export rather than
depending on `lsp-types = "0.97"` directly — the trait bounds on
`Client::send_notification` etc. are keyed on the version tower-lsp was
compiled against, so the types must match.

If/when tower-lsp upgrades to lsp-types 0.97+, we can drop the alias.

## Testing strategy

Integration tests in `crates/sdust-lsp/tests/integration.rs` construct
a `DocAnalysis` directly and invoke each feature module, asserting on
the lsp-types result. This catches the same regressions as a full
JSON-RPC round-trip but stays fast and deterministic.

Run:

```bash
cargo test -p sdust-lsp
```

15 tests today (4 line_index unit, 11 integration). Add a regression
test for every bug fix.
