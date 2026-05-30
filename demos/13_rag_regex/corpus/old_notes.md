# Older notes (pre-2026)

Released: 2025-11-09

These notes describe the v0.33 RAG pipeline *before* the regex
surface landed. Kept here so demo 13's date-filter recipe has at
least one document that should be **rejected** by the
`_has_recent_date` filter.

## v0.33 vanilla RAG

`Rag.new().with_index(idx).with_retriever_top_k(3).with_member(member)`
runs end-to-end with paragraph chunking only. Token budgets are
estimated by the embedder; no regex pre-pass.

## What changed in 2026

See the rest of the corpus — the v0.40 T4 regex surface lifted this
pipeline into a regex-augmented form.
