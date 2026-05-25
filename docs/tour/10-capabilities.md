# 10 — Capabilities

A capability is a value that grants authority. Code in Mighty cannot
do I/O unless it holds a capability that confers it. Capabilities are
ordinary values, plumbed through function and constructor parameters.

## The program

```mty
fn load(fs: Fs, path: Path) -> Bytes!IoErr {
  fs.read(path)?
}

agent Fetcher(net, clock): Fetch {
  on Page(url) -> net.get(url) @2s?
}
```

## What is interesting

- `fs: Fs` is a filesystem capability. To call `fs.read(...)`, the
  caller must hand the function an `Fs` value — the language has no
  ambient filesystem.
- `agent Fetcher(net, clock): Fetch` takes two capabilities at
  construction. The compiler infers `net: Net` and `clock: Clock`
  from the canonical names per [spec §8](../spec/v1.0-rc.md).
- Capabilities cannot be forged, only narrowed and delegated. The
  built-in idiom is `let read_only = fs.ro("/data")` — see spec §8.1.
  Passing a wider cap where a narrower one is wanted triggers
  `MT4010 capability_too_broad`.
- Capabilities can't cross agent boundaries as references. Anything
  you pass through `!Msg(...)` / `?Msg(...)` must be **Sendable**:
  Copy ∨ owned ∨ owned struct of Sendable fields. References and raw
  pointers fail `MT3011 non_sendable_message_arg`. See
  [14 — Ownership](14-ownership.md).

## Try it

```bash
mty check examples/13_capabilities.mty
```

The current `13_capabilities.mty` emits one `MT2026` warning (handler
message `Page` is not declared by any implemented protocol in the
example's package). The warning is intentional — example 13 is
deliberately self-contained and the `Fetch` protocol lives in its
header comment only.

## Next

Continue to [11 — Budgets](11-budgets.md).
