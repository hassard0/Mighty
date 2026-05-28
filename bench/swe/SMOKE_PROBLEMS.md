# SWE-bench Verified — 10-Problem Smoke Subset

The Mighty smoke run targets a **fixed, hand-picked subset** of 10
problems from [SWE-bench Verified](https://huggingface.co/datasets/princeton-nlp/SWE-bench_Verified)
(`commit 7a1ddb6` of the dataset, pinned in `src/dataset.rs`).

The point of fixing the subset is **comparability**: every run of
`make bench-smoke` exercises the same 10 problems against the same
dataset snapshot, so the pass/fail rate is directly comparable
across Mighty versions, models, and prompt iterations.

## Selection criteria

* **Diversity of repo**: 7 different upstream projects (no single
  repo dominates).
* **Diversity of difficulty**: 5 easy / 3 medium / 2 hard based on
  SWE-bench Verified's `difficulty` annotation.
* **Bounded blast radius**: every problem's failing tests run in
  under 90s on a laptop (no full-repo `pytest` runs).
* **No network mocks**: every problem is solvable from a frozen git
  checkout — no `pip install`-of-unpinned-deps, no calls to live
  third-party services.
* **Stable across dataset versions**: each instance was present in
  both SWE-bench Verified `v1.0` and the current pinned snapshot.

## The 10

| # | Instance ID | Repo | Difficulty | Why picked |
|---|---|---|---|---|
| 1 | `sympy__sympy-20590`            | sympy/sympy                | easy   | Single-file fix; tight failing-test diff. |
| 2 | `django__django-11999`          | django/django              | easy   | Field renaming bug, one-line patch. |
| 3 | `astropy__astropy-14365`        | astropy/astropy            | easy   | QDP-format reader regression, focused. |
| 4 | `scikit-learn__scikit-learn-13779` | scikit-learn/scikit-learn | easy   | `VotingClassifier` sample-weight None bug. |
| 5 | `pytest-dev__pytest-7373`       | pytest-dev/pytest          | easy   | Mark-expression caching, well-scoped. |
| 6 | `matplotlib__matplotlib-23476`  | matplotlib/matplotlib      | medium | Figure DPI doubled on unpickle — multi-file. |
| 7 | `requests__requests-1142`       | psf/requests               | medium | Re-redirect with method preserved. |
| 8 | `flask__flask-4045`             | pallets/flask              | medium | Blueprint name validator. |
| 9 | `django__django-13447`          | django/django              | hard   | Admin app_list serialisation overhaul. |
| 10 | `sympy__sympy-13971`           | sympy/sympy                | hard   | Latex printing of indexed bases. |

(Difficulty bucketing follows the SWE-bench Verified labels; instance IDs
match those in `princeton-nlp/SWE-bench_Verified`.)

## Why not just pick the 10 easiest?

A 10-easy smoke would over-state Mighty's adoption-readiness. The
3-medium-2-hard mix forces the agent through:

* **Multi-file edits** (#6, #9).
* **Cross-module reasoning** (#9, #10).
* **Test-discovery edge cases** (#5, #7).
* **Subtle semantic regressions** that look right but break behaviour
  (#1, #6).

If Mighty passes all five easies and zero medium/hards, that's a
**publishable, honest** "easy: 5/5; medium-hard: 0/5" — and a clear
v0.31 roadmap.

## Updating the subset

Change requests to the list should go through `dev/history/notes/`
with a paragraph explaining why. Once the user accepts, bump the
`SMOKE_SUBSET_VERSION` constant in `src/problems.rs` so prior
results stay distinguishable.
