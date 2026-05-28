# Homebrew tap publishing

The `mty.rb` formula in this directory is the canonical source for
the Mighty Homebrew tap.

## One-time tap repo setup

Create a public repo named `homebrew-mighty` under your GitHub
account. Homebrew taps live at `<user>/homebrew-<name>` and are
referenced as `<user>/<name>`.

```bash
gh repo create hassard0/homebrew-mighty --public \
  --description "Homebrew tap for the Mighty programming language"
git clone git@github.com:hassard0/homebrew-mighty.git
mkdir -p homebrew-mighty/Formula
```

## Per-release publish

```bash
# From the Mighty source tree, after a tag is published:
cp tools/distribution/homebrew/mty.rb \
   ../homebrew-mighty/Formula/mty.rb

cd ../homebrew-mighty
git add Formula/mty.rb
git commit -m "mty 0.30.1"
git push
```

## End-user install

```bash
brew tap hassard0/mighty
brew install mty
mty --version
```

## Updating the formula for a new release

1. Bump `version "X.Y.Z"`.
2. Replace both `url` lines with the new tag.
3. Replace both `sha256` lines. Fetch with:

   ```bash
   curl -sL https://github.com/hassard0/Mighty/releases/download/vX.Y.Z/mty-aarch64-apple-darwin.tar.gz.sha256
   curl -sL https://github.com/hassard0/Mighty/releases/download/vX.Y.Z/mty-x86_64-unknown-linux-gnu.tar.gz.sha256
   ```

4. Optionally run `brew audit --strict --online mty` against the
   local file before committing.

## Future work

- Publish an `x86_64-apple-darwin` and `aarch64-unknown-linux-gnu`
  binary in release.yml so Intel Mac and Linux ARM aren't a
  source-build path.
- Once the formula is stable, submit to homebrew-core so users can
  drop the `brew tap` step.
