# control_flow — break / continue / iterator protocol

v0.5 conformance cases for the loop semantics added in slice A80 / A81.

Each subdirectory holds one `input.sd` plus an `expected.txt`. The harness
runs the input through the SIR interpreter and compares the program's exit
code (or the host's collected stdout where the case marks `mode: stdout`).

| Case                 | Asserts                                                          |
|----------------------|------------------------------------------------------------------|
| `01_break_simple`    | `loop { if cond { break } }` terminates                          |
| `02_break_value`     | `let x = loop { break 42 }` evaluates to 42                      |
| `03_continue`        | for-loop with continue skips iterations                          |
| `04_nested_break`    | break only exits the innermost loop (no labels in v0.5)          |
| `05_iter_range`      | `for i in 1..5` iterates exactly 4 times                         |

The cases are intentionally tiny — they are mirrored by unit tests in
`crates/sdust-sir/tests/loop_break.rs` / `loop_continue.rs` /
`for_range.rs`. The conformance suite here is the cross-crate witness:
if either the parser or the interpreter regresses, both the unit test
AND the conformance case fail. Two independent paths is the v0.1
conformance-suite contract (spec §37).
