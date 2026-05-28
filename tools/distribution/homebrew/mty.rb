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
# When a new Mighty release is cut, update `version`, the two `url`
# lines, and the two `sha256` lines. The conformance kit reference
# stays optional.

class Mty < Formula
  desc "Mighty - agent-first systems programming language"
  homepage "https://hassard0.github.io/Mighty/"
  version "0.30.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/hassard0/Mighty/releases/download/v0.30.1/mty-aarch64-apple-darwin.tar.gz"
      sha256 "ed786d66da4211724d42e66d289ea530af1a174c7e40cb1f5c38a6fb7700ab8e"
    end
    # No x86_64 macOS binary is published yet. Intel-Mac users can
    # either install via Rosetta (`arch -arm64 brew install mty`) or
    # build from source: `cargo install --git https://github.com/hassard0/Mighty mty-cli`.
  end

  on_linux do
    on_intel do
      url "https://github.com/hassard0/Mighty/releases/download/v0.30.1/mty-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "c5bb431ea6d3e57c0952ecde6d9943281d00d513d388ae6f55722e810031c602"
    end
    # aarch64 Linux is not yet published. Build from source for now.
  end

  def install
    bin.install "mty"
  end

  test do
    assert_match "mty", shell_output("#{bin}/mty --version")
  end
end
