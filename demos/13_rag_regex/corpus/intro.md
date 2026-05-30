# Mighty regex-augmented RAG

Updated: 2026-04-12

Demo 13 layers the v0.40 T4 `std.regex` surface on top of the v0.33
`std.rag` pipeline so a real corpus can be pre-processed before it
hits the embedder.

## Why regex on top of RAG

The embedder picks up *semantic* similarity but is blind to structure.
A typical RAG pipeline still wants:

- Token-budget oracles that match the embedder's own tokenisation.
- Heading extraction for citation rendering.
- Date / ID / link filters that run in microseconds instead of
  embedding every doc.

See [the std-regex internals doc](docs/internals/std-regex.md) for
the full surface.

## Pre-processing recipe

1. `Regex.new("\\b\\w+\\b").find_all(body).len()` → token count.
2. `Regex.new("(?m)^#\\s+(.+)$").captures_all(body)` → headings.
3. `Regex.new("\\d{4}-\\d{2}-\\d{2}").find_all(body)` → ISO dates.
4. `Regex.new("\\[([^\\]]+)\\]\\(([^)]+)\\)").captures_all(body)` →
   markdown link targets.
