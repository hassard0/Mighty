# Contributing to Mighty

Mighty is pre-alpha. The fastest way to contribute is to:

1. Pick an open issue, or file one describing what you want to do.
2. Fork the repo, branch off `main`.
3. Make your change with tests.
4. Run the full gate (see below).
5. Open a pull request.

## Code of Conduct

This project follows the
[Contributor Covenant 2.1](https://github.com/hassard0/Mighty/blob/main/CODE_OF_CONDUCT.md). Be respectful.

## Building

```bash
cargo build --workspace
cargo test --workspace
```

Minimum Rust: 1.82. The toolchain is pinned in `rust-toolchain.toml`.

## The pre-commit gate

Every PR must pass these three commands:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

These exactly match what CI runs (see
[`.github/workflows/ci.yml`](https://github.com/hassard0/Mighty/blob/main/.github/workflows/ci.yml)).

## Snapshot tests

The HIR dump and parser smoke tests use
[insta](https://docs.rs/insta) snapshots. If you intentionally change
the dump shape, regenerate them:

```bash
cargo insta review
```

Then commit the updated `.snap` files.

## Style

- Follow `rustfmt` defaults. The CI gate enforces it.
- Prefer named functions over deeply-nested closures.
- Public items get a doc comment. At minimum, one sentence.
- Errors get a stable [`DiagCode`](reference/diagnostics.md).
- No `unwrap`/`expect` in non-test code unless the invariant is local
  and documented inline.

## What kinds of changes are welcome right now

- Parser fixes for syntax already in the spec.
- New example programs that exercise spec corners.
- Documentation improvements (this docs tree, the inline rustdocs,
  example annotations).
- New diagnostic codes with good error text.
- Tests — there are never enough.

## What kinds of changes need a design discussion first

- New language constructs not in the spec.
- IR shape changes.
- Anything that touches the build / release process.

File an issue describing the design before writing the code.

## Single-commit PRs

For most changes, squash to a single commit before review. The commit
message should explain the **why**, not just the **what**. Reference
the issue number.

## License

By contributing, you agree to dual-license your contribution under
Apache-2.0 OR MIT, matching the project license.
