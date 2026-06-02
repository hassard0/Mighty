# Contributing to Mighty

Mighty is pre-alpha. The fastest way to contribute is to:

1. Pick an open issue, or file one describing what you want to do.
2. Fork the repo, branch off `main`.
3. Make your change with tests.
4. Run the full gate (see below).
5. Open a pull request.

## Code of Conduct

This project follows the
[Contributor Covenant 2.1](https://github.com/hassard0/Mighty/blob/main/CODE_OF_CONDUCT.md).
Be respectful.

## Building

```bash
cargo build --workspace
cargo test --workspace
```

Minimum Rust: **1.85**. The toolchain is pinned in
[`rust-toolchain.toml`](https://github.com/hassard0/Mighty/blob/main/rust-toolchain.toml).
The current pin is **1.95.0** (the latest stable at this slice's
release time); the MSRV gate in CI confirms the 1.85 floor.

## The pre-commit gate

Every PR must pass these commands:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

These exactly match what CI runs (see
[`.github/workflows/ci.yml`](https://github.com/hassard0/Mighty/blob/main/.github/workflows/ci.yml)).
Six CI jobs are required gates: `test` (cross-OS matrix),
`test-minimal`, `msrv`, `clippy-strict` (pedantic + `-D warnings`),
`bench`, `security` (`cargo audit --deny warnings`).

### Pre-push git hook (v0.34 T4, extended in v0.37 T1)

A pre-push hook lives at `.git-hooks/pre-push`. It mirrors the three
cheapest CI gates:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `mty fmt --check` on every `.mty` file under `examples/`,
   `demos/*/src/`, and `tools/gallery/examples/*/main.mty`
   (added in v0.37 T1; the first two checks were Rust-only and
   missed the `.mty` drift that caused the v0.36.1 retag cycle —
   two retags + two main-branch fixes in one release).

The hook is **REQUIRED for swarm agents** and strongly recommended
for human contributors. Install it once per clone:

```bash
mty hooks install
```

Verify:

```bash
mty hooks status
# -> mty hooks status: Mighty pre-push hook installed at ...
```

Re-running `mty hooks install` is idempotent — when the hook script
in `.git-hooks/pre-push` changes (e.g. a future track adds a fourth
gate), a plain `mty hooks install` overwrites the previously
installed copy without needing `--force`.

Bypass for one push (e.g. a docs-only branch where you know the
.mty surfaces are untouched):

```bash
git push --no-verify
```

Or skip the hook for the whole session:

```bash
MTY_PRE_PUSH_SKIP=1 git push
```

Uninstall: `mty hooks uninstall`.

### `scripts/test-like-gha.sh` (v0.45 T4)

`scripts/test-like-gha.sh` (and the equivalent `.ps1` for Windows
PowerShell) runs `cargo test --workspace` under the exact same
configuration the GitHub Actions runners use: `RUSTFLAGS=-D warnings`,
`CARGO_PROFILE_DEV_DEBUG=0` + `CARGO_PROFILE_TEST_DEBUG=0` (drops
debuginfo to keep Linux runners under their disk ceiling), and the
Windows leg routes to `cargo test --workspace -- --test-threads=1
--nocapture` so behaviour matches the Windows runner's serial-test
mode. `$CARGO_TARGET_DIR` is honored so a per-worktree target dir is
respected.

Use this script before pushing whenever your change touches codegen,
the test harness, or anything else that could expose a disk-pressure
or threading regression. The cautionary example is the **v0.42 T1
incident** (recapped in `RELEASE-v0.44.md`): the L28/L21 Vec fix was
"verified-fixed-and-locked" on Vulcan Linux and on local Windows, but
GHA Ubuntu SIGSEGV'd the regression tests because debuginfo-heavy
test binaries blew past the small ephemeral disk and produced
truncated artifacts the test harness then crashed on. v0.44 main CI
had to rerun two infra-failed jobs to clear the same trap. The CI-side
fix (drop debuginfo + Windows serial) ships from
`codex/ci-disk-headroom`; `scripts/test-like-gha.sh` is the local
mirror so swarm agents stop discovering that class of regression *on
the runner*.

Optional pre-push integration: set `MTY_PRE_PUSH_GHA=1` and the
pre-push hook will, after fmt + clippy + mty-fmt-check, invoke
`scripts/test-like-gha.sh`. Off by default to keep the normal push
fast; flip it on for the duration of a codegen-heavy track:

```bash
MTY_PRE_PUSH_GHA=1 git push
```

**REQUIRED for swarm agents.** The v0.27 / v0.30 / v0.32 / v0.33
retros all surfaced the same failure mode: a swarm agent pushed a
branch that passed local Windows checks but failed Linux-side
`cargo fmt --check` in CI, breaking the integrator's merge train.
v0.36.1 added a new wrinkle: even with the hook installed, tracks
pushed `.mty` drift that only `mty fmt --check` catches — the
Rust-only hook didn't see it. v0.37 T1 plugs that gap. Every
swarm-agent setup boilerplate MUST run `mty hooks install` right
after `git worktree add`, alongside `export CARGO_TARGET_DIR`. The
hook itself is a no-op when `cargo` isn't on PATH (so doc-only
worktrees still push cleanly).

For docs and example-only PRs, the same fmt + clippy + test gate
still runs (cargo-fast). The
[example-sweep job](https://github.com/hassard0/Mighty/blob/main/.github/workflows/ci.yml)
additionally runs `mty check` + `mty fmt --check` on every
`examples/*.mty` and asserts a clean exit — modulo the
`@compile-error` markers, which deliberately expect a specific
MT-code. The local hook covers a slightly different surface
(examples + demos + gallery) so a `.mty` change to any of them
trips the hook before it trips CI.

## Snapshot tests

The HIR dump and parser smoke tests use
[insta](https://docs.rs/insta) snapshots. If you intentionally
change the dump shape, regenerate them:

```bash
cargo insta review
```

Then commit the updated `.snap` files.

## Branch policy

Single-branch workflow: PRs land on `main`. Long-lived feature
branches are reserved for swarm coordination (multiple parallel
agent worktrees under an integrator) — see the swarm-discipline
notes in [`dev/history/`](https://github.com/hassard0/Mighty/tree/main/dev/history)
for the pattern. For typical contributor PRs, branch off `main`,
push, open the PR.

## Style

- Follow `rustfmt` defaults. The CI gate enforces it.
- Prefer named functions over deeply-nested closures.
- Public items get a doc comment. At minimum, one sentence.
- Errors get a stable
  [`DiagCode`](reference/diagnostics.md).
- No `unwrap`/`expect` in non-test code unless the invariant is
  local and documented inline.

## What kinds of changes are welcome right now

- Parser fixes for syntax already in the spec.
- New example programs that exercise spec corners.
- Documentation improvements (this docs tree, the inline
  rustdocs, example annotations, demo READMEs).
- New diagnostic codes with good error text.
- Tests — there are never enough.
- SWE-bench harness improvements (see
  [`bench/swe/README.md`](https://github.com/hassard0/Mighty/blob/main/bench/swe/README.md)).

## What kinds of changes need a design discussion first

- New language constructs not in the spec.
- IR shape changes.
- Anything that touches the build / release process.

File an issue describing the design before writing the code. For
large language-shape proposals, the
[RFC process](https://github.com/hassard0/Mighty/tree/main/docs/spec/rfcs)
is the right venue — eight comment windows are open as of the
v1.0 freeze prep.

## Swarm + integrator pattern

For larger multi-track changes, the project uses an integrator
pattern: independent agents (or contributors) work on isolated
worktrees off a shared branch; one integrator reviews each track
and merges into the branch in order. The discipline is documented
inline in the per-release `RELEASE-v0.X.md` files under
[`dev/history/releases/`](https://github.com/hassard0/Mighty/tree/main/dev/history/releases).

The contract: each track is a separately-mergeable unit with green
CI, clean clippy, and a passing smoke; the integrator is
responsible only for the cross-track merge order (typically
alphabetic) and any cross-cutting conflict resolution.

## Single-commit PRs

For most changes, squash to a single commit before review. The
commit message should explain the **why**, not just the **what**.
Reference the issue number.

## License

By contributing, you agree to release your contribution under the
MIT license, matching the project license.
