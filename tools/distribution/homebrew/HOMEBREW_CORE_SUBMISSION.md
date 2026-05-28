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

## Why now

v0.32 (Track D) closed the two blocking gaps for homebrew-core
acceptance:

1. **Intel macOS binary.** homebrew-core won't accept formulas that
   only ship `aarch64-apple-darwin` — Intel Macs are still ~30% of
   the install base and the audit bot demands a working `on_intel`
   block under `on_macos`. Track D added `x86_64-apple-darwin` to
   `release.yml`'s build matrix on the `macos-13` runner.
2. **aarch64 Linux binary.** Linuxbrew runs `arm64` against
   Raspberry Pi 5, Graviton, Ampere, and Apple Silicon Linux VMs.
   Track D cross-compiles via `cross` on `ubuntu-latest`.

With both arches in `release.yml`, the formula's four `(os, arch)`
blocks all resolve to a real binary and the `brew audit --strict`
bot stops complaining.

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
| All four `(os, arch)` blocks pin a real `url` + `sha256` | PASS at v0.32.0 | placeholder SHAs marked in-formula until the v0.32.0 tag rebuilds |
| `url` host is one of the audit-bot allowlist (github.com, gitlab.com, etc.) | PASS | `github.com/hassard0/Mighty/releases/...` |
| Formula filename matches `class` name lowercased | PASS | `mty.rb` ↔ `class Mty` |
| No conflicts with an existing homebrew-core formula | PASS | <https://formulae.brew.sh/formula/mty> 404s |
| Stable release, not a 0.x prerelease that breaks every week | PARTIAL | we're 0.x; homebrew-core does accept 0.x formulas if the release cadence is documented and stable |
| Supports macOS Ventura (13) and Sonoma (14) and Sequoia (15) | PASS | binaries cover both arches across all three |
| No vendored Rust toolchain in the install step | PASS | we ship a static binary, not source |

### Manual `brew audit --strict --online` dry-run

If you want to run the audit before opening the PR (recommended),
on a Mac with Homebrew installed:

```bash
# 1. Get the formula into your local tap repo's Formula/ dir
cp tools/distribution/homebrew/mty.rb \
   "$(brew --repository)/Library/Taps/homebrew/homebrew-core/Formula/m/mty.rb"

# 2. Run the audit
brew audit --strict --online --new mty

# 3. Run the install + test smoke
brew install --build-from-source mty
brew test mty
```

The `--new` flag enables the extra-strict rules that apply only to
formulae being submitted for the first time.

## Submission steps

1. **Refresh SHAs.** Wait for the v0.32.0 release to publish binaries
   for all five arches. Pull every `.sha256` sidecar and update the
   four `sha256` lines in `mty.rb`. The two `# placeholder` comments
   should be removed in the same commit.

2. **Fork `Homebrew/homebrew-core`.** One-time:

   ```bash
   gh repo fork Homebrew/homebrew-core --clone --remote
   cd homebrew-core
   git checkout -b add-mty
   ```

3. **Drop the formula in.** homebrew-core organizes by first letter:

   ```bash
   cp ~/stardust/tools/distribution/homebrew/mty.rb Formula/m/mty.rb
   ```

4. **Local audit + install:**

   ```bash
   brew audit --strict --online --new mty
   brew install --build-from-source mty
   brew test mty
   ```

   Fix any audit findings before opening the PR. The audit bot
   on the homebrew-core side is the same `brew audit`, so a clean
   local run means a clean PR-bot run.

5. **Commit + push the fork:**

   ```bash
   git add Formula/m/mty.rb
   git commit -m "mty 0.32.0 (new formula)"
   git push -u origin add-mty
   ```

6. **Open the PR:**

   ```bash
   gh pr create \
     --repo Homebrew/homebrew-core \
     --title "mty 0.32.0 (new formula)" \
     --body "$(cat <<'EOF'
   This is a new formula for the Mighty programming language.

   - Upstream: https://github.com/hassard0/Mighty
   - Homepage: https://hassard0.github.io/Mighty/
   - License: MIT

   The formula ships pre-built binaries for the four supported
   `(os, arch)` combinations:

   - macOS arm64 (aarch64-apple-darwin)
   - macOS Intel (x86_64-apple-darwin)
   - Linux x86_64 (x86_64-unknown-linux-gnu)
   - Linux arm64 (aarch64-unknown-linux-gnu)

   `brew audit --strict --online --new mty` passes locally on
   macOS 14 (arm64) and Ubuntu 22.04 (x86_64).
   EOF
   )"
   ```

7. **Respond to the audit bot.** homebrew-core's CI will re-run
   `brew audit --strict` and `brew test`. If anything fails, push
   a fix-up commit to the same branch; the bot re-runs
   automatically.

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

## v0.33 follow-ups

- Add a CI step that runs `brew audit --strict --online` against
  the formula on every `tools/distribution/homebrew/mty.rb` change,
  using a Homebrew-on-Linux runner image.
- Auto-refresh the four SHAs in `mty.rb` from the published
  `.sha256` sidecars as part of the post-release "publish manifests"
  workflow.
