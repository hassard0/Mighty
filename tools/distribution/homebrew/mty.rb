# typed: false
# frozen_string_literal: true

# Mighty Homebrew formula
#
# This file is shipped from the Mighty source tree at
# `tools/distribution/homebrew/mty.rb`. To publish, copy it into the
# `homebrew-mighty` tap repository at `Formula/mty.rb` and push:
#
#     cp tools/distribution/homebrew/mty.rb \
#        ../homebrew-mighty/Formula/mty.rb
#     (cd ../homebrew-mighty && git add Formula/mty.rb \
#        && git commit -m "mty 0.30.1" && git push)
#
# End users then install via:
#
#     brew tap hassard0/mighty
#     brew install mty
#
# When a new Mighty release is cut, update `version`, every `url`
# line, and every `sha256` line. v0.32 (Track D) wired the formula
# for four `(os, arch)` blocks; the two new arches
# (`x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`) land in
# v0.32.0 binaries — until then the SHAs for those two blocks are
# placeholder zeros and must be refreshed before the v0.32.0 tap
# push.
#
# Homebrew-core submission: see
# `tools/distribution/homebrew/HOMEBREW_CORE_SUBMISSION.md` for the
# audit checklist and PR steps.

class Mty < Formula
  desc "Agent-first systems programming language"
  homepage "https://hassard0.github.io/Mighty/"
  version "0.30.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/hassard0/Mighty/releases/download/v0.30.1/mty-aarch64-apple-darwin.tar.gz"
      sha256 "ed786d66da4211724d42e66d289ea530af1a174c7e40cb1f5c38a6fb7700ab8e"
    end
    on_intel do
      # v0.32 (Track D): Intel macOS is now produced by release.yml.
      # The SHA below is a placeholder until the first v0.32.0 build
      # publishes the binary; refresh from
      # https://github.com/hassard0/Mighty/releases/download/v$VERSION/mty-x86_64-apple-darwin.tar.gz.sha256
      # at tap-push time. Until then Intel-Mac users can install
      # under Rosetta (`arch -arm64 brew install mty`) or build from
      # source: `cargo install --git https://github.com/hassard0/Mighty mty-cli`.
      url "https://github.com/hassard0/Mighty/releases/download/v0.30.1/mty-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/hassard0/Mighty/releases/download/v0.30.1/mty-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "c5bb431ea6d3e57c0952ecde6d9943281d00d513d388ae6f55722e810031c602"
    end
    on_arm do
      # v0.32 (Track D): aarch64 Linux is now produced by
      # release.yml (cross-compiled). The SHA below is a placeholder
      # until v0.32.0 binaries publish; refresh from
      # https://github.com/hassard0/Mighty/releases/download/v$VERSION/mty-aarch64-unknown-linux-gnu.tar.gz.sha256
      # at tap-push time.
      url "https://github.com/hassard0/Mighty/releases/download/v0.30.1/mty-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "mty"
  end

  test do
    assert_match "mty", shell_output("#{bin}/mty --version")
  end
end
