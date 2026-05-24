# Stardust v0.5 — Release Notes

**Tag:** `v0.5.0`
**Date:** 2026-05-24
**Status:** SHIPPED — fifth milestone release. Self-hosting lexer
round-trips byte-for-byte against the Rust lexer, every v0.4 demo
stopgap is replaced with the real implementation, declarative
macros gain `name!(args)` invocation + extended hygiene +
cross-file `pub macro` + a proc-macro skeleton, and the LSP grows
seven advanced features (semantic tokens, rename, inlay hints,
code actions, signature help, workspace folders, semantic
completion).

Stardust v0.1 walked the spec §31 ladder end-to-end. v0.2 lit up
every surface the v0.1 deferral list named. v0.3 hardened
soundness. v0.4 was dogfood + ecosystem (3 demos, real registry,
declarative macros, self-host lexer subset, SIR loop terminator
fix). v0.5 is the **self-hosting + dogfood-completion** milestone:
loops actually terminate via `break` / `continue` / iterator
exhaustion, the Stardust lexer is now a working bootstrap, every
v0.4 demo's stopgap has its real implementation, and the LSP
grows the advanced features editors expect.

## What you can do (new in v0.5)

```bash
# break / continue work
sdust run examples/06_for_while_loop.sd          # for loops terminate naturally

# The Stardust lexer round-trips byte-for-byte against the Rust lexer
cargo test -p sdust-driver --test selfhost_lexer
# → 4 passed (selfhost_lexer_full_diff_against_rust now passes)

# `std.http.serve` actually binds a TCP socket
cargo test -p sdust-stdlib --test http_serve_real

# Real Str methods
echo 'fn main() { println("hello".contains("ll"))  /* prints true */ }' | sdust run -

# Hygienic macros via the explicit `!` marker
sdust check examples/16_macro.sd                  # `mac!(args)` parses; SD6001 on unknown

# Mem-budget violations trap deterministically
# (run_fn_with_resource_budget exposes the cap to embedders)

# LSP advanced features land
# In VS Code:
#   - Rename (F2) — single-file scope, validated against keywords
#   - Inlay hints — inferred types on `let` and fn params
#   - Semantic tokens — finer-grained highlighting than the TextMate grammar
#   - Code actions — quick fixes for SD2021 / SD2002 / SD3001 / SD4001
#   - Signature help — at call + method-call sites
```

Everything from v0.4 still works the same way.

## The four swarm agents

v0.5 was built by 4 autonomous swarm agents working disjoint
crate boundaries, then integrated through this release:

| Agent | Crates / files | Commits |
|---|---|---|
| loop CF + iterator protocol | `sdust-syntax`, `sdust-hir`, `sdust-types`, `sdust-borrow`, `sdust-sir`, conformance, selfhost test | `1446804`, `47cac2f`, `d6e65de`, `d5f5ecf` |
| dogfood completion (5 gaps) | `sdust-stdlib` (http_server, fs), `sdust-codegen-wasm` (wit + emit), `sdust-sir` (mem budget + str methods) | `1bbe12e`, `396bb46` |
| macros completion | `sdust-syntax`, `sdust-macros`, `sdust-hir`, `crates/sdust-macros/lib/*.sd` | `64cfcc6`, `63d0321`, `fd54f80` |
| LSP advanced | `sdust-lsp` (semantic_tokens, rename, inlay_hints, code_actions, signature_help, workspace_folders, completion_semantic) | `f42fa17` |

Plus two integration-time changes: the WIT / core-import
signature alignment in `crates/sdust-codegen-wasm/{wit,
tests/dom_imports}.rs` and a clippy nit in
`crates/sdust-sir/tests/budget_charges.rs`.

## Headline numbers

- **839 tests pass** (0 failures, 2 ignored — network /
  pending) — was 692 in v0.4
- **+147 tests** added in v0.5
- **0 clippy warnings** with `-D warnings`
- **`cargo fmt --check` clean**
- **20 crates** in the workspace (unchanged from v0.4)
- **11 commits** since `v0.4.0`
- **9,857 insertions / 404 deletions** across 110 files
- **20/20 examples compile to native objects** (unchanged from v0.4)
- **20/20 examples compile to wasm32-web Components** (unchanged
  from v0.4)
- **32 conformance cases run** (unchanged from v0.4), 2 still
  ignored (unchanged)
- **5 new conformance cases** under
  `tests/conformance/control_flow/` (01_break_simple,
  02_break_value, 03_continue, 04_nested_break, 05_iter_range)
- **3/3 dogfood demo smoke scripts pass** (search_api,
  counter_web, extract_tool)
- **Self-host lexer: 4/4 pass**, including the newly-unignored
  byte-for-byte diff against the Rust lexer
- **15 new spec amendments** (A74 + A80..A82 + A90..A100)
- **2 new SD codes** (SD6005, SD6006, both for proc macros)
- **MSRV unchanged at 1.85**

## Correctness assertions newly enforced

| Property | v0.4 | v0.5 |
|---|---|---|
| `break <value>` exits a `loop` + yields a value | parses as bare IDENT, no effect | real HIR node + SIR lowering (A80) |
| `continue` skips to loop header | parses as bare IDENT, no effect | real HIR node + continue_tgt block (A80) |
| `for x in arr` terminates on exhaustion | spins until step budget | iterator protocol via `__sdust_iter_next` (A81) |
| Borrow check at loop back-edges | one-pass walk | bounded fixed-point (16-iter cap) (A82) |
| `name!(args)` parses as a macro call | required registration + plain `foo(args)` | explicit `!` marker; SD6001 on unknown (A90/A91) |
| Macro hygiene covers tuple / struct / ref patterns | `let IDENT` only | whole pattern subtree mangled (A92) |
| `use otherpkg.foo` imports a macro | n/a | cross-file via `pub macro` + per-pkg registry split (A93) |
| Stardust lexer source round-trips against Rust lexer | first token only (full diff `#[ignore]`d) | byte-for-byte (loop CF + iterators unblocked) |
| `std.http.serve(addr)` binds a real socket | dispatcher routed to no-op | real `tokio` + `hyper` listener (A96) |
| Wasm Component imports `stardust:web/dom` | n/a | 4-method interface + 4 core imports (A97) |
| `"hello".contains("ll")` | returned `false` (stub) | real substring match (A98) |
| SIR interp traps on runaway allocation | step budget only | MemBudgetExceeded with bytes / limit (A99) |
| `std.fs.read` rejects paths outside allowlist | always succeeded | `Result::Err(forbidden:<path>)` (A100) |
| LSP `rename` / `inlayHint` / `semanticTokens` / `codeAction` / `signatureHelp` | absent | shipped (rename = single-file) (A74) |

## Closed deferrals from v0.4

The v0.4 deferral list named 43 carry-over items. v0.5 closes:

- **`break` / `continue` HIR nodes** — shipped (loop CF agent)
- **`for` iterator-exhaustion check** — shipped (`__sdust_iter_next`
  wire protocol)
- **Loop-back-edge borrow modelling** — shipped (bounded
  fixed-point)
- **`mac!name(...)` syntactic marker** — shipped (macros agent)
- **Set-of-scopes hygiene** — partially shipped (extended
  mangling to tuple / struct / ref patterns; full set-of-scopes
  is v0.6)
- **Proc macros** — parse-and-store skeleton shipped; execution
  gated behind SD6006 (v0.6)
- **Cross-file macro export + visibility (`pub macro foo`)** —
  shipped
- **Stdlib macros** — shipped (`assert!`, `assert_eq!`,
  `assert_ne!`, `debug!`, `unreachable!`)
- **`std.http.serve` host bridge** — shipped (`sdust-stdlib::http_server`)
- **`stardust:web/dom` import lowering** (Wasm side) — shipped
  (WIT + core imports; SIR-side `BuiltinId::Dom` is v0.6)
- **Auto-charging in the SIR interpreter** — shipped
  (`MemBudgetExceeded`)
- **Real `Str` method intrinsics** — shipped (full table)
- **LSP rename / inlay hints / semantic tokens / code actions /
  signature help / workspace folders / semantic completion** —
  shipped (single-file rename for v0.5)
- **Self-host lexer full-diff against Rust lexer** — shipped
  (was v0.4 `#[ignore]`; now passes)

The remaining items (parser precedence, full self-host parser,
registry seed repo, Polonius-style borrows, WASI Preview 2, DWARF
v5, LLVM smoke, 2 ignored conformance cases, etc.) roll into v0.6.

## Spec amendments (15 new)

```
A74 — LSP v0.5 capability expansion
A80 — `break` / `continue` as real HIR nodes
A81 — Iterator protocol via `__sdust_iter_next`
A82 — Loop back-edge fixed-point in the borrow checker
A90 — `name!(args)` macro invocation marker
A91 — SD6001 unknown_macro activated
A92 — Extended hygiene mangling
A93 — Cross-file `pub macro`
A94 — Procedural macros (parse-and-store) + SD6005/SD6006
A95 — Standard macro library shipped with sdust-macros
A96 — `std.http.serve` binds a real socket (dogfood)
A97 — `stardust:web/dom` interface added to wasm32-web (dogfood)
A98 — Str method table real impls (dogfood)
A99 — `RunResult::MemBudgetExceeded` + memory auto-charging (dogfood)
A100 — FsCap allowlist enforcement via process-wide default cap (dogfood)
```

All committed to `docs/spec/v0.1-amendments.md`. A74 was
renumbered at integration time from a draft A96 to deconflict
with the dogfood A96.

## Diagnostic codes

Two new SD codes for proc macros, defined in `sdust_macros::diag`
as bare `u16` and wrapped at emission:

- **SD6005** — `proc_macro_impure` (fires at declaration time if
  the proc-macro body's token-tree contains an `effect.*` pattern)
- **SD6006** — `proc_macro_unsupported_v0_5` (fires at *call*
  sites for parsed-but-unexecutable proc macros)

`sdust explain SD6xxx` is not yet wired (v0.6 cleanup folds the
SD6xxx codes into `sdust-diagnostics::codes`).

The v0.4 SD6001..SD6004 codes also stay live; SD6001
`unknown_macro` is now reachable from the new
`IDENT!(...)` parse path (A91).

## Toolchain

- **MSRV: Rust 1.85** (unchanged from v0.2)
- No new workspace crates (v0.5 work landed in existing crates)
- `sdust-stdlib` gains an `http_server` module backed by `tokio`
  + `hyper`
- All-platform: Windows, macOS, Linux
- Cargo workspace; no `build.rs` magic

## Backwards compatibility

v0.5 is a minor-version bump from v0.4. Source compatibility is
preserved for slice 1-8 + v0.2 + v0.3 + v0.4 surfaces. **Notable
behaviour changes**:

- **Loops with `break` now actually exit.** Code that relied on
  v0.4's "break-as-IDENT no-op" + the interpreter's step budget
  to bound runaway loops will now exit at the `break`. The v0.4
  shape was already broken (the bare IDENT had no semantic
  meaning), so no real program relied on it.
- **`for x in arr` now terminates** on iterator exhaustion rather
  than spinning until the step budget. Same caveat as above —
  the v0.4 shape was broken.
- **Range literals `1..5` lower to a 3-tuple** `(start, end,
  inclusive_bit)` rather than the v0.4 2-tuple `(start, end)` to
  let the iterator distinguish exclusive (`<`) from inclusive
  (`<=`). Backwards-incompatible with any code that pattern-
  matched the 2-tuple shape — but no such code exists yet in the
  workspace.
- **`name!(args)` is now the explicit macro-call marker**, and
  unknown macros at this shape fire **SD6001**. v0.4's plain-call
  syntax (`foo(args)` for a registered macro `foo`) continues to
  work for backwards-compat, so existing examples don't churn.
- **`std.http.serve(addr)` now binds a real TCP socket.** Code
  that called `serve` for its return-sentinel side effect without
  expecting a real bind will now actually consume a port; the
  default echo dispatcher responds 200 OK on every request until
  `install_agent_dispatch` is called.
- **`std.fs.read` / `write` / `exists` / `list_dir` now reject
  paths outside the process-wide default `FsCap`.** Before v0.5
  the dispatcher silently succeeded. The new behaviour returns
  `Result::Err(forbidden:<path>)`.
- **WIT `get-text` / `query` signatures changed** from
  `func(id: string) -> string` / `func(selector: string) -> option<string>`
  to `func(id: string) -> u32` / `func(selector: string) -> u32`
  (handles into the JS shim's string table). This is the
  integration-time fix; v0.6 restores the real return types once
  the canonical-ABI return-area bridge is wired.

Diagnostic codes (SD0001..SD8010 + SD6001..SD6004 + SD6005..SD6006)
are otherwise unchanged. CLI shape is unchanged.

## Known issues

1. **Borrow checker's 16-iteration cap is a safety valve.** None
   of the in-tree examples hit it; the analysis is conservative
   (joins all iterations seen) rather than unsound.
2. **WIT `get-text` / `query` return `u32` not `string` /
   `option<string>`** — integration-time fix to line up with the
   core import signature. v0.6 restores real string returns.
3. **Proc macros are gated behind SD6006** — parse-and-store
   only; execution waits on a sandboxed SIR sub-context.
4. **LSP rename is single-file** — protocol surface shipped with
   a documented restriction; multi-file waits on a workspace-
   wide resolve map.
5. **HTTP serve default is an echo dispatcher** — real per-agent
   routing waits on `install_agent_dispatch` wiring at runtime
   startup.
6. **DOM SIR lowering is reserved** — `emit_dom_call` is
   `#[allow(dead_code)]`; Stardust source calling
   `dom.set_text(...)` directly waits on `BuiltinId::Dom`.
7. **FsCap is process-wide, not per-call** — v0.6 lifts the
   per-call cap from the sandbox manifest.
8. **`sdust-macros` SD6xxx codes** still live in
   `sdust_macros::diag` as bare `u16`; central catalog merge is
   v0.6 cleanup.
9. **Carried from v0.3/v0.4**: 2 conformance cases still ignored,
   OTLP transport gRPC-only, LLVM backend untested on this build
   host, supervisor/cap-narrow scopes strict-but-open.

## Acknowledgments

v0.5 is the fourth Stardust release built by autonomous parallel
agents. The four swarm agents shipped tightly because each touched
disjoint crates — loop CF in the parser/HIR/SIR/borrow stack vs
dogfood in stdlib + codegen-wasm + SIR runtime vs macros in
syntax/macros/HIR vs LSP in `sdust-lsp` — and the integrator only
needed two cross-cut edits (clippy nit + WIT/core import
alignment). The agents stood on the slice-1..8 + v0.2 + v0.3 +
v0.4 foundations: the declarative parser, the typed HIR / SIR /
interpreter, the Cranelift / wasm / Component pipelines, the v0.3
host bridge through `sdust_runtime::host_std::install_dispatcher`,
the v0.4 sdust-macros expansion + sdust-pkg registry transport,
and the conformance harness all carried forward without rewrites.

Big thanks to the `tokio`, `hyper`, `wit-component`, `wasmparser`,
`wasm-encoder`, and `rowan` teams — the HTTP listener, the wasm
component pipeline, and the LSP semantic-tokens machinery all
stand on those shoulders.

## What's next

v0.6 picks up the 47-item deferral catalogue. The aspirational
v0.6 tagline: *"the compiler runs its own parser, the runtime
dispatches its own HTTP requests, and the language-server
understands the whole workspace."*
