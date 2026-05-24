# LSP v0.5 Notes (interpretation calls)

Working notes captured during the v0.5 LSP build. Decisions that affect
how the protocol surfaces translate to the compiler pipeline.

## 1. Single-file rename, period

`textDocument/rename` ships **single-file only** in v0.5. The HIR's
resolve pass doesn't yet emit a per-occurrence ResolveMap surfaced in
a way the LSP can consume without re-walking; multi-file rename also
needs a workspace-wide DefMap merge that we haven't built. Decision:
ship the protocol handler today, restrict to the current file, document
the gap. The editor preview lets the user catch any wrong-rename.

For top-level items we walk the file's CST and find every IDENT whose
text matches the target name. Conservative — it may rewrite an IDENT
that just happens to shadow a top-level def — but the user's
preview-then-apply loop makes this acceptable.

## 2. Inlay hints: HIR×CST lockstep pairing

We don't have a CST-node → HIR-id map (or vice versa). For `let`
hints we pair `HirStmt::Let` entries from each fn body's HIR block
with the `LET_STMT` children of the corresponding CST `BLOCK`, in
declaration order. This works when the HIR lowering doesn't drop or
reorder lets — which is the v0.5 invariant. If lowering changes (e.g.
desugars `let pat = expr` into multiple stmts), the pairing skews; the
test suite catches the simple case.

Parameter hints use `TypedPackage::fn_params[fid]` which is keyed by
HIR `FnId`; we look up the fn by name (matching `HirFn::name` against
the CST `NAME` child of `FN_DECL`).

## 3. Semantic tokens vs TextMate grammar

The VS Code extension keeps the TextMate grammar for fallback / search
highlighting. Semantic tokens overlay on top. Where the two disagree
the LSP wins (e.g. user-defined types vs primitives — TextMate paints
all PascalCase IDENTs as "type", LSP distinguishes
`defaultLibrary`-modified primitives from user types).

## 4. Code actions: code-string matching

We match LSP-side diagnostics by their `code: NumberOrString::String`
field. The Stardust compiler renders `SDxxxx` strings via
`DiagCode::as_str()`. If we ever migrate to numeric LSP codes,
`code_actions.rs` needs updating.

When the client passes an empty `diagnostics` list (some editors
populate the lightbulb that way), we re-scan our cached doc diagnostics
for entries overlapping the request range and generate fixes from
those. This means the user gets the fix immediately on first paint,
not after a round-trip.

## 5. Signature help: inclusive end-byte for ARG_LIST

`rowan::TextRange::contains` is half-open (`[start, end)`), but a
cursor right after the `(` of a call site sits AT the L_PAREN.end
byte. We accept that boundary explicitly (`pos >= start && pos <=
end`) so the help pops up immediately on `(`.

Active-parameter is computed by counting depth-1 commas between the
`(` and the cursor, NOT by walking the parsed `ARG` children — the
parser may produce zero ARGs when the body is just `(|)` mid-edit,
and we still want help to show with active_param=0.

## 6. Receiver-aware completion: name → ADT lookup

For `recv.<cursor>`, we look up `recv` as either a top-level fn
parameter (via `TypedPackage::fn_params`) or a `HirStmt::Let` binding
whose init we can find in `expr_ty`. We then enumerate
`DefMap::impl_methods`, `DefMap::traits.by_method`, and the ADT's
field list. We do **not** yet handle:

- Field-chain receivers (`a.b.c.|`): only the immediate binding name
  is resolved.
- Method-call receivers (`a.foo().|`): MethodCall expressions aren't
  hooked through to their result types in the LSP layer.

These are tracked for the next slice.

## 7. Workspace folders: event surface, single-file analysis

We advertise `workspaceFolders.supported = true` and handle
`workspace/didChangeWorkspaceFolders` + `workspace/didChangeWatchedFiles`
notifications (just logging). The underlying analysis is still
per-file — building a workspace-wide resolve map is a larger refactor
that crosses into `sdust-driver`. Out of scope for v0.5; tracked.

## 8. SD6001 / `unknown_macro`

The task brief lists SD6001 unknown_macro as a code-action target.
That diagnostic code isn't yet allocated in
`sdust-diagnostics::codes` (codes stop at SD8010 in the codegen
range; no macro-specific code exists yet — the macros crate raises
SD1001 / SD2007-style codes today). When the macros work lands its
own code we'll wire the corresponding quick-fix.

## 9. Tests run against `DocAnalysis` directly, not JSON-RPC

All v0.5 tests construct a `DocAnalysis` and call the feature module
directly. We do not exercise the JSON-RPC transport in unit tests
(would need a full tower-lsp duplex setup). This matches the v0.2
pattern; the editor smoke test is the `editor/vscode` install.

## 10. CRLF / LF on Windows

Git is configured to auto-convert; we write LF in source files and
rely on `core.autocrlf`. The LSP's UTF-16 line index handles both
`\n` and `\r\n` (the `\r` is treated as a regular char on its line).

## 11. Multi-line semantic tokens get byte-length fallback

A semantic token that spans multiple source lines (rare: only
block-style comments classify this way today) has its `length` field
filled with the byte count rather than the per-line UTF-16 length.
Clients tolerate this for comment-style tokens; revisit if other
multi-line tokens appear.
