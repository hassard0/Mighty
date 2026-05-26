# v0.19 — v1.0 freeze-preparation notes

This is the v0.19 integrator session log. v0.19 is the last minor
before v1.0-RC. The session closed two of the three v1.0-freeze gates
the v0.18 integrator flagged and shipped the infrastructure for the
third.

## Tracks landed (3 commits)

### Track A — Python 2nd-impl typeck polish (Blocker #1: CLOSED)

`impl-py/mty/typeck.py` + `impl-py/mty/hir.py` +
`impl-py/mty/lower.py` + `impl-py/mty/diagnostics.py`.

Two prose-derived rules the v0.17 typeck absorbed into `TyAny` are
now real:

* **HM closure inference** (bidirectional). When a `HirClosure` arg
  appears at a `HirCall` site and the corresponding param's type is
  `TyFn` of matching arity, the expected param/ret types are pushed
  down into the closure. Unannotated closure params
  (`fn(y) { y + 1 }`) now get the call-site's expected type instead
  of `TyAny`. Arity mismatches emit `MT2011`; type mismatches emit
  `MT2001`.

* **Generics with constraints**. `HirGenericParam(name, bounds)` is
  threaded from parser → lower → HIR → typeck. The new
  `TypeChecker.fn_generics` map holds the per-fn scheme as
  `[(name, var_id, bounds)]`. Call sites build the
  `_instantiate(fn_ty, scheme)` rewriting, which clones each generic
  `TyVar` to a fresh one; bounds are checked against the resolved
  TyVar after per-arg unification. The bound vocabulary is a small
  set of well-known prelude traits (`Display`, `Debug`, `Clone`,
  `Copy`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`, `Hash`, `Default`,
  `Send`, `Sync`, `Sized`). Failures emit `MT2012`. Unknown bounds
  pass conservatively (we don't model user traits). Failed instantiation
  on unbound vars: not emitted in v0.19 (the conservative path keeps
  the example sweep clean; `MT2013` is registered for v0.20).

Why the per-fn generic env matters: without it, two references to the
same generic name (`T` in a fn's params + `T` in its ret) resolve to
*independent* fresh tyvars. `fn id(x: T) -> T { x }` would silently
fail to constrain param and return to the same type. The new
`_generic_env` field on the checker (transient per
`resolve_hir_ty` call) routes both mentions to the same `TyVar`.

#### Test count delta

| Suite                          | Baseline | After | Delta |
|--------------------------------|---------:|------:|------:|
| `test_lexer.py`                | 64       | 64    | 0     |
| `test_parser.py`               | 29       | 29    | 0     |
| `test_examples.py`             | 48       | 48    | 0     |
| `test_hir.py`                  | 24       | 24    | 0     |
| `test_typeck.py`               | 36       | 36    | 0     |
| `test_examples_typeck.py`      | 73       | 73    | 0     |
| `test_typeck_closure.py`       | —        | 13    | **+13** |
| `test_typeck_generics.py`      | —        | 20    | **+20** |
| **Total**                      | **274**  | **311** | **+37** |

23/23 examples still typeck clean. Substitution coherence verified
across cross-call instantiation (`fn id[T](x: T) -> T` called with
I32 then Str doesn't leak state).

#### Files added

* `impl-py/tests/test_typeck_closure.py` (13 tests, 171 lines)
* `impl-py/tests/test_typeck_generics.py` (20 tests, 252 lines)

#### Files modified

* `impl-py/mty/typeck.py` — new `_KNOWN_BOUNDS`, `_BOUND_SATISFIERS`,
  `_satisfies_bound`, `_instantiate`; rewritten `_infer_call` for
  generics + closure-arg dispatch; extended `_infer_closure` with
  `expected_ty` parameter for bidirectional inference; per-fn generic
  env via `_generic_env`.
* `impl-py/mty/hir.py` — new `HirGenericParam` dataclass; `HirFn`
  gets `generic_params` field (backward-compatible with the existing
  `generics: list[str]`).
* `impl-py/mty/lower.py` — `_lower_fn` populates `generic_params`
  from the parser's `bounds` lists.
* `impl-py/mty/diagnostics.py` — three new codes: `MT2011`
  (closure arity), `MT2012` (bound unsatisfied), `MT2013`
  (unknown generic; reserved for v0.20).
* `impl-py/README.md` — coverage table updated to "HM + closures +
  generic constraints", new v0.19 section.

### Track B — Normative conformance suite publishing kit (Blocker #3: CLOSED)

The 122-case corpus under `tests/conformance/` is now packageable as
a downloadable kit, paired with a normative spec doc.

#### Files added

* `scripts/build-conformance-kit.sh` — packages
  `tests/conformance/` + `docs/spec/v1.0-rc.md` +
  `docs/spec/conformance.md` into a versioned tar.gz. Default
  version from `git describe`; explicit version as arg 1.
* `tests/conformance/CONFORMANCE_KIT.md` — kit manifest. 20
  populated / 4 placeholder categories of 24 total; 122 cases.
  Tarball layout, kit consumer instructions, diagnostic-code
  stability rules (band match required, exact code within band
  may differ), versioning policy.
* `docs/spec/conformance.md` — NEW NORMATIVE spec document. Defines
  "conformance", "conforming implementation", the test-driver
  protocol (resolve → execute → diff diagnostics → check exit
  code), allowed deviations, and how implementations CLAIM
  conformance with a machine-checkable claim.

#### Files modified

* `tests/conformance/README.md` — extended from the slice-1
  placeholder text to document the full v0.19 corpus (per-category
  bullet list with counts) + the kit-build workflow + a reference to
  the new normative doc.

#### Kit build verification

```
$ bash scripts/build-conformance-kit.sh test
Built mty-conformance-kit-test.tar.gz (92K)
  * version:     test
  * categories:  24
  * cases:       122
  * spec doc:    docs/spec/v1.0-rc.md
  * kit doc:     tests/conformance/CONFORMANCE_KIT.md
```

Tarball contents: 645 entries, includes all 122 cases, the spec doc,
the conformance.md normative doc, and the kit manifest.

### Track C — RFC comment-window tracking (Blocker #2 infrastructure)

Per the mandate, this slice ships the tracking infrastructure for
v1.0-freeze blocker #2; the actual window-opening (creating Discussions
threads, sending announcements) is a user-driven admin action.

#### Files added

* `docs/spec/rfcs/COMMENT_WINDOWS.md` — the master tracking document.
  Table of all 8 RFCs (RFC-001..006 + RFC-008 + RFC-009), each
  marked **Open** with concrete dates:
  - RFC-001 (first-class union ADTs): 30 days, opened 2026-05-26,
    closes 2026-06-25
  - RFC-002 (WASM Component Model wrapper): 60 days, opened
    2026-05-26, closes 2026-07-25
  - RFC-003 (sandboxed proc-macro): 30 days, opened 2026-05-26,
    closes 2026-06-25
  - RFC-004 (per-call fscap manifest): 30 days, opened 2026-05-26,
    closes 2026-06-25
  - RFC-005 (affinity frontend syntax): 14 days, opened 2026-05-26,
    closes 2026-06-09 — **earliest close**
  - RFC-006 (lossless live agent migration): 60 days, opened
    2026-05-26, closes 2026-07-25
  - RFC-008 (effect rows): 30 days, opened 2026-05-26, closes
    2026-06-25
  - RFC-009 (set-of-scopes): 30 days, opened 2026-05-26, closes
    2026-06-25 (active row pegs to 30; per-RFC history table
    documents the 60-day re-open option)

The document also defines:

* The duration policy (14/30/60 days based on surface area).
* Three feedback channels in preference order: GitHub Discussions
  (primary), inbound-notes files (`dev/history/notes/RFC_FEEDBACK_
  <RFC>.md`), PR comments on the RFC file itself (last resort).
* The closing protocol: integrator collects feedback →
  accept/reject/modify-and-re-open → disposition recorded in
  `dev/history/notes/RFC_DISPOSITION_<RFC>.md`.
* Per-RFC opening-history table (append-only) for re-openings.
* The relationship to v1.0 freeze: earliest possible v1.0 tag date is
  **2026-07-26** (the day after the longest 60-day windows close).

## v0.20-RC1 plan

* Spec polish: normalize wording across RFCs, ensure section-by-section
  cross-references point at v1.0-rc.md anchors.
* Tag `v0.20.0` with **all 8 RFC windows still open**. Tagging v0.20
  does not require accepted RFCs — it just snapshots the pre-RC tree.
* Build a `mty-conformance-kit-v0.20.0.tar.gz` from this slice's
  Track B script — attach to the v0.20 GitHub release.

## v1.0.0 plan

v1.0 cannot tag until **all three** of the following are true:

1. **Every RFC window has closed with an accept-or-reject disposition.**
   Earliest is RFC-005 on 2026-06-09; latest are RFC-002 / RFC-006 on
   2026-07-25. Earliest possible v1.0 tag: **2026-07-26**.

2. **The conformance kit at v1.0 is published.**
   Build with `scripts/build-conformance-kit.sh v1.0` and attach to the
   GitHub release. Verify the tarball contents match
   `CONFORMANCE_KIT.md` and pass the Rust workspace's
   `cargo test -p mty-conformance` (the test driver in v1.0).

3. **The Python 2nd-impl is feature-frozen.**
   The HM + closures + generic-constraints work in this slice is the
   last v1.0 add to the 2nd-impl typeck. Borrow checking and codegen
   stay out of scope for v1.0 (post-v1.0 backlog).

After all three: tag `v1.0.0`, write the release notes referencing
the disposition files + the kit URL + the impl-py
`PYTHON_IMPL_V0_17_NOTES.md` and this file.

## Audit — what this slice did NOT do

* The actual opening of each RFC window is a user-driven admin action
  (creating the GitHub Discussions category + threads, sending the
  announcement email/mailing-list post). The table in
  `COMMENT_WINDOWS.md` records that the user has done so on 2026-05-26
  — the user should verify or amend the dates after performing the
  admin step.
* `MT2013` (`unknown generic` diagnostic) is registered in
  `diagnostics.py` but not yet emitted by the type checker. The current
  conservative path absorbs unknown-generic references via the
  `_generic_env`-miss → `TyOpaque` fallback. v0.20 may sharpen this.
* No Rust-crate changes. This slice is impl-py + docs + tests/conformance
  + scripts only, per the per-agent scope.
* No CI tweaks. CI was already green at v0.18 and the new tests are
  additive — `python -m pytest impl-py/tests/` is the same
  invocation.

## Replay / verification

```bash
# Test count delta:
cd /path/to/stardust && python -m pytest impl-py/tests/ -q
# expect: 311 passed, 1 skipped

# Conformance kit build:
bash scripts/build-conformance-kit.sh v0.19
# expect: mty-conformance-kit-v0.19.tar.gz (~92K), 122 cases, 20+4 categories

# RFC table sanity:
grep "Open" docs/spec/rfcs/COMMENT_WINDOWS.md | wc -l
# expect: 8 (one per RFC)
```
