# Demo 03 — `extract_tool`

A small "AI extraction" CLI built around a top-level `sandbox` with
budget and capability caps. The agent classifies candidate tokens
against an in-process entity vocabulary; the sandbox header
declares the policy the runtime enforces.

## What this demonstrates

| Surface | What this demo does |
|---|---|
| Top-level `sandbox` block | `cpu` / `wall` / `mem` / `mailbox` budgets + `fs.read` / `fs.write` capability allow-lists. |
| `BudgetTracker` runtime enforcement | The `breach.mty` companion deliberately trips the budget; the runtime returns `RunResult::MemBudgetExceeded` / `MT5009 budget_exceeded`. |
| `FsCap` allowlist | `std.fs.read("./outside")` outside the allowlist returns `Result::Err(forbidden:...)` instead of reading the file. |
| Real `Str` method table | `contains`, `find`, `char_at`, `slice`, `to_lower`, `split`, etc — the v0.5 string-methods enforcement landed via `crates/mty-sir/tests/string_methods.rs`. |
| `BudgetBreach::Path` trip | Any out-of-allowlist filesystem call fires the typed breach. |

Brought to its current shape by **v0.5** (closed three v0.4
enforcement gaps: full string method table, real CPU/memory budget
trips, FsCap allowlist consultation from the sandbox manifest).

## Layout

```
03_extract_tool/
├── README.md
├── mighty.toml             # host-profile package
├── src/
│   ├── main.mty            # Extractor agent + sandbox-wrapped driver
│   └── breach.mty          # companion: deliberately impossible caps
├── inputs/sample.json      # fixture (text + tokens)
├── expected_output.txt     # golden output for main.mty
└── smoke.sh / smoke.ps1    # diff against the golden output
```

## Build / run

```bash
cargo build -p mty-cli
./target/debug/mty check demos/03_extract_tool/src/main.mty
./target/debug/mty run   demos/03_extract_tool/src/main.mty
```

PowerShell:

```powershell
cargo build -p mty-cli
.\target\debug\mty.exe check demos\03_extract_tool\src\main.mty
.\target\debug\mty.exe run   demos\03_extract_tool\src\main.mty
```

## Expected output

Also pinned in `expected_output.txt`:

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
hit: Mighty
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
and additionally runs `breach.mty` (the deliberately impossible
sandbox) to confirm the breach path traps cleanly without
corrupting the runtime.

## Reading the sandbox header

```mty
sandbox extract_session {
  cpu     = 100ms
  wall    = 3s
  mem     = 16MiB
  mailbox = 64

  caps {
    fs.read  = ["./inputs"]
    fs.write = ["./outputs"]
  }
}
```

Every cap line is consulted by the runtime on the corresponding
host call. `std.fs.read("./inputs/sample.json")` succeeds (in the
allowlist); `std.fs.read("./outside")` returns
`Result::Err(forbidden: ...)`. `cpu`, `wall`, `mem`, `mailbox` are
checked by the `BudgetTracker` on every yield to the runtime;
`breach.mty` deliberately trips them.

## What this demo does NOT do

The token vocabulary in `main.mty` is hand-coded — the extraction
shape is the demo's focus, not a real LLM call. Demo 07
(`07_research_agent`) wires the same protocol to a real `std.llm`
provider for the LLM-driven extraction story.
