---
name: Bug report
about: Report something that broke or behaves unexpectedly
labels: bug
---

## What happened

<!-- A clear, one-paragraph description of the bug. -->

## Repro

<!-- Minimal Mighty source + the exact `mty` command that triggered it. -->

```sd
fn main() {
  ...
}
```

```bash
mty check path/to/file.mty
```

## Expected vs actual

Expected: <!-- what you thought would happen -->
Actual:   <!-- what did happen, with the diagnostic / panic / wrong output -->

## Environment

- `mty --version`:
- Rust toolchain (`rustc --version`):
- OS and version:
- Mighty commit / tag:
