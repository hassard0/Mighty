# demo 13 — RAG with regex

v0.40 T4 forcing-function demo: regex-driven RAG pre-processing
layered on top of the v0.33 `std.rag` pipeline.

## What it shows

- **`std.regex.Regex.new`** — compile-time pattern validation.
  Errors surface at `Regex.new` so a bad pattern fails fast, not at
  the first match.
- **`.find_all`** — tokeniser for the chunker's token-budget oracle.
- **`.captures_all`** — heading extraction + markdown-link extractor.
- **`.is_match`** — cheap date-window pre-filter that runs before
  the embedder.
- **`.replace_all`** — HTML tag stripper for documents with embedded
  markup.
- **`std.rag.Index` + `std.rag.Rag`** — the same one-liner pipeline
  demo 10 uses, now with regex pre-processing on every chunk.

## Build

```
cargo build -p mty-cli
```

## Smoke (no LLM call)

```
bash demos/13_rag_regex/smoke.sh
```

Asserts:
- `mty check` + `mty fmt --check` pass.
- `mty run` exercises the full pipeline and prints every expected
  event marker (`evt:rag:ask` ... `evt:rag:answered`) followed by
  the final `rag_regex: pipeline OK ...` summary.
- Every v0.40 T4 regex surface marker (`std.regex.Regex.new(`,
  `.find_all(`, `.captures_all(`, `.is_match(`, `.replace_all(`)
  appears in the demo body.
- Every v0.33 RAG surface marker (`Index.new(`, `Rag.new(`,
  `Member.anthropic(`, ...) appears too — demo 13 *extends* demo
  10's RAG pipeline; it does not replace it.
- Bundled corpus is present (≥3 markdown files) and has at least
  one document with a 2026 date (so the filter has something to
  accept) and at least one without (so the filter has something
  to reject).

## Run (real LLM)

```
ANTHROPIC_API_KEY=sk-ant-... mty run \
    demos/13_rag_regex/src/main.mty
```

The demo will:
1. Stage `corpus/intro.md`, `corpus/spec.md`, `corpus/changelog.md`,
   `corpus/old_notes.md` into an in-memory RAG index.
2. Run the regex pre-processing pass over each chunk (token count,
   heading extraction, date check, link extraction).
3. Filter out documents whose body has no recent (2026-*) date.
4. Dispatch `Rag.ask` to retrieve top-3 hits, build the augmented
   prompt, and ask the answering Member.

## When to choose regex vs language-model tokenizer

| Question | Use regex | Use LLM tokenizer |
|---|---|---|
| How many tokens are in this body? | Cheap upper bound | Exact (matches embedder) |
| Does this body have an ISO date? | Yes — microseconds | Overkill |
| What entities live in this body? | Hard — too brittle | Yes — embeddings shine here |
| Filter docs by year before retrieval | Yes | No — too slow |
| Split chunks at semantic boundaries | No — text-only | Yes — uses embedding similarity |

Rule of thumb: **regex for structure (boundaries, IDs, dates, link
targets); embeddings for meaning.** Use regex as the cheap pre- and
post-filter around an expensive embed-and-rerank core.

## RAG architecture

```
        corpus/*.md
            v
       regex pre-pass:
         * token count       (Regex.new("\\b\\w+\\b").find_all)
         * heading extraction (captures_all)
         * date extraction    (find_all)
         * link extraction    (captures_all)
            v
       date-window filter (is_match) ─── drop pre-2026 docs
            v
       Index.add_text(chunk, {source, year})
            v
       index.build()
            v
       Rag.new()
         .with_index(index)
         .with_retriever_top_k(3)
         .with_member(Member.anthropic("claude-opus-4-7"))
            v
       Rag.ask(question) ─────► answering Member ─► answer body
```

## Files

- `src/main.mty` — the regex-augmented RAG agent + main entry.
- `corpus/intro.md` — regex-RAG overview document (has 2026 date).
- `corpus/spec.md` — `std.regex` surface document (has 2026 date).
- `corpus/changelog.md` — release changelog (has 2026 dates).
- `corpus/old_notes.md` — pre-2026 notes (kept so the date filter
  has something to reject).
- `mighty.toml` — package manifest.
- `smoke.sh` — surface + corpus + runtime marker validation.

## See also

- `demos/10_vision_rag/` — the v0.33 RAG demo this one extends.
- `examples/43_secure_session.mty` — the v0.40 T4 canonical
  example (`std.regex` + AEAD).
- `docs/internals/std-regex.md` — `std.regex` design notes.
- `crates/mty-stdlib/src/regex/` — Rust implementation.
