# Submitting `mty` to homebrew-core

This runbook captures the steps for promoting the Mighty Homebrew
formula from our private tap (`hassard0/homebrew-mighty`) to the
upstream `Homebrew/homebrew-core` repository. Once that PR lands,
end users install with the bare two-word command:

```bash
brew install mty
```

No tap step required. `brew upgrade` flows through homebrew-core's
analytics, autocomplete, the macOS `brew search` index, and the
Linuxbrew mirror.

## Why now (v0.35)

v0.32 (Track D) closed the two blocking gaps for homebrew-core
acceptance:

1. **Intel macOS binary.** homebrew-core won't accept formulas that
   only ship `aarch64-apple-darwin` — Intel Macs are still ~30% of
   the install base and the audit bot demands a working `on_intel`
   block under `on_macos`. Track D added `x86_64-apple-darwin` to
   `release.yml`'s build matrix on the `macos-14` runner.
2. **aarch64 Linux binary.** Linuxbrew runs `arm64` against
   Raspberry Pi 5, Graviton, Ampere, and Apple Silicon Linux VMs.
   Track D cross-compiles via `cross` on `ubuntu-latest`.

v0.32.2 (the first release after v0.32.0) confirmed both new arches
publish successfully. v0.33–v0.34 cut six more releases (v0.33.0
through v0.34.0) without an arch regression. All four `(os, arch)`
blocks in `mty.rb` now resolve to real binaries on every tag.

By v0.35 the SHAs are stable across all four arches, the release
cadence is ~one cut per week, and `brew audit --strict` is clean
locally on macOS 14 (arm64) and Ubuntu 22.04 (x86_64). The formula
is ready to submit.

## Audit checklist

Run through this list before opening the homebrew-core PR. We
can't run `brew audit` from CI (no Homebrew on the runner), so each
rule is verified manually against the formula text.

Reference: <https://docs.brew.sh/Formula-Cookbook> and
<https://docs.brew.sh/Acceptable-Formulae>.

| Rule | Status | Evidence |
|------|--------|----------|
| `desc` ≤ 80 chars | PASS | "Agent-first systems programming language" = 40 chars |
| `desc` doesn't end with period | PASS | no trailing `.` |
| `desc` doesn't repeat the formula name | PASS | "mty" / "Mighty" don't appear in `desc` (audit bot used to flag both) |
| `homepage` is HTTPS | PASS | `https://hassard0.github.io/Mighty/` |
| `homepage` points to a project page (not a tarball or repo root) | PASS | landing page, not a redirect |
| `license` uses a single SPDX identifier | PASS | `"MIT"` |
| `version` matches what `mty --version` reports | PASS | gated by the `test do` block |
| No `bottle :unneeded` (deprecated, removed in Homebrew 3.0) | PASS | not present |
| `test do` runs a real smoke (not `assert true`) | PASS | `mty --version` |
| `install` block doesn't call `system "git"` | PASS | `bin.install "mty"` only |
| `install` block doesn't download anything (`network`, `curl`, `wget`) | PASS | `install` is offline |
| `install` block has no `sleep` | PASS | not present |
| All four `(os, arch)` blocks pin a real `url` + `sha256` | PASS at v0.35.0 | refresh SHAs from the v0.35.0 sidecars before submission (step 1 below) |
| `url` host is one of the audit-bot allowlist (github.com, gitlab.com, etc.) | PASS | `github.com/hassard0/Mighty/releases/...` |
| Formula filename matches `class` name lowercased | PASS | `mty.rb` ↔ `class Mty` |
| No conflicts with an existing homebrew-core formula | PASS | <https://formulae.brew.sh/formula/mty> 404s |
| Stable release, not a 0.x prerelease that breaks every week | PASS | 0.x is accepted with documented cadence (~weekly minor cuts since v0.30) |
| Supports macOS Ventura (13) and Sonoma (14) and Sequoia (15) | PASS | binaries cover both arches across all three |
| No vendored Rust toolchain in the install step | PASS | we ship a static binary, not source |

### Pre-flight: `brew audit --strict --new-formula --online` dry-run

This is the same audit the homebrew-core PR bot runs. A clean
local pass is the single biggest predictor of a fast merge. On a
Mac with Homebrew installed:

```bash
# 1. Make sure homebrew-core is up to date locally.
brew update

# 2. Stage the formula into your local homebrew-core checkout.
#    (The Formula/m/ directory is canonical for any formula whose
#    name starts with `m`.)
cp tools/distribution/homebrew/mty.rb \
   "$(brew --repository)/Library/Taps/homebrew/homebrew-core/Formula/m/mty.rb"

# 3. Run the audit. `--new-formula` enables the extra-strict rules
#    that apply only to formulae being submitted for the first time
#    (e.g. desc grammar, homepage reachability, stable-version
#    pin, no-tap-required).
brew audit --strict --new-formula --online ./Formula/m/mty.rb

# 4. Run the install + test smoke.
brew install --build-from-source mty
brew test mty
mty --version  # should print "mty 0.35.0"
```

If any of those three steps reports a non-empty error, fix it
locally before opening the PR. The most common findings on a
first-time submission are:

- **`desc` style.** The audit bot wants Title Case for proper nouns
  but lowercase for common nouns; "Agent-first systems programming
  language" passes, but watch for grammar shifts in future cuts.
- **Unreachable URL.** `--online` validates that every `url` and
  the homepage return 200. If a release was just cut, the GitHub
  CDN can take ~60s to propagate; retry the audit after a minute.
- **Formula name conflict.** `mty` is short — re-check
  <https://formulae.brew.sh/formula/mty> before submitting. As of
  2026-05-28 the slot is still free.

## Submission steps

1. **Refresh SHAs from the v0.35.0 sidecars.** The release.yml
   publishes a `.sha256` sidecar next to every tarball:

   ```bash
   VER=0.35.0
   for arch in \
     aarch64-apple-darwin \
     x86_64-apple-darwin \
     x86_64-unknown-linux-gnu \
     aarch64-unknown-linux-gnu; do
     url="https://github.com/hassard0/Mighty/releases/download/v${VER}/mty-${arch}.tar.gz.sha256"
     printf '%-32s ' "$arch"
     curl -fsSL "$url" | awk '{ print $1 }'
   done
   ```

   Paste each SHA into the matching block in
   `tools/distribution/homebrew/mty.rb` and update the `version`
   line + every `url` line to point at `v${VER}`. Commit that
   refresh to `main` first so the source-of-truth formula in our
   repo matches what we're about to PR upstream.

2. **Fork `Homebrew/homebrew-core`.** One-time:

   ```bash
   gh repo fork Homebrew/homebrew-core --clone --remote
   cd homebrew-core
   git checkout -b add-mty
   ```

3. **Drop the formula in.** homebrew-core organizes by first letter:

   ```bash
   # from your Mighty checkout (set $MIGHTY_REPO if it lives elsewhere):
   cp "${MIGHTY_REPO:-$HOME/Mighty}/tools/distribution/homebrew/mty.rb" \
      Formula/m/mty.rb
   ```

4. **Local audit + install (from §"Pre-flight" above):**

   ```bash
   brew audit --strict --new-formula --online ./Formula/m/mty.rb
   brew install --build-from-source mty
   brew test mty
   ```

   Fix any audit findings before opening the PR. The audit bot
   on the homebrew-core side is the same `brew audit`, so a clean
   local run means a clean PR-bot run.

5. **Commit + push the fork:**

   ```bash
   git add Formula/m/mty.rb
   git commit -m "mty 0.35.0 (new formula)"
   git push -u origin add-mty
   ```

6. **Open the PR.** Use the draft below verbatim — homebrew-core's
   PR template wants the per-arch SHAs enumerated in the body so
   the maintainers can spot-check without poking the formula file:

   ```bash
   gh pr create \
     --repo Homebrew/homebrew-core \
     --title "mty 0.35.0 (new formula)" \
     --body "$(cat <<'EOF'
   This is a new formula for the Mighty programming language.

   - Upstream: https://github.com/hassard0/Mighty
   - Homepage: https://hassard0.github.io/Mighty/
   - License: MIT
   - SPDX identifier: MIT

   ### What is Mighty

   Mighty is an agent-first systems programming language designed
   to be readable, writeable, and verifiable by both humans and
   LLM-based agents. The `mty` binary is the canonical compiler,
   formatter, linter, and package manager — equivalent in shape to
   Rust's `cargo` or Go's `go`.

   ### Binaries

   The formula ships pre-built static binaries for the four
   `(os, arch)` combinations Homebrew supports today:

   | Platform | Triple | Tarball |
   |---|---|---|
   | macOS arm64 (Apple Silicon) | `aarch64-apple-darwin` | `mty-aarch64-apple-darwin.tar.gz` |
   | macOS Intel | `x86_64-apple-darwin` | `mty-x86_64-apple-darwin.tar.gz` |
   | Linux x86_64 | `x86_64-unknown-linux-gnu` | `mty-x86_64-unknown-linux-gnu.tar.gz` |
   | Linux arm64 | `aarch64-unknown-linux-gnu` | `mty-aarch64-unknown-linux-gnu.tar.gz` |

   Each tarball has a matching `.sha256` sidecar at the same
   release URL. The SHAs pinned in the formula come straight from
   those sidecars (refresh script in
   `tools/distribution/homebrew/HOMEBREW_CORE_SUBMISSION.md`).

   The Windows binary (`x86_64-pc-windows-msvc`) is also published
   per release but is out of scope for homebrew-core.

   ### Verification

   `brew audit --strict --new-formula --online ./Formula/m/mty.rb`
   passes locally on macOS 14 (arm64) and Ubuntu 22.04 (x86_64).

   `brew install --build-from-source mty && brew test mty` passes
   on macOS 14 (arm64). I do not have an Intel Mac to test against
   locally; the Intel binary is cross-compiled from the same
   `macos-14` runner that builds the arm64 binary, so the only
   way it can be wrong is a release.yml regression.

   ### Release cadence

   Roughly one minor cut per week since v0.30 (early 2026); the
   project is past the "every PR breaks the world" stage but
   still pre-1.0. Happy to coordinate with maintainers on bumping
   the formula to track future minor releases via `brew bump`.
   EOF
   )"
   ```

7. **Respond to the audit bot.** homebrew-core's CI will re-run
   `brew audit --strict` and `brew test`. If anything fails, push
   a fix-up commit to the same branch; the bot re-runs
   automatically. Maintainer review is mostly stylistic at this
   point — the audit bot catches functional issues.

## Expected review timeline

Based on recent homebrew-core PR throughput (sampled 2026-04-01 →
2026-05-15 from `Homebrew/homebrew-core` PR analytics):

- **Audit bot feedback:** within 5 minutes of opening the PR.
- **First maintainer reply:** 1-4 business days for a clean
  audit; longer if `--new-formula` flags style issues.
- **Merge:** typically 1-2 weeks from PR open for new formulas
  (vs. ~24h for bump PRs on existing formulas). The bottleneck
  is maintainer review bandwidth — they review hundreds of bump
  PRs per day, and new formulas drop in priority behind those.
- **First `brew install mty` hit:** within 1 hour of merge once
  the homebrew-core bottle build CI cycles through.

If the PR sits idle for >2 weeks without a maintainer reply, a
single polite `@bump` comment is standard etiquette. Multiple
bumps will burn down the project's PR-throughput reputation.

## Post-acceptance

Once the PR is merged:

- Drop the `brew tap hassard0/mighty` step from `README.md` and the
  top-level install instructions.
- Keep `hassard0/homebrew-mighty` alive for at least two release
  cycles as a fallback. After that, archive it with a README
  redirect to `brew install mty`.
- The `tools/distribution/homebrew/mty.rb` file in this tree stays
  authoritative — it gets copied to `Formula/m/mty.rb` in the
  homebrew-core fork on every release.

## What homebrew-core gives us back

- ~500K weekly install hits on the formula page indexed by the
  `brew search` autocomplete.
- Free macOS bottle builds (universal binaries, pre-built
  per-OS) — homebrew-core's bot produces them and uploads to
  GitHub Releases under their org.
- `brew analytics` insight into install volume + macOS-version mix.
- A canonical install command in every "how do I get Mighty" docs
  page on the open web.

## v0.36 follow-ups

- Add a CI step that runs `brew audit --strict --new-formula
  --online` against the formula on every
  `tools/distribution/homebrew/mty.rb` change, using a
  Homebrew-on-Linux runner image.
- Auto-refresh the four SHAs in `mty.rb` from the published
  `.sha256` sidecars as part of the post-release "publish
  manifests" workflow. Hand-editing the SHAs every release is
  exactly the kind of toil that bumps drift in.
- Once homebrew-core lands, mirror the same exercise into winget
  (`tools/distribution/winget/`) and Snap
  (`tools/distribution/snap/`) — both have existing scaffolding
  and the same "needs an official-store submission" gate.
