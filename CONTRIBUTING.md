# Contributing

Issues are welcome. Pull requests must:

- be rebased on `main`;
- include tests for new behavior;
- pass `cargo fmt --all -- --check`;
- pass `cargo clippy --workspace --all-targets -- -D warnings`;
- pass `cargo test --workspace`.

CI runs the same three commands on every PR — see
[`.github/workflows/ci.yml`](.github/workflows/ci.yml).

For everything else — workflow, style, design discussions — see
[docs/contributing.md](docs/contributing.md).

By contributing you agree to dual-license your contribution under
Apache-2.0 OR MIT.
