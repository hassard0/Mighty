# Traits, Coherence, and `dyn` (slice 5)

Slice 5 ships first-class trait dispatch with name-only coherence and
conservative `dyn` object safety.

## Trait declaration

```sd
trait Hash {
  fn hash(self) -> U64
}
```

Lowered to `HirItem::Trait(HirTrait { name, generics, methods, ... })`.
At resolve time the type checker walks each trait's methods and
populates `defs.traits.trait_methods[name] = Vec<TraitMethodSig>`. The
`TraitMethodSig` records:

- `name`
- `has_self_ty` — true if any param/ret references the `Self`
  identifier
- `has_generics` — true if the method has its own generic params

These flags drive the slice-5 object-safety check at `dyn Trait`
usage sites.

## Impl declaration

```sd
impl Hash for UserId {
  fn hash(self) -> U64 { 0 }
}
```

Lowered to `HirItem::Impl(HirImpl { trait_for, self_ty, methods, ... })`.
At resolve time we:

1. Resolve `self_ty` to an `AdtId`.
2. Resolve `trait_for` to a trait name (single ident; multi-segment
   reserved for later slices).
3. Coherence: if `(trait, self_adt)` already exists in
   `defs.traits.impl_keys`, emit `MT4022 trait_coherence_violation`.
4. For each method, allocate a `FnDef` and:
   - If `trait_for` is None → register as inherent method in
     `defs.impl_methods` (slice-4 behavior).
   - If `trait_for` is Some → record in `defs.traits.impls`, populate
     `defs.traits.by_method[(self_adt, method)]`, and add to
     `impl_keys`.

## Dispatch

Method call resolution on a `Adt(aid, _)` receiver:

1. **Inherent first**: look up `defs.impl_methods[(aid, method)]`. Found
   → resolve to that FnDef.
2. **Trait fallback**: collect `defs.traits.by_method[(aid, method)]`.
   Exactly one → dispatch to that FnDef. Multiple → emit
   `MT4020 method_ambiguous` listing trait candidates.
3. **Neither** → fall through to the slice-4 built-in table or, for
   user (non-opaque) ADTs, emit `MT4021 method_not_found` /
   `MT2007 unknown_method` (kept for back-compat).

## `dyn Trait`

Parse: `dyn IDENT` as a type form (no generic args allowed in slice 5).
Lowered to `HirType::Dyn { trait_name }`. The resolver maps this to
`TyData::Dyn { trait_name }`.

Object-safety check at every `Dyn` resolution: walk the trait's
`TraitMethodSig` list; if any method has `has_self_ty` or
`has_generics`, emit `MT4023 dyn_requires_object_safe`. (Slice 5
conservative: a slightly looser rule may follow when generic methods
are box-callable via vtables.)

## `derive(Copy / Hash / Eq)`

`#[derive(Copy)]` validates that every field's type is itself Copy
(walks `is_field_copy`); failure → `MT4040 derive_copy_field_not_copy`.
On success, the ADT id is inserted into `defs.user_copy`.

`#[derive(Hash)]` / `#[derive(Eq)]` register a synthetic `TraitImpl`
entry. The method body isn't generated (no codegen yet); the entry
simply makes `let h: dyn Hash = my_t` resolve.

v0.3 (A65) adds `#[derive(Sendable)]` — a pure marker that opts the
ADT into the cross-agent Sendable trait (`crate::sendable`). No
field-level validation is run at the derive site; the per-send
MT3011 check enforces the actual contract at every `!Msg(...)` /
`?Msg(...)` use. See `docs/internals/sendable.md`.

Unknown derive names → `MT4041 derive_unknown`.

## Diagnostics

| Code   | Name                          |
|--------|-------------------------------|
| MT3011 | non_sendable_message_arg      |
| MT4020 | method_ambiguous              |
| MT4021 | method_not_found              |
| MT4022 | trait_coherence_violation     |
| MT4023 | dyn_requires_object_safe      |
| MT4031 | protocol_param_type_mismatch  |
| MT4040 | derive_copy_field_not_copy    |
| MT4041 | derive_unknown                |
