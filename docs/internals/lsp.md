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
│   ├── completion.rs       # textDocument/completion (v0.5: locals + receiver)
│   ├── semantic_tokens.rs  # textDocument/semanticTokens/full + /range
│   ├── rename.rs           # textDocument/rename + prepareRename
│   ├── references.rs       # shared find-references helper
│   ├── inlay_hints.rs      # textDocument/inlayHint
│   ├── code_actions.rs     # textDocument/codeAction
│   ├── signature_help.rs   # textDocument/signatureHelp
│   └── server.rs           # Backend impl LanguageServer + run_stdio()
└── tests/
    ├── integration.rs            # original v0.2 feature smoke tests
    ├── semantic_tokens.rs        # legend + token classification
    ├── rename_local.rs           # local + prepareRename + invalid-ident
    ├── rename_top_level.rs       # top-level fn / struct rewrite
    ├── inlay_hints.rs            # inferred-type vs annotated
    ├── code_action_unresolved.rs # SD2021 quickfix suggestions
    ├── signature_help.rs         # call-site active-param tracking
    ├── workspace_folders.rs      # multi-doc independence
    └── completion_semantic.rs    # locals + receiver completion
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

The v0.2/v0.5 LSP intentionally skips the borrow check. Two reasons:

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

**v0.2/v0.5 limitation:** top-level only. The HIR resolve pass produces
a name → DefRef map, but the per-expression resolution side-tables
aren't exposed in a form the LSP can consume. A future amendment will
surface them.

## Completion (v0.5)

`completion::complete(doc, position)` returns the union of:

1. The full Stardust keyword set (57 entries) tagged
   `CompletionItemKind::KEYWORD`.
2. Every name in `DefMap::by_name` tagged with the appropriate kind
   (Function / Struct / EnumMember / Module / TypeParameter).
3. If the character immediately preceding the cursor is `.`:
   - **Receiver-aware suggestions:** the receiver name is looked up in
     the smallest enclosing fn body for a `let`-binding or parameter
     whose inferred type is an ADT; we then list every `impl_methods`
     entry, every trait-provided method registered in
     `DefMap::traits.by_method`, and every public field of that ADT.
   - **Built-in method fallback:** the entire
     `DefMap::builtin_methods` table is always added so editors still
     surface common methods when the receiver is unresolved.
4. **Locals in scope:** walks `LET_STMT` nodes in the smallest enclosing
   `BLOCK` / `ON_HANDLER` whose `text_range().end() <= cursor` plus the
   `FN_PARAM` siblings of the enclosing `FN_DECL`. We deliberately
   operate off the CST so the suggestions survive partial parses.

Deduplication: items are keyed by `(kind, label)` so a local named the
same as a top-level def doesn't appear twice.

## Semantic tokens (v0.5)

`semantic_tokens::{full, range}` walks the CST and emits LSP semantic
tokens. The legend is declared statically:

```
types:      keyword type function variable parameter string number
            comment operator namespace enumMember typeParameter
            macro property
modifiers:  declaration readonly defaultLibrary
```

Classification rules:

- Keyword tokens (`is_keyword()` on `SyntaxKind`) → `keyword`.
- Numeric / string / comment literals → `number` / `string` / `comment`.
- Punctuation operators (`+`, `==`, `->`, …) → `operator`.
- `IDENT` tokens: looked up first against
  `DefMap::by_name` (yielding `function` / `type` / `enumMember` /
  `namespace` / `typeParameter`); if not present, the built-in primitive
  type names (`I32`, `String`, `Option`, …) classify as `type` with the
  `defaultLibrary` modifier; an `!` next sibling promotes to `macro`;
  IDENTs under `FN_PARAM` get `parameter`; everything else falls back
  to `variable`.
- The `declaration` modifier fires when the IDENT is the `NAME` child
  of a declaration node (`FN_DECL`, `STRUCT_DECL`, …, `STRUCT_FIELD`,
  `ENUM_VARIANT`).

Tokens are sorted by `(line, start_char)` and delta-encoded as required
by the LSP wire format (`SemanticToken { delta_line, delta_start,
length, token_type, token_modifiers_bitset }`). Multi-line tokens emit
their byte length as a conservative fallback for `length`; clients
still highlight them correctly because the deltas line up.

`semantic_tokens/range` returns the same encoding filtered to tokens
whose start position falls within the requested viewport — simpler than
maintaining a per-viewport cache and plenty fast for editor-sized files.

## Rename (v0.5)

`rename::prepare(doc, pos)` returns a
`PrepareRenameResponse::Range` for the IDENT under the cursor, or
`None` to make the editor reject the rename cleanly when the cursor
isn't on an identifier.

`rename::rename(uri, doc, pos, new_name)`:

1. Validates `new_name` via `references::is_valid_ident` — must match
   `[A-Za-z_][A-Za-z0-9_]*` and not be a reserved keyword.
2. Classifies: if the target name resolves in `DefMap::by_name` (and
   isn't a generic `Param`), treat as **top-level**; otherwise **local**.
3. **Top-level**: walk the file, collect every IDENT whose text equals
   the target name.
4. **Local**: restrict the walk to the smallest enclosing `BLOCK` or
   `ON_HANDLER` containing the declaration offset.
5. Emit a `WorkspaceEdit { changes: {uri → [TextEdit; N]} }` (no
   `documentChanges` — slice-1 single-file model).

**Limitations:** single-file only; shadowing inside the same block is
renamed together (the user gets to confirm in the editor preview).

## Inlay hints (v0.5)

`inlay_hints::inlay_hints(doc, viewport)` emits `: T` annotations:

- For each top-level `HirFn` with a body, pair its `HirStmt::Let`
  statements (declaration order) with the `LET_STMT` children of the
  fn's CST `BLOCK`. When the HIR stmt has `ty: None` and `init:
  Some(eid)` and the CST stmt has no type-node child, look up
  `expr_ty[eid]` and render via `pretty_ty`. The hint is positioned at
  the end of the pattern's CST range.
- For each `FN_PARAM` without a type annotation, look up the fn's
  inferred parameter type via `TypedPackage::fn_params[fid]` and emit
  a `: T` hint after the binding IDENT.
- Hints whose position falls outside `viewport` are filtered.
- Uninteresting types (`Error`, unsolved `Var`, naked `Param`) are
  suppressed to avoid noise on broken programs.

**Deferred to a follow-up:** closure parameter hints (CST `LAMBDA_EXPR`
isn't yet paired with HIR's `HirExpr::Lambda`), argument-name hints
(would need per-call-site overload resolution).

## Code actions (v0.5)

`code_actions::code_actions(uri, doc, range, diagnostics)` matches the
diagnostic codes the editor passes along with the request:

| code | fix                                                                    |
|------|------------------------------------------------------------------------|
| SD2021 unresolved value  | suggest top-3 in-scope names by edit distance ≤ 2 |
| SD2002 unresolved type   | suggest top-3 in-scope type names by edit distance |
| SD3001 use-after-move    | suggest inserting `.clone()` after the moved value |
| SD4001 effect undeclared | suggest adding `effect { name }` to the fn signature |

When the client sends an empty `diagnostics` list (some editors do
this when first painting the lightbulb), we re-scan our own cached
diagnostic list for entries whose primary span overlaps `range` and
generate fixes from those. Distance is Levenshtein (`O(m·n)` DP),
threshold 2.

Each action returns a single-edit `WorkspaceEdit` with
`is_preferred: Some(true)` so editors can auto-apply on the first
`auto fix` keystroke.

## Signature help (v0.5)

`signature_help::signature_help(doc, pos)`:

1. Locate the smallest enclosing `CALL_EXPR` / `METHOD_CALL_EXPR` whose
   `ARG_LIST` contains the cursor (inclusive of end-byte so cursors
   sitting right after the open `(` count).
2. Count commas at depth 1 between the open `(` and the cursor — that's
   the active parameter index.
3. For `CALL_EXPR`, take the callee's last IDENT and look it up: a
   `DefRef::Fn` renders the full signature; a `DefRef::Variant`
   renders the variant's payload as a constructor call.
4. For `METHOD_CALL_EXPR`, prefer the matching `builtin_methods` entry
   (yields a synthetic `.method(arg0, arg1)` label); otherwise scan
   `impl_methods` across every ADT and surface every match (each as a
   separate `SignatureInformation`).

**Deferred:** per-overload disambiguation by receiver type (we list
every candidate today), generic-arg-aware signatures, doc-comment
extraction into `documentation`.

## Workspace folders (v0.5)

The server advertises `workspaceFolders.supported = true` and accepts
`workspace/didChangeWorkspaceFolders` + `workspace/didChangeWatchedFiles`
notifications. The implementation logs the event but **does not yet
build a cross-file resolve map** — each `.sd` file remains its own
translation unit inside the LSP. This is enough that VS Code's
"open folder" UX works (every `.sd` file opens individually) and the
event surface is in place for the next slice's multi-file work.

## tower-lsp version pin

`tower-lsp = "0.20"` is pinned in `[workspace.dependencies]`. Internally
it depends on `lsp-types = "0.94"` and re-exports it as
`tower_lsp::lsp_types`. The LSP crate uses that re-export rather than
depending on `lsp-types = "0.97"` directly — the trait bounds on
`Client::send_notification` etc. are keyed on the version tower-lsp was
compiled against, so the types must match.

If/when tower-lsp upgrades to lsp-types 0.97+, we can drop the alias.

## Testing strategy

Integration tests under `crates/sdust-lsp/tests/` construct a
`DocAnalysis` directly and invoke each feature module, asserting on the
lsp-types result. This catches the same regressions as a full JSON-RPC
round-trip but stays fast and deterministic.

Run:

```bash
cargo test -p sdust-lsp
```

v0.5 ships 45 tests:

- 10 unit tests (line_index UTF-16, code-action edit distance,
  semantic-token encoding, ident validation).
- 11 v0.2 integration tests (diagnostics, hover, definition,
  completion, formatting, incremental change application).
- 5 semantic_tokens tests.
- 5 rename_local tests + 2 rename_top_level.
- 3 inlay_hints tests.
- 2 code_action_unresolved tests.
- 3 signature_help tests.
- 1 workspace_folders smoke test.
- 3 completion_semantic tests (keywords + locals + after-dot).

Add a regression test for every bug fix.
