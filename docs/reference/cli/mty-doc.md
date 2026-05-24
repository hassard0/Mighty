# mty doc

Render package documentation extracted from `///` doc comments.

## Synopsis

```
mty doc PATH [ITEM] [OPTIONS]
```

## Arguments

| Name   | Description                                                                   |
| ------ | ----------------------------------------------------------------------------- |
| `PATH` | The `.sd` source file to document.                                            |
| `ITEM` | Optional. Print one item's full doc instead of the package summary (stdout).  |

## Options

| Flag                | Purpose                                                                            |
| ------------------- | ---------------------------------------------------------------------------------- |
| `--html`            | Render an HTML site (per-item pages, embedded stylesheet, search index).           |
| `--markdown`        | Render a markdown tree (one file per item, plus `index.md`).                       |
| `--out DIR`         | Output directory for `--html` / `--markdown`. Defaults to `target/doc/<package>` (HTML) or `target/doc-md/<package>` (markdown). |
| `--check-examples`  | Type-check extracted ` ```sd ` and ` ```mighty ` code blocks. (No-op in v0.2 — see `DOC_V0_2_NOTES.md`.) |
| `-h`, `--help`      | Print help.                                                                        |

## Default behaviour (stdout summary)

With no flags and no `ITEM`, prints a Go-style summary of all **public**
items in the package to stdout. Example:

```text
package mathx

    Math helpers — addition, subtraction, and rich documentation
    samples for `mty doc`.

FUNCTIONS

pub fn add(a: I32, b: I32) -> I32
        Add returns the sum of `a` and `b`.

pub fn triple(n: I32) -> I32
        Triple returns `n * 3`.

TYPES

pub struct Pair {
        Pair holds two unsigned ints.
```

## Single item (`mty doc PATH ITEM`)

Prints the full doc body of one named item, including examples,
since-version, and back-links:

```text
package mathx

pub fn add(a: I32, b: I32) -> I32

    Add returns the sum of `a` and `b`.
    
    This is the long-form body. It can include code:
    
    ```sd
    let result = add(2, 3)
    ```

    Since: 0.2.0

    Used by:
        triple
```

Match is exact on the item's name. Match fails (and the command exits
non-zero) if no public or private item with that name exists.

## HTML output (`--html`)

```bash
mty doc --html examples/19_backend_service.sd --out target/doc/search_api
```

Produces:

```
target/doc/search_api/
├── index.html
├── style.css
├── search.js
├── search-index.json
├── agent.Api.html
├── agent.Searcher.html
├── fn.main.html
└── protocol.Search.html
```

`index.html` lists items by section with a live search field. Per-item
pages include a linkified signature, the rendered doc body, extracted
examples, and a "Used by" section.

## Markdown output (`--markdown`)

```bash
mty doc --markdown crates/foo/src/lib.sd --out docs/api
```

Produces a `BTreeMap`-ordered tree of `index.md` plus `<anchor>.md` per
item, suitable for hosting directly on GitHub or Hugo.

## Doc-comment conventions

- `///` on a fn / struct / enum / type alias / trait / agent / protocol
  / supervisor attaches to the item below. A blank line between the
  comment block and the item **detaches** the comment.
- `//!` at the top of the file becomes the package-level doc.
- Markdown is parsed via `pulldown-cmark` (CommonMark + tables +
  strikethrough + tasklists).
- ` ```sd ` and ` ```mighty ` fenced code blocks are harvested as
  examples.
- A `# Since` heading marks a version. Both `# Since 0.2.0` and a
  next-line `# Since\n0.2.0` are accepted.
- `[Name]` inside doc text resolves to a hyperlink to the named item
  when that name matches a documented item in the package.

## Exit codes

| Code | Meaning                                                              |
| ---- | -------------------------------------------------------------------- |
| `0`  | success                                                              |
| `1`  | I/O error, parse error, or `ITEM` not found                          |

## See also

- `docs/internals/doc-generator.md` — algorithm and IR shape.
- `DOC_V0_2_NOTES.md` — v0.2 interpretation calls (no example checking,
  single-file packages, no manifest version, simplistic backlinks).
