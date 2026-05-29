# LSP hover (v0.33 T6, v0.34 T3 expansion)

The LSP hover provider returns a Markdown payload that drives the box VS
Code / JetBrains / Neovim show when the user rests on an identifier. As
of v0.34 T3 it stitches together signatures from the type-checker with a
curated **stdlib examples index** so the hover for `Member.ask` (or any
of the 200+ other seeded stdlib surfaces) renders:

- a fenced signature,
- a one-line description,
- the required capability (when any),
- a small usage example,
- and a "See also" list of related symbols.

This document explains how the pieces fit together. The user-facing
reference for hover content lives in `docs/reference/lsp.md`.

## Why a curated index

The stdlib's runtime lives in the Rust crate `mty-stdlib` (Strategy A —
see `docs/internals/stdlib.md`). The doc generator in `mty-doc` walks
`.mty` source and harvests `///` doc-comments, so it never reaches
`Member::ask`. The hover provider needs an authoritative source of
`std.*` documentation that does **not** depend on a Mighty-side
declaration.

v0.33 T6 introduced that source as a flat compile-time table in
`crates/mty-doc/src/examples.rs` seeded with 58 entries; v0.34 T3 grew
that catalog to 200+ entries spanning `std.rag` (Index/Doc/Retriever/
Reranker/Rag + the four `ChunkStrategy` variants), `std.computer`
(Mouse/Keyboard/Screen, every `ComputerAction` variant, `Dispatcher`,
`ComputerCap` bounds + deny-list), `std.swarm` internals
(`SharedDollarBudget`, `Consensus` accessors, `SimilarityMode`),
`std.observe` query API (`Window`, `GroupBy`, `summarize`,
`percentiles`), the four `std.taint` sanitisers (`HtmlEscape`,
`ShellEscape`, `SqlEscape`, `PathBoundary`) plus the three untainting
methods, `std.eval` comparators + `Verdict` variants + `Case` sources,
`std.web` (`Canvas` drawing surface + `Input`/`Key`), `std.fs`
read/stat/list/exists + `FsCap`, every `std.json` `Value` variant, and
the foundational `std.string` + `std.vec` method surface. The same
shape applies:

```rust
pub const STDLIB_EXAMPLES: &[StdlibExample] = &[
    StdlibExample {
        symbol: "Member.ask",
        signature: "fn Member.ask(&self, prompt: Str) -> Result<MemberReply, LlmError>",
        description: "Sends prompt to the LLM provider and returns the reply.",
        capability: "net.https (for the provider endpoint)",
        example: "let m = Member.anthropic(\"claude-opus-4-7\");\nlet r = m.ask(\"Capital of France?\")?;\nlog(r.text);\n",
        see_also: "Member.anthropic, Member.openai, std.swarm, swarm",
    },
    /* ... */
];
```

Each entry carries six `&'static str` fields. The table lives in the
read-only data segment — there is no per-startup allocation, no JSON
parsing, no async I/O on the hover path. v0.34 will migrate to Strategy
B (real `.mty` source for the stdlib) and the index will be derived from
those files automatically; until then, the table is hand-curated and
covered by snapshot-style tests (uniqueness, presence of signature +
example body, hash determinism — see `examples::tests`).

## Persistence

`persist_examples_index()` writes the table to
`~/.mty/examples-index.json` on first use. The on-disk shape is:

```json
{
  "version": 1,
  "hash": "<fnv1a-64>",
  "examples": [
    { "symbol": "...", "signature": "...", "description": "...",
      "capability": "...", "example": "...", "see_also": "..." },
    ...
  ]
}
```

The hash is content-based (FNV-1a 64 over every field of every entry in
declaration order). External tooling (`mty doc explain`, future MCP doc
servers) can validate the cache by comparing
`stdlib_examples_hash()` against the on-disk value and rebuild on a
mismatch. The persistence step is best-effort — when the LSP runs in a
sandbox with no writable `HOME`/`USERPROFILE`, the in-memory table is
still consulted and the hover continues to work; only the on-disk cache
is skipped.

## Hover pipeline

`mty_lsp::hover::hover` does the following on each request:

1. Resolve `(uri, position)` to the SyntaxToken under the cursor via
   the cached `LineIndex`.
2. If the token is an `IDENT`, do two independent renderings and
   concatenate them with a blank line:
   - **User DefMap path** — look the bare name up in
     `doc.typed.def_map.by_name`. When it resolves to a user-declared
     fn/struct/enum/etc, render its signature via `pretty_ty`.
   - **Stdlib examples path** — see below.
3. Append the surrounding node-kind and token-kind for debuggability.
4. Wrap the result in `MarkupContent { kind: Markdown, value: ... }`.

The two paths can both fire (user shadowed a stdlib name) or neither
(garbage token), in which case the fallback "literal token text in a
code fence" still produces something for the client to draw.

### Stdlib examples-index lookup

`stdlib_hover_for_token` walks the CST around the cursor token and
tries three increasingly-loose lookups, in this order:

1. **PATH-form** — if the token is inside a `PATH` / `PATH_EXPR`
   ancestor that contains at least one `.`, join its IDENT children and
   look up the joined string (`Member.anthropic`, `std.http.get`,
   `Compare.tool_call_set_equal`).
2. **Method-call form** — if the token is inside a `METHOD_CALL_EXPR`,
   pull the first IDENT out of the receiver subtree and try
   `<receiver>.<token>` (`Member.ask` reached as `Member` ← receiver of
   `Member.anthropic("x").ask(...)`). When the receiver chain starts
   with a lower-case binding whose type we cannot statically infer,
   this step falls back to bare-method lookup, which is still useful
   for the common cases.
3. **Bare-name** — last-segment match on `STDLIB_EXAMPLES`. This is
   how `log`, `swarm`, and other un-prefixed stdlib builtins resolve.

The first lookup that hits wins. The rendered Markdown is the one
section of the hover output that carries the description / capability /
example / see-also content.

### Markdown layout

`render_stdlib_entry` emits the sections in this order, skipping empty
ones:

```
```mty
<signature>
```

<description>

**Required capability:** `<cap>`

**Example:**

```mty
<example>
```

**See also:** `<sym1>`, `<sym2>`, ...
```

The "See also" list is curated-first, then back-filled by
`infer_see_also` (same struct/agent family → same `std.<module>` prefix
→ same capability), capped at five entries total.

## Tests

- `crates/mty-doc/src/examples.rs` carries a `tests` module asserting
  that the table has at least 50 entries (v0.33 T6 floor) and at least
  140 entries (v0.34 T3 floor), that every symbol is unique, that
  every entry has a signature and example body, that lookup hits for
  both qualified and bare forms, that the content hash is
  deterministic, that the rendered Markdown contains every expected
  section header, and that the v0.34 T3 module-coverage probe lights
  up entries for `std.rag`, `std.computer`, `std.swarm`,
  `std.observe`, `std.taint`, `std.eval`, `std.web`, `std.fs`,
  `std.json`, `std.string`, and `std.vec`.
- `crates/mty-lsp/tests/integration.rs` exercises the full hover path
  end-to-end on:
  - `log` (bare builtin, capability-less),
  - `Member.ask` (method-call with receiver-type bias),
  - `Member.anthropic` (path-form ctor),
  - `swarm` (bare builtin with rich see-also).

Run them with:

```sh
cargo test -p mty-doc --lib examples
cargo test -p mty-lsp --test integration hover
```

The integration tests are CST-level — they don't depend on the source
type-checking — so they remain useful even when the stdlib's actual
type signatures evolve.

## v0.34 follow-ups

- **Strategy B migration.** Once `mty-pkg` resolves a bundled `std.*`
  package, regenerate the examples index from the package's `///`
  doc-comments and drop the compile-time table.
- **Capability lints.** When the user's `@cap` set excludes the
  required capability declared in the hover, surface a `mty check`
  warning at the call site.
- **VS Code / JetBrains screenshot harness.** The current tests assert
  the Markdown payload; we still need a tiny smoke runner that drives
  a real client through `tower-lsp` and captures the rendered hover.
