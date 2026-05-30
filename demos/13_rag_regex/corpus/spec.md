# std.regex — RE2-style finite automata

Released: 2026-05-01

`std.regex` wraps the Rust `regex` crate. The wire shape:

- `Regex.new(pattern)` → compiled pattern.
- `r.find(hay)` → first match.
- `r.find_all(hay)` → every non-overlapping match.
- `r.captures(hay)` → first match + capture groups.
- `r.captures_all(hay)` → every match + capture groups.
- `r.replace(hay, rep)` → first replacement.
- `r.replace_all(hay, rep)` → every replacement.
- `r.is_match(hay)` → cheap predicate.
- `r.split(hay)` → split on every match.

## Guarantees

- **Linear time.** No look-around; RE2-style automata only.
- **Unicode-aware `\w` `\d` `\s`** by default.
- **Compile errors** at `Regex.new` not at first match — fail fast.

## When to use regex in a RAG pipeline

Regex shines on **structure** — boundaries, IDs, dates, link
targets. The embedder shines on **meaning**. Combine them: pre-
filter with regex, retrieve with embeddings, rerank with an LLM.

See [the v0.40 release notes](docs/release-notes/v0.40.md).
