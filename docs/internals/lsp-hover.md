# LSP hover (v0.33 T6, v0.34 T3, v0.35 T5, v0.38 T4 expansions)

The LSP hover provider returns a Markdown payload that drives the box VS
Code / JetBrains / Neovim show when the user rests on an identifier. As
of v0.38 T4 it stitches together signatures from the type-checker with a
curated **stdlib examples index** of 300+ seeded entries so the hover
for `Member.ask` (or any other seeded stdlib surface) renders:

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
the foundational `std.string` + `std.vec` method surface.

v0.35 T5 split the catalog into per-module docstub files
(`crates/mty-stdlib/docs/<module>.docstub`) that are the runtime
source-of-truth; the curated `STDLIB_EXAMPLES` is the gold-set the
`mty doc --check` drift gate compares against (Strategy B).

v0.38 T4 grew the catalog to **300+ entries** and added ten new modules
covering v0.37's new surfaces and previously-uncovered stdlib helpers:

- **`extern`** — FFI surfaces: `extern c { ... }` blocks, `extern c fn`
  shorthand, `extern c fn ... (..., ...)` variadics (v0.37 T6),
  `[[extern_lib]]` manifest entries, the v0.37 T3 call-site coercions
  (`Str → *U8`, `&local → *const T`, `&mut local → *mut T`), and the
  v0.38 T3 by-value struct return surface.
- **`cast`** — `expr as Ty` and the MT2027 INVALID_CAST emit-site:
  every recognised scalar conversion pair (U8↔I64, F32↔F64, I32↔F32,
  USize↔U64, Bool→U8, Char→U32, pointer↔USize) plus the invalid-cast
  diagnostic.
- **`env`** — every runtime / build env var renamed at v0.36
  (`MTY_LINKER`, `MTY_OTLP_ENDPOINT`, `MTY_TRACE`,
  `MTY_RUNTIME_THREADS`, `MTY_RUNTIME_CONTROL_SOCK`) with their legacy
  `STARDUST_*` deprecation notes.
- **`process`** — `Command` builder + `std.process.{spawn,exec,wait,
  kill}` + `ProcessExit`.
- **`io`** — `std.io.{stdin,stdout,stderr}` handles, `BufReader` /
  `BufWriter` wrappers, `read_line` / `write_line`, `eprintln!`.
- **`path`** — `Path` borrow + `PathBuf` owned variants, `components`,
  `parent`, `file_name`, `extension`, `join`, `is_absolute`,
  `with_extension`, `exists`.
- **`collections`** — `HashMap`/`HashSet` + `BTreeMap`/`BTreeSet`
  constructors and core ops.
- **`iter`** — every workhorse combinator: `map`, `filter`, `fold`,
  `collect`, `zip`, `chain`, `take`, `skip`, `enumerate`, `sum`,
  `count`, `any`, `all`, `find`.
- **`result`** — `Result.{ok,err,is_ok,map_err,unwrap_or,and_then}`.
- **`option`** — `Option.{is_some,is_none,map,and_then,unwrap_or,ok_or}`.
- **`error`** — `Error` trait + `anyhow!` macro ergonomics.

The table's per-entry shape is unchanged across all five expansions:

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
parsing, no async I/O on the hover path. v0.35 T5 migrated to Strategy B
(per-module `.docstub` files at `crates/mty-stdlib/docs/<module>.docstub`)
which is now the runtime source-of-truth; the curated table is the
gold-set the `mty doc --check` drift gate compares against. Coverage is
locked in by snapshot-style tests (uniqueness, presence of signature +
example body, hash determinism, per-version module-coverage floors at
v0.34 T3 and v0.38 T4 — see `examples::tests`).

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
  that the table has at least 50 entries (v0.33 T6 floor), at least
  140 entries (v0.34 T3 floor), at least 300 entries (v0.38 T4 floor),
  at least 400 entries (v0.39 T5 floor), and at least 500 entries
  (v0.40 T5 floor); that every symbol is unique; that every entry has
  a signature and example body; that lookup hits for both qualified
  and bare forms; that the content hash is deterministic; that the
  rendered Markdown contains every expected section header; and that
  the per-version module-coverage probes light up entries for the
  v0.34 T3 surfaces (`std.rag`, `std.computer`, `std.swarm`,
  `std.observe`, `std.taint`, `std.eval`, `std.web`, `std.fs`,
  `std.json`, `std.string`, `std.vec`), the v0.38 T4 surfaces (FFI /
  cast / env vars / std.process / std.io / std.path / std.collections
  / std.iter / std.result / std.option / std.error), the v0.39 T5
  surfaces (`std.crypto`, `std.encoding`, `std.url`, `std.uuid` plus
  the v0.38-backlog gap-fillers), and the v0.40 T5 surfaces
  (`std.regex`, `std.crypto.aes_gcm`, `std.crypto.chacha20_poly1305`,
  `Char.from_u32`, plus the v0.39 gap-fillers: std.iter advanced
  combinators, std.collections BTreeMap/BTreeSet/HashSet polish,
  std.json / std.path / std.string / std.vec / std.option /
  std.result polish, std.swarm Member helpers, std.observe percentiles
  / aggregate_by, std.eval Replay glue, std.fs cap helpers).
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

## v0.40 follow-ups (after v0.39 T5)

- **Cast-polish hover entries.** v0.39 T2 ships `cast_int_to_bool`,
  `cast_int_to_char`, `cast_ref_to_ptr`, and `MT2028
  INVALID_CODEPOINT`. The T2 docstub patch lives on
  `origin/v039-track-cast-polish` and lands when that branch merges —
  the T5 catalog deliberately holds the pre-polish shape so the
  current main-merge stays drift-free.
- **Vec typed-slot v2 header entries.** v0.39 T3 introduces
  `VEC_HEADER_V2` and per-element-size headers. Same pattern as cast
  polish — entries arrive with the T3 merge.
- **Cipher / AEAD.** `std.crypto` ships hash + HMAC + CSPRNG in v0.39
  T1 but not AES-GCM / ChaCha20-Poly1305. Add entries when the cipher
  surface lands.
- **regex.** `std.regex` is not yet shipped in T1; hover entries for
  `Regex.new`, `Regex.find`, `Regex.captures` follow the surface.
