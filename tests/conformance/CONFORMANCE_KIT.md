# Mighty Conformance Kit — Manifest

This manifest describes the contents of the Mighty normative conformance
suite, as packaged by `scripts/build-conformance-kit.sh` into a
downloadable tarball.

The conformance kit closes v1.0-freeze blocker #3: it gives implementors
a self-contained set of inputs + expected diagnostics that can be run
against any Mighty implementation to verify spec conformance.

## Kit version

The kit's version matches the spec RC tag at the time of packaging
(`mty-conformance-kit-vX.Y.Z.tar.gz`). The intent is that the kit and
the spec RC ship as a single coordinated artifact: a kit built at
`v1.0-rc1` is paired with `docs/spec/v1.0-rc.md` at the same commit.

## Contents

```
mty-conformance-kit-<version>.tar.gz
├── tests/
│   └── conformance/
│       ├── README.md
│       ├── CONFORMANCE_KIT.md         (this file)
│       ├── <category>/
│       │   ├── README.md              (per-category index)
│       │   └── <NN_case_name>/
│       │       ├── input.mty          (the test program)
│       │       ├── expected_diagnostics.txt
│       │       ├── expected_exit_code.txt
│       │       └── command.txt        (suggested CLI invocation)
│       └── ... (24 categories total — see manifest below)
└── docs/
    └── spec/
        ├── v1.0-rc.md                 (the normative spec)
        └── conformance.md             (this kit's normative description)
```

## Manifest — per category

| Category               | Cases | Status   |
|------------------------|-------|----------|
| `lexical/`             | 3     | populated |
| `parser/`              | 1     | populated |
| `type_checking/`       | 20    | populated |
| `type_inference/`      | 5     | populated |
| `borrow_checking/`     | 13    | populated |
| `ownership_rejection/` | 4     | populated |
| `effect_checking/`     | 5     | populated |
| `capability_checking/` | 4     | populated |
| `budget_violation/`    | 6     | populated |
| `traits_derive/`       | 4     | populated |
| `macros/`              | 6     | populated |
| `agent_protocol/`      | 5     | populated |
| `mailbox_ordering/`    | 7     | populated |
| `supervisor_restart/`  | 4     | populated |
| `control_flow/`        | 5     | populated |
| `runtime/`             | 6     | populated |
| `runtime-7/`           | 8     | populated |
| `runtime_traps/`       | 2     | populated |
| `codegen/`             | 9     | populated |
| `spec_coverage/`       | 5     | populated |
| `deterministic_replay/` | 0    | placeholder (v1.0 backlog) |
| `formatter_idempotence/` | 0   | placeholder (v1.0 backlog) |
| `native_abi/`          | 0     | placeholder (v1.0 backlog) |
| `wasm_component/`      | 0     | placeholder (v1.0 backlog) |
| **Total**              | **122** | 20 populated / 24 categories |

The 4 placeholder categories ship as empty directories with stub
`README.md` files. Implementations are not required to pass placeholder
categories for v1.0; the post-v1.0 roadmap fills them.

## How to use the kit

The kit is designed to be extracted next to any Mighty implementation
and run by a thin test driver. Each case directory has the shape:

```
01_unterminated_string/
  input.mty                  — the source program
  expected_diagnostics.txt   — expected diagnostic codes + messages
  expected_exit_code.txt     — expected process exit code
  command.txt                — suggested CLI invocation
```

### Test-driver protocol

A conforming implementation runs each case as follows:

1. Read `command.txt` for the suggested invocation (typically
   `mty check input.mty` or `mty build input.mty`). Substitute the
   path to the implementation's binary.
2. Execute the command. Capture stdout, stderr, and the exit code.
3. Diff stderr against `expected_diagnostics.txt`. Diagnostics may be
   reported in any order; the diff is on the **set** of `MTxxxx`
   codes plus the canonical message body (whitespace-normalised).
4. Compare the exit code to `expected_exit_code.txt`. Implementations
   may pick their own exit codes outside `{0, 1, 2}` — only the
   contract `0 = success, ≠ 0 = failure` is normative.

### Implementation hooks

For Rust-backed implementations, the reference workspace exposes a
`mty-conformance` crate (under `crates/mty-conformance/`) that wires
the kit into `cargo test`. Other implementations should run the cases
through whatever driver matches their language ecosystem (e.g., the
Python 2nd-impl can grow a `tests/test_conformance.py` that walks
`tests/conformance/` and exec's `python -m mty` on each input).

## Diagnostic-code stability

The kit pins the **band** of each diagnostic, not the exact numeric
value:

* MT0xxx — lexer
* MT1xxx — parser
* MT15xx — HIR / lowering (Python 2nd-impl)
* MT2xxx — type checker
* MT3xxx — borrow checker
* MT4xxx — effect / capability checker
* MT5xxx — codegen
* MT6xxx — runtime traps

Independent implementations are allowed to differ on the precise
numeric code within a band (per `docs/spec/independent-impls.md`). The
kit's expected_diagnostics.txt uses the Rust reference's codes; a
2nd-impl test runner is expected to map between code spaces.

## Versioning policy

* The kit ships with each spec RC and v1.0+ release.
* Adding cases is non-breaking — a new kit version may include new
  cases that older implementations don't pass yet.
* Removing or *materially* changing an expected diagnostic is a
  breaking change and bumps the kit's major version.
* Kit version `v1.0.0` is paired with spec `v1.0`.

## Roadmap

* v0.20 — fill `deterministic_replay/` + `formatter_idempotence/`
  category placeholders (target: ~15 additional cases).
* v1.0 — `native_abi/` and `wasm_component/` get their first cases
  (these depend on codegen completion).
* Post-v1.0 — a public per-impl scorecard tracker, updated when each
  release of the Rust reference and the Python 2nd-impl ship.
