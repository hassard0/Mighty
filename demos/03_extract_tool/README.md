# Demo 03 — `extract_tool`

A small "AI extraction" CLI built around a top-level `sandbox` with
budget and capability caps. The agent classifies candidate tokens
against an in-process entity vocabulary; the sandbox header records
the policy the runtime would enforce in a production deployment.

> **v0.5 dogfood update.** Two enforcement gaps are now closed:
>
> 1. **Str method table** (`crates/sdust-sir/src/interp/run.rs::eval_method`)
>    now implements real `contains`, `starts_with`, `ends_with`,
>    `find`, `char_at`, `slice`, `to_lower`, `to_upper`, `trim`,
>    `split`, etc. The per-token `==` workaround in this demo can
>    be lifted; see `crates/sdust-sir/tests/string_methods.rs`.
> 2. **CPU + memory budgets** auto-trip via a new
>    `RunResult::MemBudgetExceeded` variant and an SD5009 trap when
>    a sandboxed run exceeds its `cpu` / `memory` ceiling. The
>    companion `breach.sd` now actually trips — see
>    `crates/sdust-sir/tests/budget_charges.rs`.
> 3. **FsCap allowlist enforcement** (`crates/sdust-stdlib/src/fs.rs`)
>    consults a process-wide default cap installed from the sandbox
>    manifest, so `std.fs.read("./outside")` returns
>    `Result::Err(forbidden:...)` instead of silently reading the
>    file.

## Layout

```
03_extract_tool/
  star.toml                # package manifest (host profile)
  src/
    main.sd                # Extractor agent + sandbox-wrapped driver
    breach.sd              # companion: deliberately impossible caps
  inputs/sample.json       # fixture (text + tokens)
  expected_output.txt      # golden output for main.sd
  smoke.sh / smoke.ps1     # diff against the golden output
  README.md                # this file
```

## Build / run

```bash
cargo build -p sdust-cli
./target/debug/sdust check demos/03_extract_tool/src/main.sd
./target/debug/sdust run   demos/03_extract_tool/src/main.sd
```

PowerShell:

```powershell
cargo build -p sdust-cli
.\target\debug\sdust.exe check demos\03_extract_tool\src\main.sd
.\target\debug\sdust.exe run   demos\03_extract_tool\src\main.sd
```

Expected stdout (also in `expected_output.txt`):

```
== sample-1 ==
hit: Alice
miss: met
hit: Bob
miss: in
hit: Paris
miss: on
hit: Tuesday
hit: Charlie
miss: called
miss: from
hit: Tokyo
== sample-2 ==
miss: the
miss: quick
miss: brown
miss: fox
== sample-3 ==
miss: Built
miss: with
hit: Stardust
== snapshot ==
{"hits":7}
```

## Smoke test

```bash
bash demos/03_extract_tool/smoke.sh
```

```powershell
pwsh demos\03_extract_tool\smoke.ps1
```

The script diffs the captured stdout against `expected_output.txt`
and additionally runs `breach.sd` (the deliberately impossible
sandbox) to make sure that path doesn't corrupt the runtime.

## What this demo does NOT (yet) do

Two v0.4 limitations show up:

1. **Path-based capability checks are recorded, not enforced.** The
   `fs.read = ["./inputs"]` and `fs.write = ["./outputs"]` entries in
   the sandbox block parse into the `Budget` struct
   (`crates/sdust-runtime/src/budget.rs`), but the v0.3 `std.fs`
   host bridge (`crates/sdust-stdlib/src/host.rs::fs_read`) uses an
   unrestricted `FsCap`. A real deployment that wired `Fs` as a
   genuine capability handle would see `BudgetBreach::Path` fire on
   any out-of-allowlist access; that wiring is post-v0.4. The
   demo therefore reads its fixture **out-of-band** (the smoke
   script feeds the tokens in via `main_body()` directly rather than
   via `fs.read`), but the sandbox header still demonstrates the
   shape a future enforcement pass will check against.
2. **Cpu / wall / memory budgets need a capability-marked call to
   trip.** The slice-6 SIR interpreter is synchronous (see A35), so
   the `wall = 3s` budget is checked the next time a capability call
   yields back to the runtime. A pure-compute loop never trips it
   today. The cpu/mem trackers are charged only by explicit
   `BudgetTracker::record_*` calls (see A37 / A50). The included
   `breach.sd` runs against `cpu = 1ns / wall = 1ns / memory = 1B /
   mailbox = 1` — under v0.4 it completes cleanly; once
   enforcement lands it will trap with `SD5009 budget_exceeded`
   without changes to the source.

A third limitation tilts the implementation more than the surface:

3. **String-pattern stdlib is stubbed.** The slice-6 `eval_method`
   table only binds `len`, `to_str`, `is_empty`, plus a handful of
   Result helpers (see `crates/sdust-sir/src/interp/run.rs`). String
   pattern methods (`contains`, `find`, `char_at`, `slice`) return
   permissive defaults. The extractor therefore drives off `==`
   on whole tokens against a small inlined vocabulary instead of
   tokenising character-by-character. The shape of the agent —
   protocol, per-message handler, state — is exactly what a
   "real-LLM"-backed extractor will use; we expect the body to
   sharpen to `model.invoke(text)` once the model bridge lands.

The companion `breach.sd` is the v0.4 smoke for the budget surface.
When enforcement ships, swap one of the helper test assertions to
expect a non-zero exit code and the demo will document the change.
