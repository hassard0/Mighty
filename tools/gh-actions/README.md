# Mighty GitHub Actions

A reusable composite-action library that lets any project drop
[Mighty](../../README.md) into GitHub Actions in **three lines**:

```yaml
- uses: hassard0/Mighty/tools/gh-actions/setup-mty@v0.31.0
  with:
    version: "0.31.0"
- uses: hassard0/Mighty/tools/gh-actions/mty-check@v0.31.0
- uses: hassard0/Mighty/tools/gh-actions/mty-test-eval@v0.31.0
  with:
    replay-only: true
```

That's it. Your job now has the `mty` binary on `$PATH`, has run
`mty check` over every `.mty` file with the upstream
`@typeck-pending` / `@compile-error` discipline, and has replayed
every recorded `*.eval.mty` cell — all without a single API key.

## The actions

| Action                                          | One-liner                                                                |
| ----------------------------------------------- | ------------------------------------------------------------------------ |
| [`setup-mty`](./setup-mty)                      | Download + verify + install the `mty` binary for the runner OS.          |
| [`mty-check`](./mty-check)                      | Run `mty check` (and `mty fmt --check`) over a glob of `.mty` files.     |
| [`mty-test`](./mty-test)                        | Run the Mighty unit-test runner (`mty test`).                            |
| [`mty-test-eval`](./mty-test-eval)              | Run LLM-eval suites (`mty test --eval`); default `--replay-only=true`.   |
| [`mty-bench-smoke`](./mty-bench-smoke)          | Run the 10-problem SWE-bench Verified smoke; gated on an Anthropic key.  |

Every action is a [composite
action](https://docs.github.com/en/actions/creating-actions/creating-a-composite-action) —
no Docker image, no Node toolchain, no proprietary marketplace listing
to grant access to. They just shell out to `mty` (and `cargo` for the
bench harness).

## Subpath gotcha — read this once

Composite actions normally live at the **root** of a repository. These
actions live in a **subpath** (`tools/gh-actions/<name>/`). That's a
deliberate trade-off: it keeps the action sources adjacent to the
`mty` source they target, so a release-branch tag pins both at once.

GitHub Actions supports calling subpath actions from external repos
using the `<owner>/<repo>/<path>@<ref>` form:

```yaml
- uses: hassard0/Mighty/tools/gh-actions/setup-mty@v0.31.0
```

Include the `tools/gh-actions/<name>` segment **every time** —
omitting it (`hassard0/Mighty@v0.31.0`) won't find the action.

## Version pinning — read this twice

**Always pin to a release tag**, never to `@main`:

```yaml
# good — reproducible, won't break when the next version lands
- uses: hassard0/Mighty/tools/gh-actions/setup-mty@v0.31.0

# bad — silently picks up breaking changes
- uses: hassard0/Mighty/tools/gh-actions/setup-mty@main
```

Action inputs (binary version, default flags, file shapes) are
considered part of the public Mighty release surface — a major-version
bump may change them. Pin the action ref AND the
`setup-mty.with.version` to the same `v0.X.Y`.

## Examples

Copy-paste-ready workflows live in [`examples/`](./examples):

| Workflow                                       | Use case                                                          |
| ---------------------------------------------- | ----------------------------------------------------------------- |
| [`basic-check.yml`](./examples/basic-check.yml) | Minimal: install + `mty check` + `mty fmt --check`.               |
| [`full-ci.yml`](./examples/full-ci.yml)         | Recommended PR gate: check + unit tests + replay-only eval.       |
| [`nightly-eval.yml`](./examples/nightly-eval.yml) | Daily real-LLM eval + SWE-bench smoke; gated on a secret.       |

Drop one into `.github/workflows/` and edit the `version:` to match
the Mighty release you're targeting.

## Authoring conventions

If you're hacking on these actions:

- **YAML**: 2-space indent, LF line endings, double-quoted strings.
- **Inputs**: lowercase-kebab-case, every input documented even when
  defaulted, every `default:` quoted so YAML treats it as a string.
- **Shell**: `bash` with `set -euo pipefail`. Use `${{ inputs.foo }}`
  *inside* `env:` blocks, not directly inside `run:` — that keeps
  shell injection at bay.
- **No Docker**, **no Node**: composite-action only. The marketing
  promise is "drop-in, no extra tools" — uphold it.
- **Mirror upstream discipline**: when `.github/workflows/ci.yml`
  gains a new sweep convention (e.g. v0.30's `@compile-error`
  marker), reflect it in the composite actions the same release.

## v0.32 follow-ups

These aren't blockers for shipping the v0.31 library, but they're the
obvious next moves:

- **PR auto-comment with cost delta** — surface `bench-smoke`'s
  `$5-$20` cost line as a sticky PR comment so reviewers see the
  spend before approving the merge.
- **`mty-explain` action** — wrap `mty explain MTxxxx` so a CI failure
  can link directly to the diagnostic's prose.
- **Native binary cache** — drop the workaround `tar.gz` → manual
  extract step in `setup-mty` if upstream `release.yml` switches to a
  flat-layout archive (no nested `mty-v<version>/` directory).
- **Add `arm64` Linux + `x86_64` macOS** when upstream `release.yml`
  starts shipping those targets again (Intel macOS dropped in
  v0.18 — see `release.yml` comment).
- **Auto-pin via `dependabot.yml`** — ship a tiny dependabot config
  example so consumers' action refs bump on every Mighty release.

## Where this fits

These actions are the **adoption** surface, not the **development**
surface. Mighty's own CI sits in `.github/workflows/` (one folder
up) and runs `cargo test`, `cargo clippy`, the example sweep, the
demo smoke harness, the conformance kit build, and so on. The
composite actions here let *consumers* of Mighty get the same
discipline without copying that workflow.

For a deeper tour of Mighty itself, see the [top-level
README](../../README.md) and the [tour](../../docs/tour.md).
