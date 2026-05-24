# Stardust v0.4 dogfood demos

Three end-to-end demos that exercise the v0.4 compiler + runtime
surface as an external user would. Each demo lives in its own
directory with a `star.toml`, source, a smoke script (bash +
PowerShell), and a step-by-step `README.md`.

Per spec §31.8 the alpha exit criterion is:

> external users can install compiler; examples compile from scratch;
> benchmarks published honestly; issue tracker and RFC process active.

These demos cover the first two points.

| # | Demo | Surface exercised | How it runs |
|---|------|-------------------|-------------|
| 01 | [`01_search_api`](01_search_api/) | HTTP service shape: protocol + agent + per-handler state | `sdust run` drives every endpoint; `smoke.sh` golden-checks the stdout |
| 02 | [`02_counter_web`](02_counter_web/) | Wasm Component Model output; browser host parses the imported `log` calls | `sdust build --target wasm32-web` → component validated by `smoke.sh`; `web/serve.sh` serves the HTML loader |
| 03 | [`03_extract_tool`](03_extract_tool/) | Top-level `sandbox` with cpu/wall/mem/mailbox + cap allow-lists; agent-driven token classifier | `sdust run` exercises the extractor; `smoke.sh` diffs against `expected_output.txt` |

## Run all smoke scripts

```bash
cargo build -p sdust-cli
for d in demos/0*/; do
  bash "$d/smoke.sh" || { echo "$d FAILED"; exit 1; }
done
```

PowerShell:

```powershell
cargo build -p sdust-cli
Get-ChildItem demos\0*\ -Directory | ForEach-Object {
    pwsh (Join-Path $_ "smoke.ps1")
    if ($LASTEXITCODE -ne 0) { throw "$($_.Name) FAILED" }
}
```

All three should print `<demo>: PASS`.

## v0.4 caveats summary

Each demo's README spells out its specific caveats. The shared themes
are tracked in [`DEMOS_V0_4_NOTES.md`](../DEMOS_V0_4_NOTES.md) at the
repo root:

* **`std.http.serve` host bridge**: shipped as a real hyper-backed
  Rust API but not yet routed by the v0.3 generic-call dispatcher, so
  Demo 01 drives the handler bodies directly inside `main()` rather
  than binding a real TCP socket.
* **wasm32-web DOM bindings**: WIT stubs exist (`get-element-by-id`,
  `set-text`) but the slice-8 lowerer hasn't filled them in — Demo 02
  routes UI updates through the working `log` import instead.
* **Sandbox + budget enforcement scope**: caps parse and are recorded
  by the runtime, but the cpu/wall/mem/path checks trip only when a
  capability-marked call yields back to the runtime — Demo 03's
  `breach.sd` documents the v0.4 frontier.
* **Slice-6 interpreter string API**: `len`/`to_str`/`is_empty` are
  bound; `contains`/`find`/`char_at`/`slice` return permissive stubs
  today. Demos pick shapes that avoid these stubs.

None of these caveats stop the demos from running end-to-end; they
document where v0.5 + v0.6 will sharpen the surface.
