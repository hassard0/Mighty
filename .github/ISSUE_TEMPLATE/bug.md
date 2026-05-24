---
name: Bug report
about: Report something that broke or behaves unexpectedly
labels: bug
---

## What happened

<!-- A clear, one-paragraph description of the bug. -->

## Repro

<!-- Minimal Stardust source + the exact `sdust` command that triggered it. -->

```sd
fn main() {
  ...
}
```

```bash
sdust check path/to/file.sd
```

## Expected vs actual

Expected: <!-- what you thought would happen -->
Actual:   <!-- what did happen, with the diagnostic / panic / wrong output -->

## Environment

- `sdust --version`:
- Rust toolchain (`rustc --version`):
- OS and version:
- Stardust commit / tag:
