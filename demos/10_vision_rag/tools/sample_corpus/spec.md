# std.rag — RAG-as-stdlib

`std.rag` is the v0.33 Track T2 surface that promotes the v0.26
`std.memory.VectorStore` into a one-liner RAG pipeline.

## Index

`Index` wraps a `VectorStore` plus a `Chunker`. The staging-then-build
pattern lets callers batch I/O:

```
let mut idx = Index.new("./corpus")
idx.add_text("...", {source: "intro"})
idx.add_file("./docs/spec.md")
idx.build()
```

## Retriever

`Retriever` is a stateless policy over the index — top-k, score
threshold, optional MMR diversification.

## Reranker

`Reranker` is the optional LLM-as-reranker pass. Cheap embedding
retrieves 20-100 candidates; a smarter (slower) LLM picks the actual
top-k by scoring relevance on a 0-100 scale.

## Rag

`Rag` is the end-to-end pipeline. Chained constructor wires the index,
retriever knobs, optional reranker, and answering Member into a single
`ask(query)` or `ask_with_image(query, image)` call.
