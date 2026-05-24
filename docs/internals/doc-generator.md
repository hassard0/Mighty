# Doc generator (sdust-doc)

`sdust-doc` is the Stardust documentation generator. It walks a parsed
`.sd` source file, harvests `///` and `//!` comments, and renders the
result in three flavours:

1. **Go-style stdout** — `sdust doc` (whole package) or `sdust doc Item`
   (one item with full body). The format intentionally echoes Go's
   `go doc` tool: section headers in caps (FUNCTIONS, TYPES, …), a
   signature on its own line, and the synopsis indented underneath.
2. **Markdown** — `sdust doc --markdown`. One file per item plus an
   index, suitable for hosting on GitHub.
3. **HTML** — `sdust doc --html`. Per-item pages, an index, an embedded
   stylesheet, and a search index served as a static JSON file with a
   ~50-line client-side fuzzy matcher.

This page describes the internals.

## Pipeline

```
source.sd
  │
  ▼
sdust_syntax::parse          (rowan green tree + parse errors)
  │
  ▼
sdust_hir::lower             (HIR + lowering diagnostics)
  │
  ▼
sdust_doc::extract           (DocPackage IR)
  │      walks top-level item nodes,
  │      harvests adjacent /// trivia tokens,
  │      renders one signature per item from HIR,
  │      runs CommonMark over each body
  ▼
sdust_doc::render            (text | markdown | html)
```

The generator deliberately stays on the **HIR** rather than typed
output. That's fine for v0.2 because signatures, doc strings, and
back-link names are all syntactic. v0.3 will optionally feed the
typed package in and surface inferred types.

## Doc-comment harvesting

Two source-level conventions are recognised:

| Marker | Attaches to                       | Form                                         |
| ------ | --------------------------------- | -------------------------------------------- |
| `///`  | The immediately-following item    | `DOC_COMMENT` token (per the syntax kind)    |
| `//!`  | The enclosing module / package    | `LINE_COMMENT` token whose text starts `//!` |

### Trivia attachment

The parser greedily absorbs trailing whitespace + comment trivia into
the previous CST node, so the `///` tokens that visually precede an
item often live *inside* the previous sibling's deepest leaf node
(usually its `PATH`). Walking `prev_sibling_or_token()` would miss
them.

Instead, `collect_doc_comments` walks **backwards through every leaf
token** (`item_node.first_token().prev_token()` …) regardless of node
nesting, collecting `WHITESPACE`, `DOC_COMMENT`, `LINE_COMMENT`, and
`BLOCK_COMMENT` tokens until it hits a non-trivia token. The accepted
trivia stream is then filtered by `extract_doc_run`, which keeps the
*last* maximal run of `///` tokens not separated from the item by a
blank line, a regular line/block comment, or any unexpected token.

This implements the rule: **a `///` block must be glued directly to its
item, with at most single-line separators**. A blank line detaches.
This matches both Go's and Rustdoc's behaviour.

### File-level `//!`

The first `//!` block in the file (before any item node) becomes the
package's `synopsis` and `body`. `extract_file_doc` walks
`File.children_with_tokens()` and consumes only the leading
`LINE_COMMENT` tokens whose text starts with `//!`, stopping at the
first node or unrelated token.

## IR

```
DocPackage { name, version, synopsis, body, modules, items }

DocItem {
    name,
    kind: DocItemKind,
    visibility,
    signature: { plain, html },
    synopsis,
    body,        // CommonMark source
    examples,
    since,
    used_by,
    anchor,      // "fn.add", "struct.Pair", …
}
```

Item kinds and their section headers:

| Kind         | Section      | Anchor prefix |
| ------------ | ------------ | ------------- |
| `Fn`         | FUNCTIONS    | `fn.`         |
| `Struct`     | TYPES        | `struct.`     |
| `Enum`       | TYPES        | `enum.`       |
| `TypeAlias`  | TYPES        | `type.`       |
| `Trait`      | TRAITS       | `trait.`      |
| `Agent`      | AGENTS       | `agent.`      |
| `Protocol`   | PROTOCOLS    | `protocol.`   |
| `Supervisor` | SUPERVISORS  | `supervisor.` |
| `Const`      | CONSTANTS    | `const.`      |

Anchor slugs are stable across renderings — they are what the
linkifier emits in `#anchor` fragments and what becomes the file name
for per-item markdown / HTML pages.

## Body parsing

`parse_doc_body` splits the raw doc-block text into:

- **Synopsis** — the first sentence. Stops at the first `.` followed
  by whitespace (or end of input), or at the first blank-line
  paragraph break, whichever comes first. Used in indexes and the
  Go-style listing.
- **Body** — the whole CommonMark source (synopsis included). The
  renderer is responsible for either re-extracting the synopsis or
  rendering the body verbatim.
- **Examples** — fenced code blocks tagged `sd` or `stardust` are
  extracted as `DocExample { code, language }`.
- **Since** — text following a `# Since` heading. Two forms accepted:
  same-line (`# Since 0.2.0`, `# Since: 0.2.0`) or next-non-empty-line
  (`# Since\n0.2.0`).

## Backlinks

For every public item, `compute_backlinks` populates `used_by` by
walking every fn body's `HirExpr` tree and every agent handler's
block, crediting any `Path` (or `PathGeneric`) whose **last segment**
matches a documented item name. Self-references are not stripped (a
function calling itself shows up as its own caller in v0.2 — flagged
in `DOC_V0_2_NOTES.md`).

## Linkification

Two levels of cross-reference:

1. **Doc-string `[Name]`** — `linkify_doc_text` scans for `[Ident]`
   sequences and rewrites them to markdown `[Name](#anchor)` when
   `Name` matches a documented item. Whitespace-padded brackets and
   non-ident contents are left alone.
2. **HTML signature hyperlinks** — `linkify_signature` performs
   whole-word substitution of every documented item's name with an
   `<a href="#anchor">Name</a>` wrapper. The substitution list is
   sorted longest-first so that overlapping names (`Foo`, `FooBar`)
   replace correctly. The plain (text) signature is left alone.

## Renderers

### Text (Go-style)

`render::text` produces a single string. It groups items by section
header (stable order: FUNCTIONS, TYPES, TRAITS, AGENTS, PROTOCOLS,
SUPERVISORS, CONSTANTS), prints each signature on its own line, and
indents the synopsis underneath. Private items are skipped.

`render::item_text` produces a single-item dump with the full body,
since-version, and back-links.

### Markdown

`render::markdown` returns a `BTreeMap<String, String>` of relative
path → file contents:

```
index.md
fn.add.md
fn.triple.md
struct.Pair.md
…
```

Each item page has the signature, since block, linkified body,
extracted examples, used-by list, and a back-to-index link.

### HTML

`render::html` returns the same map shape plus three static assets:

- `style.css` — embedded stylesheet with CSS-variable theming and a
  `prefers-color-scheme` dark mode toggle.
- `search.js` — ~50-line vanilla-JS fuzzy filter over the search
  index.
- `search-index.json` — hand-encoded JSON array of `{ name, kind, url,
  synopsis }`.

The HTML pages use a single inline template (no Askama yet — Askama is
declared as a dep so v0.3 can swap in proper templates without
churning the manifest).

`render::write_tree(dir, files)` writes the map to disk, creating the
output directory if needed.

## CLI surface

See `docs/reference/cli/sdust-doc.md` for the user-facing reference.

## Tests

- `tests/extract.rs` — doc-comment extraction edge cases (blank-line
  detach, undocumented items, since-version forms, backlinks).
- `tests/render_markdown.rs` — markdown index + per-item shape.
- `tests/render_html.rs` — HTML linkified signatures, since blocks,
  static asset emission.
- `tests/cli.rs` — end-to-end shape (matches what
  `crates/sdust-cli/src/cmd/doc.rs` calls).
