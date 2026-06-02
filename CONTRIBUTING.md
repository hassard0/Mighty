# Contributing

Issues are welcome. Pull requests must:

- be rebased on `main`;
- include tests for new behavior;
- pass `cargo fmt --all -- --check`;
- pass `cargo clippy --workspace --all-targets -- -D warnings`;
- pass `cargo test --workspace`.

CI runs the same three commands on every PR — see
[`.github/workflows/ci.yml`](.github/workflows/ci.yml).

For a closer-to-real preview of what the GitHub Actions runners will
do (Linux/macOS test mode + Windows-serial test mode + the dropped
debuginfo profile that keeps Ubuntu under its disk ceiling), run
[`scripts/test-like-gha.sh`](scripts/test-like-gha.sh) or its Windows
sibling [`scripts/test-like-gha.ps1`](scripts/test-like-gha.ps1)
before you push. See
[docs/contributing.md](docs/contributing.md#scriptstest-like-ghash--v045-t4)
for when to reach for it.

For everything else — workflow, style, design discussions — see
[docs/contributing.md](docs/contributing.md).

By contributing you agree to release your contribution under the
MIT license (see [LICENSE](LICENSE)).
