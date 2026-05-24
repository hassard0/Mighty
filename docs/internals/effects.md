# Effect Inference (slice 5)

Stardust effects describe observable authority and runtime behavior
(spec §9). Slice 5 ships the inference + validation pipeline.

## Pipeline

```
parse → lower → typecheck → effect_infer → borrowcheck
```

Effect inference runs inside `sdust_types::items::check_typed` after
expression typing settles, before the side tables are handed to the
borrow checker.

## Algorithm

1. **Per-fn pass:** walk every fn body and record:
   - Effect set from syntactic constructs:
     - `arena ...` → `alloc`
     - `spawn ...`, `target!Msg(...)`, `target?Msg(...)`, `detach` → `spawn`
     - `expr @ duration` → `time`
     - `unsafe { ... }` → `unsafe`
     - `html"..."` template → `alloc`
     - Map literal → `alloc`
     - Method receivers/path-callees prefixed by `fs.`, `net.`, `clock.`,
       `dom.`, `model.` → corresponding cap effect.
     - Container method names (`push`, `pop`, `insert`, `encode`,
       `collect`, `clone`, `to_string`) → `alloc`.
   - Callee list (fn-by-name calls).

2. **Fixpoint:** until no set changes (bounded at 32 iterations), for
   each fn, union the callee fns' inferred and declared effect sets.

3. **Public-fn discipline:** if `pub fn`'s inferred set is non-empty,
   verify the declared `effect ...` clause is a superset. Else
   `SD4001 effect_undeclared` with the missing effects listed.

4. **Profile gate:** if `star.toml` declares `profile = "core"`, any
   `pub fn` with `alloc` in its inferred set triggers
   `SD4002 alloc_in_core`.

## Limitations

- Capability effects use a path-prefix heuristic on the receiver
  expression, not a typed dispatch. A bound `let alias = net; alias.get(...)`
  would not contribute `net` today. Refinement: post-v0.1.
- Recursion via dynamic dispatch (`dyn Trait` method calls) does NOT
  propagate effects through the dyn (slice 5 keeps `dyn` effect-free
  to avoid pessimistic over-tagging).
- Inferred effect names exposed today: `alloc`, `net`, `fs`, `time`,
  `dom`, `model`, `spawn`, `unsafe`. The spec also lists `io`, `rand`,
  `block` — those are reserved for slice 6.

## Side table

`TypedPackage.fn_effects: HashMap<FnId, Vec<EffectId>>` — every fn's
inferred effects in deterministic order. Consumers (LSP, codegen,
docs) read from here.
