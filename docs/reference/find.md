# mty find

Capability-tagged search across the Mighty stdlib. Shipped in v0.33
(Track T7).

`mty find` answers the question *"which stdlib API should I use?"* —
written for both humans (table output) and agents (NDJSON over the
`--format json` flag). The index walks `crates/mty-stdlib/src/**/*.rs`,
extracts every `pub` item with its surrounding `///` doc comment, and
heuristically tags each one with a Mighty capability (`fs.read`,
`net.https`, `mcp`, etc.). Queries are matched against item names,
verbs extracted from the docs, and the capability tags themselves.

## Synopsis

```
mty find QUERY                    [--format pretty|json|short] [--explain] [--top N]
mty find --by-capability CAP      [--format pretty|json|short]
```

## Examples

| Query | Expected top hit | Why |
|---|---|---|
| `mty find "write files"` | `std.fs.write` | exact verb + capability match |
| `mty find "send http"` | `std.http.send` / `std.http.post` | doc + name token match |
| `mty find "ask llm"` | `std.swarm.member.ask` / `std.llm.anthropic.AnthropicClient.complete` | verb + module-path match |
| `mty find "vector store"` | `std.memory.vector.VectorStore` | exact split-identifier match |
| `mty find --by-capability fs.write` | every fs-mutating item | inverse query |

## Output formats

### Pretty (default)

A table with `(ITEM, MODULE, KIND, score)` plus a summary line per hit.
Pass `--explain` to append the capability tag, first example snippet,
and source pointer.

### `--format json`

NDJSON — one [`Item`](#item-shape) per line. This is the format demo
07's research agent uses to discover APIs without a human in the loop:

```bash
mty find --format json "vector store" \
  | jq -r '.module + "." + .name + " (" + .capability + ")"'
```

### `--format short`

One-line summary suitable for terminal completion:

```
std.fs.write (fn) [fs.write]
std.fs.write_file (fn) [fs.write]
std.fs.current_default_write_cap (fn) [fs]
```

## Item shape

Each indexed item carries:

| Field | Meaning |
|---|---|
| `name` | The item's identifier (`write`, `VectorStore`, `LlmError`). |
| `module` | Mighty-style module path (`std.fs`, `std.memory.vector`). For methods inside an `impl` block, the impl target is included (`std.swarm.member.Member.ask`). |
| `kind` | `fn` / `struct` / `enum` / `trait` / `const` / `type`. |
| `signature` | One-line synthesized signature. |
| `capability` | Best-effort capability tag (may be empty when unguessable). |
| `verbs` | Tokens extracted from the name + the first ~60 doc tokens. |
| `summary` | First paragraph of the doc comment (≤240 chars). |
| `example` | First fenced code block in the doc comment, if any. |
| `source` | `<file>:<line>` pointer (relative to `crates/mty-stdlib/src/`). |
| `score` | Search score; only present after running a query. |

## Capability inference

`mty find` doesn't read `effect {...}` annotations directly (the
stdlib is Rust, not Mighty); instead it heuristically maps each item
to a capability based on module path + name + doc tokens:

| Module path | Default capability |
|---|---|
| `std.fs.read*`, `std.fs.open`, `std.fs.list_dir`, `std.fs.exists` | `fs.read` |
| `std.fs.write*`, `std.fs.atomic_write`, `std.fs.remove`, `std.fs.rename` | `fs.write` |
| `std.http*`, `std.tls`, `std.web` | `net.https` (or `net.bind` for `serve`/`bind`) |
| `std.llm.*`, `std.swarm` | `net.https + model` |
| `std.mcp.*` | `mcp` |
| `std.memory.*` | `fs.read + fs.write` (or `net.https` for qdrant backends) |
| `std.computer.*` | `computer` |
| `std.env` / `std.time` / `std.random` / `std.observe` | `env` / `time` / `random` / `observe` |
| `std.test`, `std.eval` | `test` |

A doc-comment fallback recognises the `@tool(cap: "<cap>")` decorator
syntax and prefers it over the heuristic.

## Ranking spec

Per matched token, scores accumulate as:

| Match | Score |
|---|---|
| Exact name match | +1.0 |
| Verb match (token == split-name word) | +0.9 |
| Verb match (token in doc-extracted verbs) | +0.7 |
| Substring match in name | +0.5 |
| Capability match (token is a known capability) | +0.6 |
| Module-path segment match | +0.4 |
| Verbatim case-sensitive name match | +0.2 (boost) |

Items with score 0 are dropped. Ties are broken by `(module, name)`
for deterministic output. `--top N` (default `5`) trims the result
set. Verbs are extracted by:

1. Splitting the name on `_` and on PascalCase boundaries (so
   `atomic_write` → `{atomic, write}` and `VectorStore` → `{vector,
   store}`).
2. Taking the first ~60 alphanumeric tokens in the doc comment,
   lower-casing them, and dropping a small stop-word list.

## `--by-capability`

`mty find --by-capability fs.write` is the inverse query — given a
capability, list every item that requires it. Useful when an agent
already knows what permission its manifest grants and wants to know
which APIs it just unlocked.

The match is conservative: exact, comma-separated-list-aware, or
prefix-aware (e.g. `--by-capability fs` returns both `fs.read` and
`fs.write` items).

## Index cache

The index lives at `~/.mty/find-index.json`. It's rebuilt
automatically when the stdlib source tree changes (we hash sorted
file `(path, size, mtime)` tuples — cheap, not cryptographic). Pass
`--rebuild` to force a refresh.

The on-disk format is:

```json
{
  "version": 1,
  "stdlib_hash": "fnv1a64:1234abcd5678ef00",
  "items": [
    { "name": "write", "module": "std.fs", "kind": "fn", "signature": "...",
      "capability": "fs.write", "verbs": ["bytes","write","..."],
      "summary": "Write bytes to a file (creates or truncates).",
      "example": "std.fs.write(cap, \"out.txt\", b\"hello\")?;",
      "source": "fs.rs:201" }
  ]
}
```

## Agent quickstart

```bash
# Find the right surface
RESULT=$(mty find --format json --top 1 "vector store" | head -1)

# Read off the module path and capability
echo "$RESULT" | jq -r '.module + "/" + .name'   # std.memory.vector/VectorStore
echo "$RESULT" | jq -r '.capability'             # fs.read + fs.write
```

See `demos/07_research_agent/README.md` for the wired-up version.

## See also

- `mty doc PATH` — render full per-item documentation
- `mty inspect --cost` — observe live agent calls
- `docs/reference/stdlib/` — the actual reference manuals
