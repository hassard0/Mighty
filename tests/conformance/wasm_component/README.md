# wasm_component/

v0.20-populated. Cases pinning the Wasm-Component-Model backend's
import/export shape (spec §30, `crates/mty-codegen-wasm`,
`dev/history/notes/INTROSPECT_V0_16_NOTES.md`).

The contract under test: when an implementation lowers a Mighty
package to a Wasm Component, the emitted component MUST:

1. Validate cleanly under `wasm-tools validate --features=component-model`.
2. Carry the canonical P2 import names for each effect family
   used (per `crates/mty-stdlib/src/{time,random,fs}.rs`).
3. Wire user-supplied `.wit` worlds when present, generating the
   matching `export` interfaces.

Each case ships:

```
NN_case_name/
  input.mty                       — the source program
  command.txt                     — `check` (asserts the program
                                    parses + type-checks)
  expected_diagnostics.txt        — usually empty (positive case)
  expected_exit_code.txt          — 0
  expected_component.txt          — descriptive: canonical import +
                                    export list the emitted component
                                    MUST carry (consumed by the
                                    wasm-component integration tests
                                    under crates/mty-codegen-wasm/tests/)
  README.md                       — what the case proves
```

The conformance_full harness asserts the program parses + type-checks.
The build-and-validate step lives in
`crates/mty-codegen-wasm/tests/component_shape.rs` (v0.20 stretch).

## Cases

| Case | Property under test |
|------|---------------------|
| `01_minimal_component` | empty `fn main` -> valid component |
| `02_wasi_p2_log`       | uses `log` -> imports `wasi:cli/stdout`, `wasi:io/streams` |
| `03_wasi_p2_fs`        | uses `Fs.read` -> imports `wasi:filesystem` |
| `04_user_wit`          | user-supplied `world.wit` -> custom world export |
