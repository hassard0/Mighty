# demo 10 — Vision RAG

v0.33 Track T2 forcing-function demo: `std.rag` + multi-modal image
input in one program.

## What it shows

- **`std.rag.Index`** — staged corpus build with the paragraph
  chunker.
- **`std.rag.Rag`** — end-to-end pipeline behind one chained
  constructor.
- **`std.llm.Image`** — disk-loaded PNG/JPG/GIF/WebP base64-encoded
  for the provider's wire shape.
- **`Rag.ask_with_image`** — augmented prompt + image content block
  in a single round-trip.
- **`@tool("...", cap: fs.read)`** — v0.27 Track A macro still works
  alongside the v0.33 surface.

## Build

```
cargo build -p mty-cli
```

## Smoke (no LLM call)

```
bash demos/10_vision_rag/smoke.sh
```

Asserts:
- `mty check` and `mty fmt --check` pass.
- The bundled corpus + sample diagram are present.
- Every v0.33 surface marker (`Index.new`, `Rag.new`,
  `Image.from_file`, `ask_with_image`, …) appears in the demo body.

## Run (real LLM)

```
ANTHROPIC_API_KEY=sk-ant-... mty run \
    demos/10_vision_rag/src/main.mty -- \
    ./demos/10_vision_rag/tools/sample_diagrams/architecture.png \
    "Summarise this architecture diagram."
```

The demo will:
1. Spawn the `VisionResearcher` agent.
2. Build an in-memory RAG index over the bundled corpus.
3. Load the diagram with `Image.from_file`.
4. Dispatch `Rag.ask_with_image` — retrieves top-3 corpus hits,
   augments the prompt, sends the image + prompt to Claude Opus 4.7,
   returns the answer body.

## Files

- `src/main.mty` — the agent + main entry point.
- `tools/sample_corpus/{intro,spec}.md` — the RAG corpus.
- `tools/sample_diagrams/architecture.png` — the input image.
- `mighty.toml` — package manifest.
- `smoke.sh` — surface-marker validation.

## See also

- `docs/tour/21-rag-and-vision.md` — tour chapter.
- `docs/internals/rag.md` — design notes for the `std.rag` module.
- `crates/mty-stdlib/src/rag/` — Rust implementation.
- `crates/mty-stdlib/src/llm/image.rs` — multi-modal `Image` type.
