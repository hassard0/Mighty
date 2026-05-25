# mty-doc — v0.2 interpretation calls

This document records the deliberate scope cuts and "good enough" choices
made while building `crates/mty-doc` for the v0.2 milestone. Each item
is a follow-up candidate for v0.3.

## 1. Single-file packages only

`mty doc PATH` operates on a single `.sd` file. There is no
cross-file module resolution — `mod foo` items are recorded in
`DocPackage::modules` but the renderer does not yet walk into the
linked file.

> v0.3: take a package root (directory with a `Mighty.toml`) and a
> recursive file walk, building one `DocPackage` per package and
> emitting cross-package links via `package.module.Item` syntax.

## 2. No manifest version

`DocPackage::version` is hard-coded to `"0.0.0"`. The driver-resident
`Manifest` loader is intentionally not pulled in (`mty-doc` deps
were pared to avoid the `mty-driver` → `mty-codegen-cranelift`
chain, which was in-flight when this crate landed).

> v0.3: thread the package version from `Mighty.toml` via a new
> `build_doc_package_with_version` helper or by re-introducing the
> driver dep once codegen stabilises.

## 3. `--check-examples` is a no-op

The flag is parsed and acknowledged with a stderr warning. v0.2
extracts examples into `DocExample` but does not feed them through
`mty check`. Example bodies are surfaced verbatim in markdown and
HTML output.

> v0.3: wrap each example body in a synthetic `fn main()` (or accept
> top-level items per the user's intent), invoke
> `sdust_types::check_package`, and surface example-level diagnostics
> on the item page.

## 4. Simplistic back-link computation

`compute_backlinks` walks `HirExpr::Path` nodes and credits any
function whose Path's final segment matches a documented item's name.
Caveats:

- Self-references are not filtered (a fn calling itself shows up in
  its own `Used by` list).
- Shadowed locals can shadow item names, producing false positives.
- Method calls credit the receiver path, not the called method.
- Cross-package callers are not tracked (this is single-file only).

> v0.3: drive back-links from the typed package (`sdust_types::check_package_typed`)
> so callers are name-resolved rather than syntactic.

## 5. Agents are unconditionally "public"

The HIR doesn't track a `pub` flag on `HirAgent` (agents are public
by spec convention in v0.2 — see `crates/mty-hir/src/nodes.rs`). The
generator marks every agent as `DocVisibility::Public`.

> v0.3: align with the typed visibility surface once agents grow an
> explicit visibility modifier.

## 6. Signatures are pretty-printed from HIR

Type rendering uses a small local pretty-printer in
`crates/mty-doc/src/extract.rs::render_type`. It does not consult
`sdust_fmt::printer` — partly because `mty-fmt`'s type printer
returns a `Doc` over CST nodes, not over `HirType`, and partly to keep
the doc generator independent from formatting policy.

> v0.3: extract a small "render HirType to String" helper in
> `mty-types` or `mty-fmt` and share it.

## 7. HTML uses an inline template, not Askama

Askama 0.12 is declared as a workspace dep (and as a `mty-doc` dep)
so it's available, but the v0.2 HTML renderer uses inline `format!`s.
Reason: keep the per-page template trivially auditable and avoid
shipping a multi-template layout that we'll want to redesign in v0.3.

> v0.3: move to one `templates/page.html` driven by Askama, with
> per-kind partials for `Fn`, `Struct`, `Enum`, `Agent`, etc.

## 8. Search index is hand-rolled JSON

`render::search_index` emits `[{ ... }, { ... }]` directly. No
`serde_json` dep was added. This keeps the dep tree small at the cost
of fragile escaping.

> v0.3: pull in `serde_json` if the index ever needs richer fields
> (e.g. nested paths, parameter signatures, type tags).

## 9. No `--check` mode

There is no `mty doc --check` to verify a documented API hasn't
drifted from the source. The CLI surface is render-only in v0.2.

> v0.3: add a `--check` mode that compares an existing rendered tree
> against the generated tree and fails on diff, for CI use.
