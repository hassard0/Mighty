# v0.27 Track A — `@tool(...)` source-level attribute parser

## Summary

v0.26 Track B shipped the `@tool` macro REGISTRY plus the
`mty_macros::stdlib::tool::expand_tool_attribute` expander, but the
parser had no surface for `@tool(args...)` on user fns. v0.26 demo 07
had to ship its three tool fns as plain `fn` decls with the intended
`@tool("desc", cap: fs.read)` preserved only as a leading comment.

v0.27 Track A closes the gap end-to-end:

1. **Parser**: new `TOOL_ATTR` / `TOOL_ATTR_ARGS` / `TOOL_ATTR_CAP_ARG`
   CST nodes. The dispatch in `parser::items::item` recognises
   `@<ident>(args...)` immediately preceding a `fn` / `agent` /
   `protocol` decl.
2. **AST**: `mty_ast::ToolAttr` typed accessor — `ToolAttr::for_fn_decl`
   locates the attribute, `description_literal()` / `cap_expr_text()` /
   `named_args()` pull the validated parts.
3. **HIR**: `HirFn::tool_attr: Option<HirToolAttr>` carries the decoded
   description + cap text through to downstream stages.
4. **HIR preprocessor**: a new attribute-pass runs before declarative
   macro expansion. It calls
   `mty_macros::expand_builtin_attribute("tool", ...)` per fn and
   splices the `__tool_descriptor_<NAME>` / `__tool_invoke_<NAME>` /
   `__tool_register_<NAME>` companion fns into the source AFTER the
   user's fn. The `@tool(...)` prefix STAYS in source so the typed-AST
   surface still sees it at HIR lowering time.

## Surface

```mty
@tool("Description shown to the LLM", cap: fs.read("./data/**"))
fn name(args) -> ret_ty !{effects} { body }
```

- `@<ident>(args...)` — attribute, recognized only when immediately
  preceding `fn`, `agent`, or `protocol` decl.
- For v0.27, accept ONLY `@tool`; unknown `@<ident>` is a clean MT1003
  diagnostic.
- First positional arg: string literal description (required).
- Subsequent named args: `cap: <expr>`, `streaming: <bool>`,
  `name: <str-literal>` (optional).
- `cap:` value goes through the existing expression sub-parser, so
  `fs.read("./data/**")` parses as a method call against the `fs`
  module.

## Diagnostic codes added

- **MT1003** — `unknown attribute @<name>` (only `@tool` accepted in
  v0.27). Emitted at the parser layer with the attribute-name span.
- **MT1004** — `@<name>` attribute decorates a non-fn/agent/protocol
  item. Emitted when the parser sees a TOOL_ATTR prefix on a `struct`,
  `let`, etc.
- **MT6011..MT6016** — re-uses the v0.26 macro-expander codes via the
  attribute pass in `mty_hir::lower::macros::expand_tool_attributes`.

## v0.26 macro patch

`crates/mty-macros/src/stdlib/tool.rs` had two type-check issues the
v0.26 Track B tests didn't catch because no source-level `@tool`
example existed yet:

- `synthesise_descriptor_fn` returned `String` (the owned-buffer ADT
  in stdlib) when string literals lex as `Str`. Swapped to `Str`.
- `synthesise_invoke_fn` emitted a cap-check that referenced
  `std.mcp.current_capability_set()` — fine as a v0.26 macro stub but
  trips MT1001 (`unresolved name`) at every example call site because
  `std.mcp` isn't auto-imported. Dropped the runtime cap-check from
  the synth body (the descriptor + register fns still carry the cap
  text; the Rust-side `mty_stdlib::mcp::register_tool` does runtime
  enforcement).
- `synthesise_register_fn` had the same problem — dropped the
  `std.mcp.register_tool_from_json` call. Will return when the v0.28
  `std.mcp` auto-import lands.

## What Track F (demo 08) can consume

- Source-level `@tool("desc", cap: fs.read("./data/**"))` decorates
  fns directly. No more "documentation in comments" pattern.
- `mty check examples/27_tool_attr.mty` exits 0 — proof that the
  decorator round-trips through parse → lower → typeck.
- Multiple named args (`streaming: true`, `name: "rd"`) parse cleanly;
  unknown attributes (`@route`, `@cache`) raise MT1003.
- `HirFn.tool_attr` carries the decoded description + cap text so a
  Track F mighty-source LLM agent loop can introspect its own tool
  surface via the HIR.

## Owned files (this slice)

- `crates/mty-syntax/src/syntax_kind.rs` — added TOOL_ATTR /
  TOOL_ATTR_ARGS / TOOL_ATTR_CAP_ARG variants.
- `crates/mty-syntax/src/language.rs` — bumped the SyntaxKind upper
  bound assert.
- `crates/mty-syntax/src/parser/items.rs` — added `attr_at` +
  `tool_attr_prefix_ahead` + MT1003 / MT1004 diagnostic emission.
- `crates/mty-ast/src/generated.rs` + `src/items.rs` + `src/lib.rs` —
  added typed accessor `ToolAttr::for_fn_decl` and friends.
- `crates/mty-hir/src/nodes.rs` — added `HirToolAttr` +
  `HirFn.tool_attr`.
- `crates/mty-hir/src/lower/items.rs` — populated `HirFn.tool_attr` in
  `lower_fn`.
- `crates/mty-hir/src/lower/macros.rs` — added
  `expand_tool_attributes` pass; runs before declarative macro
  expansion.
- `crates/mty-macros/src/stdlib/tool.rs` — synth-fn typeck fixes (see
  "v0.26 macro patch").
- `crates/mty-syntax/tests/tool_attribute.rs` — 8 parser tests.
- `crates/mty-hir/tests/tool_attribute_lowering.rs` — 5 lowering tests.
- `examples/27_tool_attr.mty` — proof example, `mty check` exits 0.

## Open follow-ups for v0.28

- The synthesised invoke fn currently returns a `"todo:typed-args"`
  literal. Real JSON-driven arg-deserialisation needs the typed JSON
  ADT to land in stdlib (the v0.28 plan).
- The synthesised register fn doesn't actually call
  `std.mcp.register_tool_from_json` at runtime — the v0.28 wiring will
  re-introduce that call once `std.mcp` is in the auto-prelude.
- The parser accepts only `@tool` by name. The grammar generalises to
  any `@<ident>(args)` so adding `@route` / `@cache` etc. is a
  one-liner in `expand_builtin_attribute`.
