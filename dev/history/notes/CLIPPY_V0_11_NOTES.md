# Strict clippy (`clippy::pedantic`) gate — v0.11

## Outcome

The `clippy (strict)` CI job is now **required** (no
`continue-on-error`). `cargo clippy --workspace --all-targets --
-D warnings` exits 0 with the workspace-level `[lints.clippy]`
table in `Cargo.toml` setting `pedantic = "warn"` and allow-listing
the lints below.

## Audit numbers

| Stage                                  | Findings |
| -------------------------------------- | -------- |
| Baseline `-W clippy::pedantic`         | **2341** warnings (55 distinct lints) |
| After workspace allowlist              | ~30 real lints remaining |
| After fixes + extra allowlist tweaks   | **0** |

## Workspace-level allowlist

These are kept at `level = "allow"` in `[workspace.lints.clippy]`
because they're style preferences, not bugs. They generate a lot
of noise without finding actual problems in our codebase.

### High-noise style preferences

- `module_name_repetitions` — module names mirror types for
  discoverability (e.g. `parser::ParseError`).
- `missing_errors_doc`, `missing_panics_doc` — we use `thiserror`
  so each error variant self-describes; per-fn `# Errors` blocks
  would be redundant.
- `similar_names`, `many_single_char_names` — arena/IR code
  conventionally uses short names (`a`, `b`, `id`, `tid`).
- `too_many_lines` — `mty-diagnostics::explain` is a giant lookup
  table by design; some lowering matches are inherently long.
- `unreadable_literal` — long literals are sometimes raw bit
  patterns where breaking them up is misleading.
- `single_match_else`, `if_not_else` — both forms read fine; we
  use whichever matches the surrounding voice.
- `doc_markdown` — false-positives on technical prose
  (`CamelCase` paths, ANSI escapes, file extensions).
- `match_same_arms` — sometimes intentional for documentation:
  each arm corresponds to a distinct concept.
- `must_use_candidate`, `return_self_not_must_use` — most
  builder/pure functions are self-evidently must-use; the
  attribute adds noise.
- `needless_pass_by_value` — keeping `T` vs `&T` consistent at
  the API boundary often beats microoptimization.
- `uninlined_format_args` — both `format!("{x}")` and
  `format!("{}", x)` are fine.
- `redundant_closure_for_method_calls` — `|x| x.to_string()` vs
  `str::to_string`: explicit closure is often clearer for
  readers unfamiliar with method references.
- `map_unwrap_or` — `.map(f).unwrap_or(d)` reads more
  naturally than `.map_or(d, f)` for the lookup-then-default
  shape.
- `unused_self` — preserved for forward compatibility when the
  receiver will be used in a future expansion.
- `struct_field_names` — `Span { start_byte, end_byte }` is
  unambiguous.
- `fn_params_excessive_bools` — config structs sometimes take
  many bools; flag enums are heavier.

### Numeric casts (audited at use sites)

The compiler is full of legitimate `usize → u32` for span lengths,
`u32 → i64` for IR consts, etc. We don't want a `From` cast at
each one.

- `cast_possible_truncation`
- `cast_possible_wrap`
- `cast_sign_loss`
- `cast_lossless`
- `cast_precision_loss`

### Iteration / pattern style

- `enum_glob_use`, `wildcard_imports` — `use SyntaxKind::*;` in
  large lowering matches is clearer than the wall of imports.
- `items_after_statements` — nested helper fns inside larger
  fns are readable.
- `elidable_lifetime_names` — explicit `'a` is occasionally
  clearer than elision.
- `explicit_iter_loop` — both forms are equivalent.
- `ignored_unit_patterns` — `let _: () = …` is sometimes
  intentional documentation.
- `default_trait_access` — `T::default()` is more searchable
  than `Default::default()`.
- `implicit_hasher` — we don't expose hash-builder parameters.
- `ignore_without_reason` — `#[ignore]` on optional tests is
  fine; a reason isn't always meaningful.

### Other style allows

- `format_push_string` — `format!(..).push_str` reads cleaner
  in `mty-doc`'s renderer than the `write!` + `fmt::Write`
  trait import dance.
- `bool_to_int_with_if` — sometimes clearer than `usize::from`.
- `comparison_chain` — three-way `if a < b … else if a > b …`
  reads better than `match a.cmp(&b)` for numeric heuristics.
- `unnecessary_wraps` — placeholder `Result` returns kept for
  forward error paths.
- `match_wildcard_for_single_variants` — wildcard arms are
  sometimes more readable than enumerating one variant.
- `single_char_pattern` — `contains("!")` vs `contains('!')`,
  matter of taste.
- `needless_raw_string_hashes` — `r#"…"#` is consistent across
  raw strings that may need hashes elsewhere.
- `float_cmp` — interpreter compares NaN canonically; intentional.
- `manual_string_new` — `String::default()` is sometimes clearer.
- `no_effect_underscore_binding`, `used_underscore_binding` —
  `_ty` placeholders are deliberate.
- `duration_suboptimal_units` — `from_secs(60)` is fine; we
  don't need `from_mins`.
- `unused_async` — runtime APIs keep `async` for stability even
  when the current body is sync (callers already `.await`).
- `missing_fields_in_debug` — formatters intentionally elide
  large/cyclic fields.
- `semicolon_if_nothing_returned` — `b.iter(|| { … })` and
  `b.iter(|| { …; })` are equivalent in criterion closures.
- `implicit_clone` — `path.to_path_buf()` is fine.
- `stable_sort_primitive` — we sometimes want stable sort for
  determinism even on primitives.
- `manual_assert` — `if c { panic!(…) }` reads better than
  `assert!(!c, …)` in some places.

## Real fixes landed

About 30 actual call sites were changed to satisfy lints we
*do* enforce:

| Lint                          | Count | Fix                                                          |
| ----------------------------- | ----- | ------------------------------------------------------------ |
| `manual_let_else`             | ~18   | `match x { Some(y) => y, None => return }` → `let Some(y) = x else { return };` |
| `unnested_or_patterns`        | ~8    | `Some(A) \| Some(B)` → `Some(A \| B)`                        |
| `assigning_clones`            | ~6    | `x = y.clone()` → `x.clone_from(&y)`                         |
| `format_push_string`          | 4 (kept) | `s.push_str(&format!(…))` → `write!(s, …)` in benches    |
| `implicit_clone`              | 1     | `name.to_string()` → `name.clone()` (`&String`)              |
| `manual_is_variant_and`       | 1     | `.ok().filter(p).is_some()` → `.ok().is_some_and(p)`         |
| `match_wildcard_for_single_variants` | 1 | enumerated the variant in `mty-hir/tests`                |
| `single_char_pattern`         | 2     | `contains("!")` → `contains('!')`                            |
| `manual_string_new`           | 1     | `"".into()` → `String::new()`                                |
| `missing_fields_in_debug`     | 3     | `.finish()` → `.finish_non_exhaustive()`                     |
| `needless_continue`           | 1     | `=> continue` removed from match arm at end-of-loop          |
| `used_underscore_binding`     | 1     | `_ty` param renamed when returned                            |

## Inheritance mechanism

Every member crate's `Cargo.toml` now ends with:

```toml
[lints]
workspace = true
```

The workspace's `[workspace.lints.clippy]` table sets
`pedantic = { level = "warn", priority = -1 }` plus the allows
above. With `-D warnings` on the CLI, the remaining warnings
promote to errors and the job exits non-zero on regression.

## CI change

```diff
 clippy-strict:
   name: clippy (strict)
   runs-on: ubuntu-latest
-  continue-on-error: true
   …
-    cargo clippy --workspace --all-targets -- \
-      -D warnings \
-      -W clippy::pedantic \
-      -A clippy::missing_errors_doc \
-      …
+    cargo clippy --workspace --all-targets -- -D warnings
```

The per-lint allow flags moved into `Cargo.toml` where they
participate in `cargo metadata` and IDE clippy runs.

## Tightening process

To re-enforce a lint we currently allow, delete its line from
`[workspace.lints.clippy]` in the root `Cargo.toml`, fix the
surfaced sites, and push. CI will catch regressions on the next
PR.
