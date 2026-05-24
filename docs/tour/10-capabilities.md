# 10 — Capabilities

A capability is a value that grants authority. Code in Stardust cannot
do I/O unless it holds a capability that confers it. Capabilities are
ordinary values, plumbed through function and constructor parameters.

## The program

```sd
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
  construction. The compiler infers `net: Net` and `clock: Clock` from
  the canonical names per [spec §8](../spec/v0.1.md).
- Capabilities cannot be forged, only narrowed and delegated. The
  built-in idiom is `let read_only = fs.ro("/data")` — see spec §8.1.
- Capabilities can't cross agent boundaries as references. Anything you
  pass through `!Msg(...)` / `?Msg(...)` must be **Sendable**: Copy ∨
  owned ∨ owned struct of Sendable fields. References and raw pointers
  fail `SD3011 non_sendable_message_arg`. See
  [14 — Ownership](14-ownership.md).

## Run it

```bash
sdust check examples/13_capabilities.sd
```

## Next

Continue to [11 — Budgets](11-budgets.md).
