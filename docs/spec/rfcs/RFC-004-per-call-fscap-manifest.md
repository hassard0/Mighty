# RFC-004 — Per-call FsCap manifest materialisation

**Status:** Draft (v0.9 spec-freeze prep).
**Tracks amendments:** A100 (FsCap process-wide default cap stop-gap),
A109 (per-call FsCap isolation contract — pinned by test but not
yet wired through the MtyIR lower).
**Target release:** v1.1.
**Owner:** *unassigned* — design owner needed before promotion.

## Implementation Status

**NOT YET SHIPPED.** Forward-looking RFC. The A100 stopgap — a
process-wide default `FsCap` installed via `install_default_read_cap`
/ `install_default_write_cap` — **remains the v1.0 enforcement
primitive**. A109's isolation invariant test still pins it.

Adjacent v0.13..v0.23 work that does not pre-empt this RFC:

* **v0.20** — Cluster mTLS opt-in (`ClusterMesh::from_config_mtls`)
  added a parallel "cap-from-config" surface for cluster identities;
  the FsCap world stays process-wide.
* **v0.21** — Cap-name resolver (MT4060..MT4065 active emit) added
  scope-frame resolution for `Fs` / `Net` / `Clock` / `Dom` / `Model`
  names against their cap family + narrowing surface. The resolver
  is the **front half** of the RFC — it knows which cap value should
  flow at each site; the back half (threading the value through the
  MtyIR lower) is what the RFC must still close.

The v1.1 per-call surface is unchanged: each `std.fs.*` call
receives an explicit `FsCap` value threaded from the surrounding
`sandbox` block's `with { fs.ro(...) }` manifest entries.

The window opened 2026-05-26 (close 2026-06-25) is **substantive**.

Cross-references:

* [`v1.0-rc.md`](../v1.0-rc.md) §8.6 — per-call FsCap isolation
  (A100, A109).
* [`RFC_DASHBOARD.md`](RFC_DASHBOARD.md) — live window status.

## Summary

Replace v1.0's process-wide-default `FsCap` (`install_default_read_cap`
/ `install_default_write_cap`) with **per-call manifest materialisation**:
each `std.fs.*` call in MtyIR receives an explicit `FsCap` value
threaded from the surrounding `sandbox` block's `with { fs.ro(...) }`
manifest entries. The process-wide default cap survives for v1.0
back-compat (any call outside a sandbox) but ceases to be the
enforcement primitive.

## Motivation

The v0.5 dogfood pass landed `FsCap::allows(&Path) -> bool` and a
process-wide default cap so `std.fs.read(path)` could refuse
forbidden paths *without* MtyIR lower changes. A100 was a deliberate
stopgap; A109 added an isolation invariant test pinning the per-call
contract. But two real problems remain:

1. **Concurrent sandboxes share the default.** Two sandbox blocks
   open simultaneously each have their own `BudgetTracker` (A43) but
   share the process-wide cap. If sandbox A narrows to `/tmp/a` and
   sandbox B narrows to `/tmp/b`, the second to call
   `install_default_read_cap` overrides the first. Concurrent
   correctness requires per-call threading.
2. **Sub-process workers can't be trusted to install correctly.** Any
   v1.1+ proc macro (RFC-003) or sub-runtime would have to manage
   the global cap state explicitly. Threading per call sidesteps
   the issue.

## Detailed design

### MtyIR shape

Each `Call { func: BuiltinId::FsRead, args: [path_local] }` gains an
implicit prepended argument: `cap_local: FsCap`. The full call shape
becomes `Call { func: BuiltinId::FsRead, args: [cap_local, path_local] }`.
Same for `FsWrite`, `FsExists`, `FsListDir`.

The lowerer responsible for materialising `cap_local`:

1. Walks outward from the call site to the nearest enclosing
   `HirItem::Sandbox` (a `SandboxId`).
2. Reads that sandbox's `with { ... }` manifest entries.
3. Materialises a per-call `FsCap` value at the **sandbox entry**
   (lowered as a `Stmt::Let { local: cap_local, rvalue: FsCap::from_manifest(entries) }`
   at the top of the sandbox body's MtyIR block).
4. Threads `cap_local` into every nested `Fs*` call.
5. Nested sandboxes compose by intersection (existing A43 rule):
   the inner sandbox's `cap_local` is `outer.intersect(&inner)`.

If a `Fs*` call site has **no** enclosing sandbox, the lowerer
emits the call with the materialised `FsCap::Any` (back-compat: A100
process-wide default still applies as a defense in depth, but only
matters for unsandboxed call sites).

### Manifest grammar

Already defined by A23 capability narrowing constraints. The v1.1
materialisation pass reuses the existing constraint algebra:

```mty
sandbox Job with {
  fs.ro("/inputs"),
  fs.rw("/outputs"),
  net.host("api.example.com:443"),
  budget { wall = 30s, mem = 64MiB },
} {
  let raw = fs.read("/inputs/data.json")?    // cap = fs.ro("/inputs")
  let body = process(raw)?
  fs.write("/outputs/result.json", body)?    // cap = fs.rw("/outputs")
}
```

The lowerer materialises one cap per `fs.<kind>` family in the
manifest: `fs_read_cap = FsCap::with_paths(["/inputs", "/outputs"], ReadOnly+ReadWrite)`,
`fs_write_cap = FsCap::with_paths(["/outputs"], ReadWrite)`. Calls
pick the correct cap by operation kind.

### Compile-time vs runtime checks

Two layers:

1. **Compile-time** (typeck, slice-5 A23 surface): the sandbox manifest
   syntax already validates constraint shapes. The materialiser
   produces a deterministic cap shape from a deterministic manifest,
   so static analysis on the manifest determines reachability.
2. **Runtime** (sdust_runtime, A43 surface): the materialised
   `FsCap.allows(path)` check fires at every call. A breach traps with
   `MT5015 sandbox_violation` carrying the cap + path for the
   diagnostic.

### Migration from A100

The `install_default_read_cap` / `install_default_write_cap` APIs and
`current_default_*` accessors are **retained** in v1.1 for two
reasons:

- The host harness (e.g. embedder code outside a `sandbox` block) may
  still want to set a process-wide cap baseline.
- Any v1.0 program that relied on A100 without a sandbox keeps
  working.

But the runtime's `host::dispatch` reads cap from the call args
**first**; only if the call shape is the v1.0 single-arg form does
it fall through to `current_default_*`. v1.1 codegen always emits
the cap-arg form for sandboxed sites; v1.0 codegen continues to emit
the single-arg form (back-compat).

### Capability handle representation

The v0.5 `FsCap` struct (a `Vec<Constraint>` allowlist + a few
metadata bits) is **Copy** under v1.0 § 7.1's blanket opaque-prelude
admission. v1.1 promotes `FsCap` to an explicit
`#[derive(Copy, Clone)]` so the per-call threading does not introduce
ownership friction. Same for `NetCap` and `ClockCap`.

The threaded cap argument is a passed-by-value handle; no reference
semantics, no aliasing concerns. The borrow checker treats it the
same way as a primitive.

## Drawbacks

- **MtyIR shape change.** Every existing tool that walks MtyIR
  `BuiltinId::Fs*` calls needs to handle the new cap-arg shape. A
  one-line `match args { [cap, path] => ..., [path] => ... // v1.0 }`
  pattern is enough but every site must adopt it.
- **Codegen ABI bump.** The cranelift / wasm backends each gain a
  cap-arg parameter on the runtime ABI bridge fn signatures. The
  WIT contract update (RFC-002) needs to thread the cap through too,
  raising a coordination dependency.
- **Manifest visibility.** A function called from inside a sandbox
  needs to receive the cap as a parameter (or inherit it from a
  thread-local; we choose the former). This means stdlib fn
  signatures that take fs ops must add a `cap: FsCap` parameter —
  an ergonomic cost.

## Alternatives considered

1. **Keep A100 forever.** Simpler but unsound for concurrent
   sandboxes; rejected by A109's isolation invariant.
2. **Thread-local cap stack.** Caps pushed/popped per sandbox entry
   to a thread-local. Avoids the MtyIR shape change but breaks for
   tokio tasks that cross worker threads; rejected.
3. **Token-style capabilities (à la `&Cap` borrow).** Cleaner type
   story but requires lifetime-aware borrow check on cap values;
   defer to a future RFC if the explicit-arg form proves awkward.
4. **Implicit cap injection via effect machinery.** A `fs` effect
   already exists (A22); attach the cap as an effect attribute. More
   elegant but harder to make precise — multiple sandboxes that
   contribute to a single effect would need a join rule.

## Unresolved questions

- How does this compose with proc macros (RFC-003)? Proc macros run
  in a sandbox that forbids `fs.*` entirely, so the cap-arg shape is
  N/A for proc-macro callees.
- Should the manifest support per-call cap *expressions* (e.g.
  `fs.ro(some_var)`)? v1.0 requires literal paths; v1.1 materialiser
  could relax this. Punt to RFC-004.b.
- Public-fn signatures that perform fs ops: do they declare the cap
  in their `effect` clause, or as an explicit `cap: FsCap` param?
  Initial answer: explicit param. The effect clause records the
  capability *family* only (`effect fs`).

## Adoption plan

1. **v1.1-alpha.1:** MtyIR shape extended with cap-arg form; codegen
   emits both forms behind a `--per-call-caps` opt-in flag. A100
   default-cap surface unchanged.
2. **v1.1-alpha.2:** stdlib fn signatures audited; fs-using fns add
   `cap: FsCap` parameters. LSP quick-fix for "missing cap arg" on
   v1.0 code being upgraded.
3. **v1.1-beta:** flag flips — codegen emits cap-arg by default;
   `--legacy-process-cap` opts back into A100 for hosts that need
   the v1.0 shape.
4. **v1.1.0:** A100 reclassified SUPERSEDED → RFC-004; A109's
   per-call isolation invariant test gains positive-fire coverage in
   the conformance corpus.
5. **v2.0:** drop `--legacy-process-cap`; remove
   `install_default_*` APIs.

A 30-day public comment window opens with v1.1-alpha.1. Coordinate
with RFC-002 author to land the WIT cap-arg signature changes
together.
