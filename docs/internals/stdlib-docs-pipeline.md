# stdlib hover-catalog pipeline (Strategy B)

This is the v0.35 T5 design doc for how Mighty's LSP hover, `mty doc`,
and `mty doc check` get their stdlib usage examples. It supersedes the
v0.33 T6 "Strategy A" sketch which lived in
`crates/mty-doc/src/examples.rs` as a comment.

## TL;DR

```
crates/mty-stdlib/docs/<module>.docstub   ← source of truth (text)
                  │
                  │  include_str! at compile time
                  ▼
crates/mty-doc/src/stdlib_walker.rs        ← walker / catalog builder
                  │
        ┌─────────┼─────────┐
        ▼         ▼         ▼
   LSP hover   mty doc    mty doc --check
                          (drift gate)
```

The walker parses every docstub file into an
`ExtractedExample { symbol, signature, description, capability,
example, see_also, module }` record. The flat catalog is what every
downstream consumer reads.

A frozen snapshot of the catalog still lives in
`crates/mty-doc/src/examples.rs::STDLIB_EXAMPLES` (the v0.33 / v0.34
hand-curated gold-set) and powers the LSP hover today. `mty doc check`
guarantees the two stay byte-for-byte equal.

## Why two surfaces

The v0.33 T6 plan called for "Strategy B" — extract from the stdlib's
own source. v0.35 T5 ships that, but does NOT immediately retire the
curated table. Reasons:

- The LSP hover provider built on `STDLIB_EXAMPLES` is shipped, stable,
  and used by every IDE plugin since v0.33.
- The curated table is the only entry point that holds `&'static str`
  end-to-end (zero allocation on hot LSP paths). The walker produces
  `String` so the hover would either need to allocate on every hover
  or thread a `LazyLock<Vec<ExtractedExample>>` through the LSP.
- Keeping the curated table as the **frozen snapshot** lets us roll
  forward incrementally: any future docstub edit is checked against
  the snapshot via `mty doc check`. v0.36 (planned) flips the
  direction — the docstubs become primary and the curated table is
  regenerated from them.

So today:

| Surface              | Reads          | Notes                          |
|----------------------|----------------|--------------------------------|
| LSP hover            | `STDLIB_EXAMPLES` | `&'static str`, zero-alloc.  |
| `mty doc --check`    | both           | Compares extracted ↔ curated. |
| `mty doc <path>`     | extract.rs     | Unrelated — walks `.mty` source. |
| `regen-stdlib-docstubs` (bin) | `STDLIB_EXAMPLES` → docstubs | One-shot regen tool. |

## Docstub format

Per-module text files at `crates/mty-stdlib/docs/<module>.docstub`.
Line-oriented; one entry per `##sym ... ##end` block; blank lines and
lines starting with `# ` are comments.

```
##module llm
##generated_from STDLIB_EXAMPLES (run `cargo run -p mty-doc --bin regen-stdlib-docstubs` to re-emit)

##sym Member.ask
##sig fn Member.ask(&self, prompt: Str) -> Result<MemberReply, LlmError>
##cap net.https (for the provider endpoint)
##desc Sends prompt to the LLM provider and returns the reply.
##see Member.anthropic, Member.openai, std.swarm, swarm
##example
let m = Member.anthropic("claude-opus-4-7");
let r = m.ask("Capital of France?")?;
log(r.text);
##end
```

Directives:

- `##module <name>` — once per file; bucket name (matches the filename
  stem). Optional; the walker falls back to the filename.
- `##sym <qualified-name>` — opens an entry. Required first.
- `##sig <signature>` — required. One line.
- `##desc <prose>` — required. One line.
- `##cap <capability>` — optional. Omitted = empty.
- `##see <comma-list>` — required. Renders as the hover's "See also".
- `##example` / `##end` — required body delimiters. Everything between
  them is the example body, verbatim. Trailing newline is normalised
  (the walker re-appends exactly one).

Directive order inside an entry is free except `##example` must be
last (it consumes lines until `##end`).

## The walker

`crates/mty-doc/src/stdlib_walker.rs`. Two public entry points:

- `parse_docstub(src, default_module)` — parse one file's text body.
- `build_extracted_catalog()` — parse every embedded docstub
  (`EMBEDDED_DOCSTUBS`) and return the flat catalog in module-iteration
  order. Panics on a parse failure — the docstubs ship with the binary,
  so a malformed one is a build-time bug the CI gate catches.

The module list is hard-coded in `EMBEDDED_DOCSTUBS` so `include_str!`
can pin every path at compile time. Adding a new stdlib surface is two
line edits: add the docstub file, add the tuple to `EMBEDDED_DOCSTUBS`.

## Drift detection

`diff_catalogs(extracted, curated)` returns a `Vec<Drift>` listing every
divergence in stable order:

- `MissingFromExtracted` — curated has the symbol; docstub does not.
- `ExtraInExtracted` — docstub has it; curated table does not.
- `FieldMismatch { field }` — both sides have the symbol but `field`
  (signature / description / capability / example / see_also) differs.

`render_drift_report(&drift)` formats the result. Empty drift returns
the empty string; callers print a success banner separately.

## `mty doc check`

Surfaces the drift gate as a one-shot CLI. Synopsis:

```bash
mty doc --check                            # report on stdout, exit 0/1
mty doc --check --report drift.md          # also write to drift.md
```

Exit codes:

- `0` — extracted ≡ curated, byte-for-byte. Report ends with
  `OK: extracted catalog matches curated table byte-for-byte.`
- `1` — drift detected. Report lists every divergence and ends with a
  pointer to the fix: edit the docstub or rerun
  `cargo run -p mty-doc --bin regen-stdlib-docstubs`.
- `2` — argument error (e.g. `--check` alongside an incompatible flag
  combo). Reserved for clap-side errors.

The CI gate runs `mty doc --check` on every push (Linux job in
`.github/workflows/ci.yml`). A non-zero exit blocks the merge.

## Editing workflow

Two flavours:

### A. The stdlib gained a public surface; the catalog needs a new entry.

1. Add the entry to `crates/mty-doc/src/examples.rs::STDLIB_EXAMPLES`.
2. Run `cargo run -p mty-doc --bin regen-stdlib-docstubs`.
3. `cargo run -p mty-cli -- doc --check` should print
   `OK: extracted catalog matches curated table byte-for-byte.`
4. Commit the curated edit + the regenerated docstub.

### B. An existing entry's signature / example / capability is stale.

Same as A — the regen step rewrites the docstub from the curated table.

### C. A docstub was edited by hand (hover-only fix).

1. `mty doc --check` will fail; the report names the diverging field.
2. Either:
   - Roll the curated table forward to match the docstub (preferred —
     the docstub is the new source of truth).
   - Or revert the docstub edit if the curated table is the intended
     value.

v0.36 will invert the convention so the docstubs become primary and
the curated table is regenerated, not edited.

## Lookup semantics

`lookup_extracted(catalog, name)` mirrors the curated `lookup`:

- Exact qualified match first (`Member.ask`).
- Exact bare match next (`log`).
- Last-segment bare-name fallback (`ask` → `Member.ask`).

The LSP hover provider only falls back to bare-name lookup once it has
ruled out a user-side definition; see `crates/mty-lsp/src/hover.rs`.

## Test coverage

`crates/mty-doc/src/stdlib_walker.rs::tests` enforces, on every CI run:

- The embedded docstubs all parse cleanly.
- `extracted.len() == curated.len()`.
- Every curated symbol resolves in the extracted catalog with the same
  signature.
- `diff_catalogs(extracted, curated)` is empty (the drift gate
  inverted — a unit test, not just the CI job).
- The mini-grammar parser rejects malformed input (missing `##end`,
  missing `##sig`, unexpected directive inside an entry).

Plus the broader integration tests in
`crates/mty-lsp/tests/integration.rs` exercise the hover surface end
to end on `Member.ask`, `Member.anthropic`, `log`, and `swarm`.

## Future direction (v0.36+)

- Flip the source-of-truth: regenerate `STDLIB_EXAMPLES` from the
  docstubs at build time (`build.rs` emits a `&'static`-friendly
  representation into `OUT_DIR`).
- Surface the `module` field in hover ("found in `std.llm`").
- Add `##since <version>` directives so hover can render "added in v0.35".
- Replace the per-module `EMBEDDED_DOCSTUBS` list with a `build.rs`
  walker so new files are picked up without a Rust-side list edit.

## See also

- `crates/mty-doc/src/examples.rs` — curated table + LSP-hover helpers.
- `crates/mty-doc/src/stdlib_walker.rs` — Strategy B walker + drift.
- `crates/mty-cli/src/cmd/doc.rs::run_check` — CLI front-end.
- `docs/internals/lsp-hover.md` — LSP hover provider design.
- `docs/internals/doc-generator.md` — the `.mty`-side doc extractor
  (`mty doc <path>` — separate pipeline).
