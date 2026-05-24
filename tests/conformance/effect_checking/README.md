# effect_checking

Spec §9 + slice-5 design. Public functions must declare every effect
they (transitively) perform. The `core` profile additionally bans
`alloc`.

## Cases

- `01_undeclared_net` — MT4001: `pub fn` uses `net.get` but doesn't
  declare `!{net}`.
- `02_declared_ok` — positive: same shape, but with `!{net}`
  declaration, check is clean.
- `03_undeclared_fs` — MT4001 against `fs.read`.

ALLOC_IN_CORE (MT4002) requires a `mighty.toml` with `profile = "core"`
on disk at the time of check. Because the conformance driver runs in
the workspace root (which does not have such a manifest), the MT4002
case is documented in `CONFORMANCE_V0_2_FINDINGS.md` rather than
populated here.
