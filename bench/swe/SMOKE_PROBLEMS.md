# SWE-bench Verified — 10-Problem Smoke Subset

The Mighty smoke run targets a **fixed, hand-picked subset** of 10
problems from [SWE-bench
Verified](https://huggingface.co/datasets/princeton-nlp/SWE-bench_Verified)
(`commit 7a1ddb6` of the dataset, pinned in
[`src/dataset.rs`](src/dataset.rs)).

The point of fixing the subset is **comparability**. Every run of
`make bench-smoke` exercises the same 10 problems against the same
dataset snapshot, so the pass / fail rate is directly comparable
across Mighty versions, models, and prompt iterations. Bumping the
subset bumps `SMOKE_SUBSET_VERSION`; older published results stay
distinguishable.

This page is the per-problem rationale. The published numbers live
in
[`dev/history/benchmarks/swe-bench-smoke-v0.30.md`](../../dev/history/benchmarks/swe-bench-smoke-v0.30.md).

## Selection criteria

Five rules constrain which problems can join the subset.

1. **Diversity of repo.** Seven distinct upstream projects across
   the ten instances. No single repo dominates more than two
   slots. The 12 repos in Verified span very different idioms
   (Django ORM internals vs. NumPy array semantics vs. SymPy
   symbolic algebra); a smoke that only tested one would tell us
   nothing about cross-domain robustness.
2. **Diversity of difficulty.** 5 easy / 3 medium / 2 hard based on
   SWE-bench Verified's `difficulty` annotation. A 10-easy smoke
   would over-state Mighty's adoption-readiness; a 10-hard smoke
   would produce a 0/10 that says nothing about the framework.
   The 5/3/2 mix forces the agent through multi-file edits, cross-
   module reasoning, and subtle semantic regressions.
3. **Bounded blast radius.** Every problem's failing tests run in
   under 90s on a laptop. No full-repo `pytest` runs. This is what
   keeps the smoke under 40 minutes wall-clock total — every
   second the scorer adds is a second the agent doesn't have for
   the next turn.
4. **No network mocks.** Every problem is solvable from a frozen
   git checkout. No `pip install`-of-unpinned-deps mid-run; no
   calls to live third-party services. The dataset row caches into
   `data/instances/` on first fetch; subsequent runs are
   network-free.
5. **Stable across dataset versions.** Every instance was present
   in both SWE-bench Verified v1.0 and the current pinned
   snapshot. When the dataset re-curates, we don't lose the
   ability to re-run prior published numbers from the same SHA.

## The ten

| # | Instance ID | Repo | Difficulty | Why picked |
|---|---|---|---|---|
| 1 | `sympy__sympy-20590` | sympy/sympy | easy | Single-file fix; tight failing-test diff. Exercises symbolic math reasoning. |
| 2 | `django__django-11999` | django/django | easy | Field renaming bug, one-line patch. Exercises ORM model introspection. |
| 3 | `astropy__astropy-14365` | astropy/astropy | easy | QDP-format reader regression, focused. Exercises file-format parsing. |
| 4 | `scikit-learn__scikit-learn-13779` | scikit-learn/scikit-learn | easy | `VotingClassifier` sample-weight None bug. Exercises numerical-API edge cases. |
| 5 | `pytest-dev__pytest-7373` | pytest-dev/pytest | easy | Mark-expression caching, well-scoped. Exercises test-discovery internals. |
| 6 | `matplotlib__matplotlib-23476` | matplotlib/matplotlib | medium | Figure DPI doubled on unpickle — multi-file. Exercises serialisation round-trips. |
| 7 | `psf__requests-1142` | psf/requests | medium | Re-redirect with method preserved. Exercises HTTP-protocol corner cases. |
| 8 | `pallets__flask-4045` | pallets/flask | medium | Blueprint name validator. Exercises framework-level invariants. |
| 9 | `django__django-13447` | django/django | hard | Admin `app_list` serialisation overhaul. Exercises multi-file refactor reasoning. |
| 10 | `sympy__sympy-13971` | sympy/sympy | hard | LaTeX printing of indexed bases. Exercises subtle precedence rules. |

Difficulty bucketing follows the SWE-bench Verified labels; the
instance IDs match those in `princeton-nlp/SWE-bench_Verified`
exactly so leaderboard cross-references work.

## What the difficulty buckets force

Each bucket exercises different agent failure modes:

| Bucket | What the agent must do | Common failure |
|---|---|---|
| Easy | Read one file, locate the bug, write a 1–10 line patch. | Misidentifies the function; patches the wrong call site. |
| Medium | Read 2–5 files, reason across modules, write a multi-file patch. | Multi-file edits that compile but break a `PASS_TO_PASS` test. |
| Hard | Refactor 5+ files to fix the underlying invariant. | Over-edits; can't keep the patch tight; runs out of turns. |

If Mighty passes all five easies and zero medium/hards, that's a
**publishable, honest "easy: 5/5; medium-hard: 0/5"** result — and
a clear v0.31 roadmap (focus on cross-file reasoning).

If Mighty passes all ten, we celebrate, then quietly worry that
the curated subset is too easy and re-curate for v0.31.

## Updating the subset

Change requests to the list go through `dev/history/notes/` with a
paragraph explaining why. Once the user accepts, bump
`SMOKE_SUBSET_VERSION` in
[`src/problems.rs`](src/problems.rs) so prior results stay
distinguishable. Each version's published page lives at
`dev/history/benchmarks/swe-bench-smoke-v0.X.md` — never overwritten.

## Why not just publish the full 500-problem number?

That's `make bench-full`, gated to v0.31. Three reasons the smoke
matters even after the full run lands:

1. **Inner-loop iteration.** Tuning the prompt, swapping a tool,
   bumping a model — the full run is 5–10 hours and $300+. The
   smoke turns that into 30 minutes and $15, with the same
   per-difficulty mix.
2. **Regression coverage on every release.** Running the smoke on
   every v0.X tag catches regressions cheaply. The full set is too
   expensive to re-run on every release boundary.
3. **Reproducibility against pinned data.** The full Verified set
   re-curates over time; the smoke subset is pinned by SHA so the
   `v0.30.md` → `v0.31.md` comparison is apples-to-apples.

The full run answers "how does Mighty stack up against the public
leaderboard." The smoke answers "did anything regress in this
release."
