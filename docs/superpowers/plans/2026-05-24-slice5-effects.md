# Stardust Slice 5 Plan — Effects, Capabilities, Traits

**Spec:** `docs/superpowers/specs/2026-05-24-slice5-effects-design.md`
**Repo:** `C:\Users\ihass\stardust` (main, push direct)
**Acceptance:** 266+ tests pass, clippy clean, fmt clean, 20 examples
clean, `v0.5.0-effects` tag pushed.

---

## Task list

### Part A — Capability types

1. **Extend `TyArena` with `Cap`** — add `TyData::Cap { family, constraint }`,
   `CapFamily` enum (Net/Fs/Clock/Dom/Model/Custom), `CapConstraint`
   enum (Any/ReadOnly/Path/Host/And). Intern `Any`-constrained singletons
   for each core family. Update `pretty_ty` printer.
2. **Prelude rewires the five core caps** — `Net`, `Fs`, `Clock`, `Dom`,
   `Model` resolve to `TyData::Cap` (was opaque ADT). Keep the ADT
   shadows for `Page`/`Url`/etc.
3. **Built-in capability methods** — register `.ro(path)`, `.path(path)`,
   `.host(host)` etc. in a slim cap method table that produces new
   `Cap { ..., constraint: ... }` types. Return types are computed at
   call sites (so `fs.ro(p)` returns `Cap { Fs, And([ReadOnly, Path(p)]) }`).
4. **Capability subsumption** — `cap_narrower_or_eq(actual, expected)`
   helper. Plug into `synth_call` and `synth_method_call` parameter
   matching. Emit `SD4010 capability_too_broad` on failure.

### Part B — Effect inference

5. **New `sdust-types::effects` module** — `infer_fn_effects(fn_def, body, defs, ...)`
   walks the body and returns a `HashSet<EffectId>`. Per spec rules.
6. **Fixpoint over the call graph** — `infer_all_effects(typed: &mut TypedPackage)`
   builds an initial empty effect map, then iterates body walks until
   no set changes. Bound by O(N × E).
7. **Public-fn declared vs inferred check** — `validate_pub_effects(...)`
   emits `SD4001 effect_undeclared` listing missing effects.
8. **Strict profile (`profile = "core"`) check** — load `star.toml`,
   read `profile` field. If `"core"`, emit `SD4002 alloc_in_core` for
   any inferred `alloc`.
9. **TypedPackage extension** — add `fn_effects: HashMap<FnId, Vec<EffectId>>`
   (deterministic order). Driver pipeline wires inference between
   typecheck and borrowcheck.

### Part C — Traits + coherence + dispatch

10. **TraitTable in `DefMap`** — add `trait_table: TraitTable` with `impls`,
    `by_method`, `impl_keys`, `trait_methods`.
11. **Trait declaration lowering** — `lower_trait` for `HirTrait`, register
    method signatures in `trait_methods`.
12. **Impl block lowering** — `lower_impl` for `HirImpl` (currently the
    only declared variant; just not lowered). Register methods in
    `impl_methods` AND, when `trait_for` is present, populate
    `by_method` + `impl_keys`. Emit `SD4022 trait_coherence_violation`
    if a duplicate `(trait, self_adt)` pair is added.
13. **Method dispatch upgrade** — replace the slice-4 permissive fallback
    for user ADT receivers. Emit `SD4020 method_ambiguous` (multiple
    matches) and `SD4021 method_not_found` (no matches). Inherent
    `impl_methods` wins.

### Part D — `dyn Trait`

14. **TYPE_DYN parser** — recognize `dyn IDENT` in type position;
    produce a TYPE_DYN node containing a TYPE_PATH child.
15. **HirType::Dyn** — add `Dyn { trait_name: String }` variant + lowering.
16. **TyData::Dyn** — add `Dyn { trait_name: String }`. Pretty as
    `dyn TraitName`.
17. **Object-safety check** — when resolving `HirType::Dyn`, look up the
    trait; if any method has `Self` in its signature or has its own
    generics, emit `SD4023 dyn_requires_object_safe`.
18. **Coercion check at let** — `let h: dyn Hash = user_id` requires
    `(Hash, type(user_id)) ∈ trait_table.impl_keys`. Else SD4023.
19. **Method call on dyn** — look up the trait's method, return its
    declared type; record dyn-coercion in a side table for codegen.

### Part E — `derive(...)`

20. **Lex `#` attribute** — already a token. Parse `#[ident( name+ )]`
    before items.
21. **`derive` keyword tolerance** — treat `derive` identifier as a
    leading-keyword attribute equivalent to `#[derive(...)]`.
22. **HIR carries derives** — `HirStruct.derives`, `HirEnum.derives`.
23. **Apply derives in resolve** — `Copy` adds to `defs.user_copy`;
    `Hash` / `Eq` synthesize an implicit `TraitImpl` entry. Unknown
    name: SD4041.
24. **`is_copy` checks user_copy + field-recursion** — if `aid` is in
    `user_copy`, walk every field and require Copy; else SD4040.

### Part F — Top-level `sandbox`

25. **Parser recognizes top-level sandbox** — `items::item` adds
    `SANDBOX_KW` → `sandbox_decl`. Re-use existing sandbox-expr parser
    body shape (entries + braced block).
26. **HirItem::Sandbox** — new variant `HirTopSandbox { name, entries, body, span }`.
27. **Type-check + borrow-check** — open sandbox tolerance for entries +
    body. No new diagnostics; the body type-checks under unit return.

### Part G — Strict protocol checks

28. **SD4030 arity mismatch** — at handler binding time, count handler
    params vs protocol message params. Error on mismatch.
29. **SD4032 missing handler** — for each implemented protocol, ensure
    every declared message has a handler.
30. **SD4033 extra handler** — convert SD2026 warning into SD4033 error
    when the message name isn't in any implemented protocol.

### Part H — Slice 4 polish

31. **SD3009 `move *ref` modelling** — extend the borrow walker's `move`
    visitor: if the moved expression is a `Unary { Deref, ... }` whose
    inner is a `Ref { .. }`, emit SD3009.
32. **SD3002 vs SD3008 split** — flow.rs's "move while borrow live"
    diagnostic chooses based on whether the move appears in argument
    position (SD3008) or in let/return position (SD3002).
33. **Field-level borrow tracking** — `LocalState` grows `field_borrows`
    map. `&[mut] place.field` records the field; cross-field doesn't
    conflict. Stretch — defer if time tight.

### Part I — Wire + ship

34. **SD4xxx + `sdust explain`** — register every new code in `codes.rs`
    with explain text. Update SDxxxx reference doc.
35. **Negative-test corpus** — `tests/effects_neg/`, `tests/caps_neg/`,
    `tests/traits_neg/`, `tests/derive_neg/`, `tests/protocol_neg/`.
    One fixture per slice-5 diagnostic.
36. **Driver pipeline** — `type_check_and_effect_infer_and_borrow_check`
    chain in `sdust-driver`. Effect inference runs only when
    type-check has no errors.
37. **Example source updates** — examples 04, 06, 11, 18, 19, 20 get
    explicit `effect ...` clauses on `pub`/`export` fns.
38. **Tour pages** — `docs/tour/15-traits.md`, update `10-capabilities.md`,
    `12-arena-budget.md`, `13-protocols.md`.
39. **Docs** — `docs/internals/effects.md`, `docs/internals/capabilities.md`,
    `docs/internals/traits.md`.
40. **Amendments + SLICE5.md** — `docs/spec/v0.1-amendments.md` A22..A30,
    new `SLICE5.md` summary, update `README.md` roadmap.
41. **Tag `v0.5.0-effects`** — after all gates pass; push tag.

---

## Gates after each part

- Compile: `cargo check --workspace` clean.
- Tests: at least the existing 266 pass; new fixtures pass.
- Examples: `sdust check examples/<n>.sd` clean for all 20.
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Format: `cargo fmt --all -- --check` clean.

If any gate fails, the slice-leader fixes before moving on.

## Risk register

- **Capability typing churn**: many call sites in tests bind generic
  param `cap: Fs`. The prelude change from "ADT" to "Cap" might break
  resolution. Mitigation: keep `Fs`/`Net`/etc. registered as
  `DefRef::Adt` pointing to a sentinel ADT whose internal repr is the
  capability — `synth_path` returns a `TyData::Cap` directly when the
  name matches.
- **Effect inference recursion**: the fixpoint must terminate even on
  mutually recursive fns. Mitigation: bound iterations at 32 and
  treat further changes as a bug.
- **Trait coherence over re-checks**: if examples accidentally define
  duplicate impls, SD4022 will block. Mitigation: examples don't use
  traits today; trait coverage starts in slice 5 with the new tour
  chapter.
- **`dyn` parsing collision with `derive`**: both are recognized as
  identifiers, not reserved keywords. We disambiguate by position
  (`dyn` only at the start of a type form; `derive` only at the start
  of an item attribute).
