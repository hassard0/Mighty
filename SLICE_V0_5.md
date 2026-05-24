# Mighty v0.5 — Complete

**Tag:** `v0.5.0`
**Date:** 2026-05-24
**Status:** SHIPPED — fifth milestone release. v0.5 is the
"self-hosting + dogfood-completion" milestone: the lexer now
round-trips byte-for-byte against the Rust lexer, every v0.4 demo
stopgap is replaced with the real implementation, declarative
macros gain `name!(args)` invocation syntax + extended hygiene +
cross-file `pub macro` + a proc-macro skeleton, and the LSP grows
the seven advanced features (semantic tokens, rename, inlay hints,
code actions, signature help, workspace folders, semantic
completion).

v0.5 was built by a four-agent autonomous swarm (loop CF +
iterator protocol / dogfood completion / macros completion /
LSP advanced) over a single session, then integrated through this
slice document. Two integration-time fixes were applied: a clippy
nit in `crates/mty-sir/tests/budget_charges.rs` and a WIT/core
import signature alignment in `crates/mty-codegen-wasm/{wit,
tests/dom_imports}.rs`.

## What landed

### Loop control flow + iterator protocol — loop CF agent (commits `1446804`, `47cac2f`, `d6e65de`, `d5f5ecf`)

The v0.4 MtyIR loop terminator fix made loops iterate, but `break`
and `continue` were still bare identifiers and `for x in arr`
ran until the step budget. v0.5 closes the gap.

- `BREAK_KW` / `CONTINUE_KW` lexer tokens; parser recognises
  `break <value>?` and `continue` as `BREAK_EXPR` / `CONTINUE_EXPR`.
- `HirExpr::Break(Option<ExprId>)` + `HirExpr::Continue` HIR nodes.
- Type checker synthesises both as `never` (matches `Return`).
- MtyIR lowering: body terminator routes header → body → continue_tgt
  → header; `break` sets the loop's `result_local` and gotos exit.
- `__sdust_iter_next` wire protocol for `for x in iterable` with
  range + array built-ins; ranges now carry an inclusivity bit
  (`1..5` lowers to `Tuple(1, 5, Bool(false))`).
- Borrow checker: bounded fixed-point at loop back-edges (16-iter
  cap with conservative `join_states + join_ledgers`).
- Self-host lexer bootstrap test unignored: the full token diff
  against the Rust lexer now passes byte-for-byte
  (`selfhost_lexer_full_diff_against_rust`).

Five new conformance cases under `tests/conformance/control_flow/`
(01_break_simple, 02_break_value, 03_continue, 04_nested_break,
05_iter_range). See `LOOPS_V0_5_NOTES.md` for interpretation calls
(no labelled break in v0.5, range-inclusivity-bit shape change,
borrow checker convergence by ledger count).

### Dogfood completion — dogfood agent (commits `1bbe12e`, `396bb46`)

Every v0.4 demo stopgap (per `DEMOS_V0_4_NOTES.md`) is replaced
with the real implementation.

- **Gap 1 — `std.http.serve` binds a real socket.** New
  `mty-stdlib::http_server` module owns a process-wide tokio
  runtime + handle registry; `start_blocking(addr)` binds a TCP
  socket, returns `(handle_id, bound_addr)`, spawns a hyper accept
  loop running the currently installed `AgentDispatch` closure.
  Default closure is a deterministic 200 OK echo so the
  bound-socket smoke test roundtrips cleanly.
- **Gap 2 — `mighty:web/dom` Wasm imports.** Web target's WIT
  carries the four-method DOM interface (set-text, get-text,
  on-click, query) plus the legacy v0.4 handle ops; core module
  declares four matching `(ptr, len)`-shaped imports; `emit_dom_call`
  is wired and reserved (`#[allow(dead_code)]`) until the MtyIR adds
  a `BuiltinId::Dom(...)` variant in v0.6.
- **Gap 3 — Full `Str` method table.** `eval_method` now binds
  real impls for `contains`, `starts_with`, `ends_with`, `find`,
  `char_at`, `slice`, `to_lower`/`to_upper`, `trim` family,
  `split`, `chars`/`bytes`, `replace`, `repeat`, mutable
  `push`/`push_str`/`clear`/`pop`, plus Vec helpers `get`/`first`/
  `last`/`iter`.
- **Gap 4 — CPU + mem auto-charging.** New
  `RunResult::MemBudgetExceeded { used, limit }` variant (exit
  code 4, `MT5009` trap code). Interpreter charges bytes on
  `AdtInit`/`TupleInit`/`ArrayInit` via `estimate_value_bytes` (24 B
  header + recursive payload). New
  `run_fn_with_resource_budget(prog, name, args, host, steps, mem)`
  entry point; `run_fn_with_budget` unchanged (mem 0 ≡ no cap).
- **Gap 5 — `FsCap` allowlist enforcement.** `IoErr::Forbidden(path)`
  variant; process-wide `DEFAULT_READ_CAP` / `DEFAULT_WRITE_CAP`
  slots with `install_default_*` / `current_default_*` helpers;
  `host::dispatch` consults the current default cap on every
  `std.fs.read`/`write`/`exists`/`list_dir` call and returns
  `Result::Err(forbidden:<path>)` for paths outside the allowlist.

See `DOGFOOD_V0_5_NOTES.md` for the per-gap decision log and v0.6
follow-ups (runtime-side agent dispatch wiring for HTTP,
MtyIR-side `BuiltinId::Dom`, per-call cap materialisation from
sandbox manifest).

### Macros completion — macros agent (commits `64cfcc6`, `63d0321`, `fd54f80`)

The v0.4 macro slice shipped expansion + hygiene + HIR
integration. v0.5 finishes the syntactic surface + extends the
hygiene + lights up cross-file + ships the standard library.

- **`name!(args)` invocation syntax.** Parser recognises
  `IDENT BANG L_PAREN` as a `MACRO_CALL` node whose args are a
  single raw `TOKEN_TREE`. v0.4's plain-call syntax
  (`foo(args)` for a registered macro `foo`) continues to work
  for backwards-compat.
- **MT6001 `unknown_macro` activated.** Any `IDENT!(...)` call
  site with no matching `MacroDef` triggers MT6001.
- **Extended hygiene mangling.** `let` bindings in tuple, struct,
  ref, and binding-pattern shapes are mangled, not just
  `let IDENT`. The walker harvests every IDENT inside the pattern
  extent.
- **Cross-file `pub macro`.** `MacroDef` carries `is_pub: bool`;
  per-package registries split into `local` + `exported`; when
  `use otherpkg.foo` resolves and `foo` is in `otherpkg`'s
  exported set, the macro is registered in the importing file's
  local registry.
- **Procedural macro skeleton.** `proc macro` declarations parse
  and register, but execution is gated behind **MT6006**
  `proc_macro_unsupported_v0_5`. MT6005 `proc_macro_impure` fires
  at declaration time if the body's token-tree contains an
  `effect.*` pattern.
- **Standard macro library.** `assert!`, `assert_eq!`, `assert_ne!`,
  `debug!`, `unreachable!` ship as source fixtures under
  `crates/mty-macros/lib/`.

See `MACROS_V0_5_NOTES.md` for the seven interpretation calls
(opaque token-tree args, MT6005-at-decl vs MT6006-at-call, lexical
pattern walker, stdlib as source fixtures, `proc macro` as a
two-keyword form, backwards-compat for `foo(args)` plain calls).

### LSP advanced — LSP agent (commit `f42fa17`)

The v0.2 LSP MVP shipped 7 features (didOpen/didChange/didClose,
publishDiagnostics, hover, definition, formatting, keyword-only
completion). v0.5 closes the documented gaps with seven more.

- **`textDocument/semanticTokens/full` + `/range`** — 14-type +
  3-modifier legend; CST-walk classifier (keywords, types, fns,
  params, strings, numbers, comments, operators, namespaces,
  enum members, type params, macros, properties); delta-encoded.
- **`textDocument/rename` + `prepareRename`** — single-file scope;
  validates new name is a legal Mighty IDENT and not a reserved
  keyword; classifies top-level vs local and restricts the walk
  to the smallest enclosing block for locals.
- **`textDocument/inlayHint`** — inferred-type hints for `let`
  bindings (no `:` annotation) and fn parameters; viewport-filtered;
  uninteresting types (Var / Param / Error) suppressed.
- **`textDocument/codeAction`** — quick fixes for MT2021
  (unresolved value), MT2002 (unresolved type), MT3001
  (use-after-move → `.clone()`), MT4001 (effect undeclared → add
  `effect { name }` to fn signature); Levenshtein ≤ 2.
- **`textDocument/signatureHelp`** — call + method-call sites;
  active parameter via depth-1 comma counting; CALL_EXPR resolves
  via fn lookup; METHOD_CALL_EXPR via built-ins or impl methods.
- **`workspace.workspaceFolders` capability** +
  `workspace/didChangeWorkspaceFolders` and
  `workspace/didChangeWatchedFiles` (logging only; analysis
  remains per-file).
- **Semantic completion** — locals-in-scope (CST walk of
  `LET_STMT` + `FN_PARAM`); receiver-aware method/field completion
  after `.` (resolves receiver via `expr_ty` + `fn_params`, then
  enumerates `impl_methods` + `traits.by_method` + ADT fields).

45 LSP tests across 9 files. See `LSP_V0_5_NOTES.md` for the nine
interpretation calls (single-file rename rationale, HIR×CST
lockstep pairing for inlay hints, semantic tokens vs TextMate
overlay, etc.).

## Tests

- **Workspace: 839 passing** (0 failures, 2 ignored — network /
  pending) — was 692 in v0.4. **+147 tests.**
- **Conformance: 32 cases, 1 driver test, 5 new control_flow
  cases** (01_break_simple, 02_break_value, 03_continue,
  04_nested_break, 05_iter_range) — all pass.
- **Self-host lexer: 4/4 pass**, including the newly-unignored
  `selfhost_lexer_full_diff_against_rust`.
- **0 clippy warnings** with `-D warnings`.
- **`cargo fmt --check` clean.**
- **20/20 examples** check + compile to native objects.
- **20/20 examples** compile to Wasm Components for the
  `wasm32-web` target.
- **3/3 demos** pass `smoke.sh` (search_api, counter_web,
  extract_tool).

## Closed deferrals from v0.4

The v0.4 deferral list named 43 carry-over items. v0.5 closes the
following:

- **`break` / `continue` HIR nodes** (A80)
- **`for` iterator-exhaustion check** via `__sdust_iter_next`
  protocol (A81)
- **Loop back-edge borrow modelling** (A82, conservative
  fixed-point)
- **`mac!name(...)` syntactic marker** (A90)
- **MT6001 unknown_macro activated** (A91)
- **Extended hygiene mangling** beyond `let IDENT` (A92)
- **Cross-file `pub macro`** (A93)
- **Proc macros — parse-and-store skeleton** (A94)
- **Stdlib macros shipped** (A95)
- **`std.http.serve` real binding** (A96)
- **`mighty:web/dom` Wasm imports** (A97)
- **Real `Str` method intrinsics** (A98)
- **MtyIR-side auto-charging for cpu/mem caps** (A99)
- **`FsCap` allowlist enforcement** (A100)
- **LSP advanced features** (A74: semantic tokens, rename, inlay
  hints, code actions, signature help, workspace folders,
  semantic completion)
- **Self-host lexer full-diff against Rust lexer** — was the
  v0.4 `#[ignore]` test; now passes byte-for-byte.

The remaining items from the v0.4 deferral list (parser
precedence, full self-host parser/HIR, registry seed repo,
Polonius-style borrows, WASI Preview 2, DWARF v5, LLVM smoke,
2 ignored conformance cases, etc.) roll into v0.6.

## New amendments (committed to spec)

```
A74 — LSP v0.5 capability expansion (v0.5)
A80 — `break` / `continue` as real HIR nodes (v0.5)
A81 — Iterator protocol via `__sdust_iter_next` (v0.5)
A82 — Loop back-edge fixed-point in the borrow checker (v0.5)
A90 — `name!(args)` macro invocation marker (v0.5)
A91 — MT6001 unknown_macro activated (v0.5)
A92 — Extended hygiene mangling (v0.5)
A93 — Cross-file `pub macro` (v0.5)
A94 — Procedural macros (parse-and-store) + MT6005/MT6006 (v0.5)
A95 — Standard macro library shipped with mty-macros (v0.5)
A96 — `std.http.serve` binds a real socket (v0.5 dogfood)
A97 — `mighty:web/dom` interface added to the `wasm32-web` world (v0.5 dogfood)
A98 — Str method table real impls (v0.5 dogfood)
A99 — `RunResult::MemBudgetExceeded` + Memory auto-charging (v0.5 dogfood)
A100 — FsCap allowlist enforcement via process-wide default cap (v0.5 dogfood)
```

15 new amendments. A74 was renumbered at integration time from a
draft A96 to deconflict with the dogfood A96.

## Headline soundness / correctness improvements

| Property | v0.4 | v0.5 |
|---|---|---|
| `break <value>` exits a `loop` and yields a value | parses as bare IDENT, no effect | **real HIR node + MtyIR lowering** (A80) |
| `continue` skips to loop header | parses as bare IDENT, no effect | **real HIR node + continue_tgt block** (A80) |
| `for x in arr` terminates when arr is exhausted | spins until step budget | **iterator protocol with exhaustion probe** (A81) |
| Borrow check at loop back-edges | one-pass walk | **bounded fixed-point (16-iter cap, conservative joins)** (A82) |
| `name!(args)` parses as a macro call | requires the macro to be registered + plain `foo(args)` | **explicit syntactic marker; MT6001 on unknown** (A90/A91) |
| Macro hygiene covers tuple / struct / ref patterns | `let IDENT` only | **whole pattern subtree mangled** (A92) |
| `use otherpkg.foo` imports a macro | n/a | **cross-file via `pub macro` + per-pkg registry split** (A93) |
| Mighty source lexer round-trips against Rust lexer | first token only (full diff `#[ignore]`d) | **full byte-for-byte diff passes** (loop CF + iterator protocol unblocked) |
| `std.http.serve(addr)` binds a real socket | dispatcher routed to no-op | **real `tokio` + `hyper` listener, default echo dispatcher** (A96) |
| Wasm Component imports `mighty:web/dom` | n/a | **4-method interface + 4 core imports** (A97) |
| `"hello".contains("ll")` | returns `false` (stub) | **real substring match** (A98) |
| MtyIR interp traps on runaway allocation | step budget only | **MemBudgetExceeded with bytes/limit** (A99) |
| `std.fs.read` rejects paths outside allowlist | always succeeded | **`Result::Err(forbidden:<path>)`** (A100) |
| LSP `rename` / `inlayHint` / `semanticTokens` / `codeAction` / `signatureHelp` | absent | **shipped (single-file scope for rename)** (A74) |

## New diagnostic codes

Two new SD codes for proc macros, defined in `sdust_macros::diag`
as bare `u16`:

- **MT6005** — `proc_macro_impure` (fires at declaration time if
  the proc-macro body's token-tree contains an `effect.*` pattern)
- **MT6006** — `proc_macro_unsupported_v0_5` (fires at *call*
  sites for parsed-but-unexecutable proc macros; soft gate so
  test code can verify parsing + storage now and unblock proc-
  macro rollout in v0.6 without source churn)

The v0.4 MT6001..MT6004 codes also stay live; MT6001
`unknown_macro` is now reachable (A91).

## Cross-cut fixes applied during integration

1. **WIT/core-import signature alignment**
   (`crates/mty-codegen-wasm/src/wit.rs`,
   `crates/mty-codegen-wasm/tests/dom_imports.rs`) —
   `get-text` was emitted as `func(id: string) -> string` in
   the WIT but the core module imported it as `(i32,i32) -> i32`.
   The canonical-ABI lift for `-> string` expects
   `(ptr,len,retptr) -> ()`. Changed the WIT signatures for
   `get-text` and `query` to return `u32` (a handle into the
   JS shim's string table) so the WIT lines up with the core
   import. Documented inline; v0.6 will switch back to `string`
   once the return-area bridge is wired in `emit.rs`. Without
   this fix, every wasm32-web example failed
   `component encode: failed to resolve import`.
2. **Clippy nit** (`crates/mty-sir/tests/budget_charges.rs`) —
   `assert!(matches!(res, Ok(_)))` flagged as
   `redundant_pattern_matching`; rewrote as
   `assert!(res.is_ok())`.

Total: 2 cross-cut files touched at integration time. No new
features.

## New deferrals to v0.6

Consolidated from `LOOPS_V0_5_NOTES.md`, `DOGFOOD_V0_5_NOTES.md`,
`MACROS_V0_5_NOTES.md`, `LSP_V0_5_NOTES.md`, plus the WIT
alignment cross-cut.

### Loops / control flow

1. **Labelled break / continue** (`break 'outer`, etc.) — v0.5
   ships unlabelled only. The HIR shape (Option<value> vs tuple
   of label+value) is the choke.
2. **Break-value unification with loop-result type** — v0.5 takes
   the simpler path of "loop result is whatever the MtyIR lowering
   emits". v0.6 will land proper unification.
3. **`Iter[T]` trait surface** — the wire protocol via
   `__sdust_iter_next` is stable; trait-based user iterables wait
   on v0.6 stdlib expansion.
4. **NLL-style liveness on borrow records inside loop bodies** —
   v0.5 takes the conservative join; v0.6 refines.

### Macros

5. **Proc-macro execution** — currently MT6006-gated. Needs a
   sandboxed MtyIR sub-context.
6. **Set-of-scopes hygiene** — replaces v0.5's mangling pass.
7. **`format!`-style variadic macro arguments** — needs an
   expression-shape arg grammar.
8. **`mty-macros::diag` SD6xxx codes** still live in
   `sdust_macros::diag` as bare `u16`, not in
   `mty-diagnostics::codes`. v0.6 cleanup folds them into the
   central catalog.

### Dogfood / runtime

9. **Runtime-side `install_agent_dispatch` wiring** — the
   `http_server` infra is in place; the v0.6 closure body needs
   to look up agents by handle id from the request path and post
   `?Request(req)` through the standard agent mailbox.
10. **MtyIR-side `BuiltinId::Dom { op: DomOp }` lowering** —
    unblocks Mighty source calling `dom.set_text(...)` directly
    instead of routing through hand-written core-module shims.
11. **Per-call FsCap materialisation from sandbox manifest** —
    v0.5 ships the process-wide default cap; v0.6 lifts the
    per-call cap into the MtyIR lower so each `std.fs.read(...)`
    site gets the manifest's narrowed scope.
12. **Canonical-ABI return-area bridge** so `get-text` /
    `query` can return real `string` / `option<string>` in WIT
    instead of u32 handles (relates to the cross-cut fix above).

### LSP

13. **Multi-file / workspace-wide rename + go-to-def** — requires
    a workspace-wide resolve map plumbed through `mty-driver`.
14. **Receiver-chain completion** (`a.b.c.|`) — only the
    immediate binding is resolved today.
15. **Method-call receiver typing** (`a.foo().|`) — MethodCall
    expressions aren't hooked through to their result types in
    the LSP layer.
16. **Borrow check in the LSP pipeline** — still skipped for
    latency reasons; `mty check` remains the authoritative
    oracle.

### Self-hosting

17. **`!fn(args)` parse precedence** (`!is_space(b)` parses as
    `(!is_space)(b)`) — v0.4 deferral, still open.
18. **`extern { fn ... }` real dispatch** — v0.4 deferral, still
    open.
19. **Cross-file module resolution** beyond macros — `use
    selfhost_lexer.SyntaxKind` for non-macro symbols.
20. **Self-host parser + HIR + typeck** — the v0.6 ladder rung
    after the lexer.

### Registry / pkg

21. Create `hassard0/stardust-pkg-registry` and seed it with the
    stdlib.
22. Move `Manifest` into `mty-pkg` (eliminate the duplicate
    parse-of-`mighty.toml` workaround).
23. `[package].include` / `.exclude` globs.
24. Yanked-version support.
25. `mty pkg audit`.
26. Signed releases via sigstore/cosign.
27. Pluggable secret store.
28. Interactive `pkg login`.
29. Real HTTP/registry-mirror backend (`registry+https://`).

### Carried from v0.3/v0.4 (still open)

30. Two-phase borrows, deeper field paths (`s.a.b`), index-aware
    disjointness.
31. Polonius-style conditional-branch ledger joins.
32. Cross-fn region inference (explicit lifetime parameters).
33. Slice-7 supervisor/cap-narrow strict cap-name resolution.
34. Function-signature cap-narrowing.
35. Cross-package Sendable propagation.
36. Sendable lambda capture analysis.
37. MtyIR-side cancellation polling (true mid-turn interrupt).
38. CpuBudget reason wiring.
39. HTTP/protobuf OTLP transport selector.
40. OTel resource-attribute env-vars
    (`OTEL_RESOURCE_ATTRIBUTES`, `OTEL_SERVICE_NAME`).
41. DelayScheduler as default per-turn timer.
42. WASI Preview 2 + user-authored WIT.
43. DWARF v5 + per-instruction line program.
44. `dyn Trait` dispatch + closure capture in compiled code.
45. LLVM backend smoke on Linux/LLVM 17.
46. `capability_checking/03_narrow_to_ro` conformance case.
47. `supervisor_restart/02_escalate` conformance case + grammar.

## Stats

- **11 commits since v0.4.0** (four swarm waves + intra-swarm
  follow-ups + integrator pass).
- **9,857 insertions / 404 deletions** across 110 files.
- **Workspace stays at 20 crates** (no new crate this slice; all
  v0.5 work landed in existing crates).
- **+147 new tests** (692 → 839).
- **0 clippy warnings** with `-D warnings`.
- **20/20 examples build to native objects.**
- **20/20 examples build to wasm32-web Components.**
- **5 new conformance cases** under `tests/conformance/control_flow/`.
- **3/3 dogfood demos pass `smoke.sh`** (search_api, counter_web,
  extract_tool).
- **15 new spec amendments** (A74 + A80..A82 + A90..A100).
- **2 new SD codes** (MT6005, MT6006).
- **MSRV unchanged at 1.85.**

## Known issues

1. **Borrow checker's 16-iteration cap is a safety valve.** If a
   real program hits it, the analysis is conservative (joins all
   iterations seen) rather than unsound, but the cap will need
   revisiting if reaching it becomes common. None of the in-tree
   examples hit it.
2. **WIT `get-text` / `query` return `u32` not `string` /
   `option<string>`.** This is the integration-time fix; v0.6
   restores the real return types once the canonical-ABI
   return-area bridge is wired in `emit.rs`.
3. **Proc macros are gated behind MT6006.** v0.5 ships
   parse-and-store only; execution waits on the sandboxed MtyIR
   sub-context (v0.6).
4. **LSP rename is single-file.** Cross-file rename needs a
   workspace-wide resolve map; v0.5 ships the protocol surface
   with a documented restriction. The editor preview lets users
   catch unintended cross-file rewrites.
5. **HTTP serve default is an echo dispatcher.** Real per-agent
   routing waits on `install_agent_dispatch` wiring at runtime
   startup (v0.6).
6. **DOM MtyIR lowering is reserved** (`emit_dom_call` is
   `#[allow(dead_code)]`). Mighty source calling
   `dom.set_text(...)` directly waits on `BuiltinId::Dom`
   (v0.6).
7. **FsCap is process-wide**, not per-call. v0.6 lifts the
   per-call cap from the sandbox manifest into the MtyIR lower so
   each `std.fs.read(...)` site carries its narrowed scope.
8. **`mty-macros` SD6xxx codes** still live in
   `sdust_macros::diag` as bare `u16` — central catalog merge is
   v0.6 cleanup.
9. **Carried from v0.3/v0.4**: 2 conformance cases still ignored,
   OTLP transport gRPC-only, LLVM backend untested on this build
   host, slice-7 supervisor/cap-narrow scopes strict-but-open.

## What's next

v0.6 picks up the 47-item deferral catalogue. Likely themes:

- **`BuiltinId::Dom` + canonical-ABI return-area bridge** —
  finishes the v0.5 Wasm Component DOM surface end-to-end.
- **`install_agent_dispatch` wiring at runtime startup** —
  finishes the v0.5 HTTP serve infra.
- **Labelled break / continue + iterator trait surface** —
  finishes the v0.5 loop work.
- **Set-of-scopes macro hygiene** — replaces v0.5's mangling
  pass.
- **Self-host parser + HIR + typeck** — the next ladder rung
  after the v0.5 lexer.
- **Multi-file LSP analysis** — workspace-wide rename / go-to-def.
- **Polonius-style borrow checker** — conditional-branch join
  refinement + two-phase borrows.
- **WASI Preview 2 + user-authored WIT** in the Component
  pipeline.

The aspirational v0.6 tagline: *"the compiler runs its own parser,
the runtime dispatches its own HTTP requests, and the
language-server understands the whole workspace."*
