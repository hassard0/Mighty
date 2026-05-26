# Normative Conformance for Mighty

**Status:** normative (v1.0-RC). This document defines what it means
for a Mighty implementation to be conforming, and references the
**conformance kit** that implementations MUST pass to claim
conformance.

It MUST be read together with the language reference
(`docs/spec/v1.0-rc.md`), which defines the abstract language; this
document defines the *evidence* that an implementation realises that
language faithfully.

## 1. Audience

This document is for:

* Implementors of new Mighty front-ends (compilers, language servers,
  formatters, linters).
* Implementors of new Mighty runtimes (interpreters, native codegen,
  WASM-component codegen).
* Reviewers / certification bodies who need a checkable claim of
  conformance.

It is NOT a tutorial. New users should read
`docs/spec/v1.0-rc.md` first.

## 2. Definitions

* **Mighty implementation** — a program (binary, library, or hybrid)
  that accepts Mighty source files (`.mty`) and either type-checks,
  compiles, or runs them.
* **Normative behaviour** — the behaviour mandated by
  `docs/spec/v1.0-rc.md`. Words like MUST / MUST NOT / SHALL / SHALL
  NOT appear there with their RFC 2119 meanings.
* **Conformance kit** — the tarball produced by
  `scripts/build-conformance-kit.sh` from the contents of
  `tests/conformance/`. Each release of the spec is paired with a kit
  at the same version.
* **Conforming implementation** — an implementation that passes every
  populated case in the conformance kit at its declared spec version,
  subject to the relaxations in §6.

## 3. The conformance kit

The kit is a self-contained tar.gz of:

```
tests/conformance/<category>/<NN_case_name>/
  input.mty                 — the test program
  expected_diagnostics.txt  — expected diagnostic codes + messages
  expected_exit_code.txt    — expected process exit code (0 / non-zero)
  command.txt               — suggested CLI invocation
```

plus the spec doc (`docs/spec/v1.0-rc.md`) and the kit's own
manifest (`tests/conformance/CONFORMANCE_KIT.md`).

The kit's version matches the spec's version. A kit built from spec
v1.0 is `mty-conformance-kit-v1.0.tar.gz`.

## 4. Categories (v0.19 snapshot)

The kit ships **122 cases across 20 populated categories**. Conformance
is computed per category — an implementation MAY claim conformance for
a subset of categories (e.g., a parse-only tool conforms to `lexical/`
and `parser/` only).

Populated categories at v0.19:

| Category               | Cases | What it checks                                      |
|------------------------|-------|-----------------------------------------------------|
| `lexical/`             | 3     | Lexer error recovery + diagnostic codes (MT0xxx)    |
| `parser/`              | 1     | Parser recovery + MT1xxx diagnostics                |
| `type_checking/`       | 20    | Type-checker rules + MT2xxx diagnostics             |
| `type_inference/`      | 5     | HM inference + literal default types                |
| `borrow_checking/`     | 13    | Ownership + lifetime rules (MT3xxx)                 |
| `ownership_rejection/` | 4     | Borrow-checker rejection cases                      |
| `effect_checking/`     | 5     | Effect-row inference + MT4xxx                       |
| `capability_checking/` | 4     | Per-call capability manifest (RFC-004)              |
| `budget_violation/`    | 6     | `budget { … }` block enforcement                    |
| `traits_derive/`       | 4     | `derive` macros for built-in traits                 |
| `macros/`              | 6     | Macro expansion + sandboxed-macro contract (RFC-003)|
| `agent_protocol/`      | 5     | Agent message protocols (RFC-006)                   |
| `mailbox_ordering/`    | 7     | Per-agent mailbox semantics                         |
| `supervisor_restart/`  | 4     | Supervisor restart policies                         |
| `control_flow/`        | 5     | `break`, `continue`, iterator protocol              |
| `runtime/`             | 6     | Generic runtime behaviour                           |
| `runtime-7/`           | 8     | v0.7-specific runtime additions                     |
| `runtime_traps/`       | 2     | Trap propagation                                    |
| `codegen/`             | 9     | WASM-component / native codegen output              |
| `spec_coverage/`       | 5     | Cross-cutting cases that exercise §-by-§ spec rules |

Placeholder categories (v1.0 backlog, do NOT block conformance for
v1.0): `deterministic_replay/`, `formatter_idempotence/`,
`native_abi/`, `wasm_component/`.

## 5. Test-driver protocol

A conforming implementation MUST run each populated case as follows:

1. **Resolve the command.** Read `command.txt`; substitute the
   implementation's binary for the leading `mty` token. Implementations
   that don't ship a `check`/`build` distinction MUST use their
   nearest equivalent (the kit's `command.txt` files are advisory but
   the choice MUST be deterministic).
2. **Execute.** Run the command on `input.mty`. Capture stdout,
   stderr, and the process exit code. The implementation MUST NOT
   network-access during this step.
3. **Compare diagnostics.** Compute the set of diagnostic codes
   produced (any band: `MT[0-6]xxx`) and compare to the set in
   `expected_diagnostics.txt`. The comparison is order-insensitive.
   Implementations MAY produce additional diagnostics not in the
   expected set; conformance only requires the expected set to be a
   subset of the produced set.
4. **Compare exit code.** Compare the process exit code to
   `expected_exit_code.txt`. Only the contract `0 = success,
   non-zero = failure` is normative; the exact non-zero value is
   implementation-defined.

A case passes if both (3) and (4) succeed. A category passes if every
case in it passes. The implementation conforms to the category if the
category passes.

## 6. Allowed deviations

Implementations MAY deviate from the kit in the following well-defined
ways:

### 6.1 Diagnostic codes within a band

Per `docs/spec/independent-impls.md`, an independent implementation MAY
use diagnostic codes that differ from the Rust reference's, as long as
the band (`MT[0-6]xxx`) matches the kit's expected code. A 2nd-impl
test driver SHOULD ship a code-mapping table to translate between code
spaces when comparing.

### 6.2 Diagnostic message wording

The exact wording of diagnostic messages is implementation-defined.
Only the code + the contract (error vs warning vs note) is normative.

### 6.3 Span precision

Where the spec mandates a span on a diagnostic, the span's start MUST
land within the offending construct's source range, but the end MAY
be tightened or widened by implementation choice. Cases in the kit do
not assert exact span endpoints (they assert codes).

### 6.4 Placeholder categories

A v1.0 implementation MAY skip placeholder categories (those listed in
§4 under "Placeholder categories"). A post-v1.0 implementation SHOULD
implement them once the spec normative text catches up.

### 6.5 Codegen-target subset

An implementation MAY target a subset of codegen backends (e.g.,
WASM-only, native-only). The `codegen/` category cases are tagged with
the target they exercise; an implementation conforms to `codegen/` if
every case for the backends it claims passes.

## 7. Claiming conformance

A claim of conformance is a statement of the form:

> *Implementation X version Y conforms to Mighty spec v1.0 at
> conformance kit v1.0, with the following categories passed:
> {categories}, and the following deviations declared: {deviations}.*

The claim SHOULD be machine-checkable by a CI workflow that:

1. Downloads the published kit tarball for the claimed spec version.
2. Runs the implementation against every populated case in the
   declared categories.
3. Verifies the test-driver protocol §5 succeeds for every case.
4. Emits the categories-passed list and any deviations.

Anthropic (the reference-impl maintainer) MAY publish a community
scorecard tracking known conformance claims; submission is voluntary.

## 8. Versioning

* Adding cases to the kit is non-breaking — older implementations
  remain conformant to their declared kit version.
* Removing or materially changing an expected outcome bumps the kit's
  major version.
* The kit version matches the spec version. A `v1.0` claim refers
  to the kit at `v1.0`.

## 9. References

* `docs/spec/v1.0-rc.md` — the normative language reference.
* `docs/spec/independent-impls.md` — the cross-impl coordination rule.
* `tests/conformance/CONFORMANCE_KIT.md` — the kit's manifest.
* `tests/conformance/README.md` — kit consumer + contributor guide.
* `scripts/build-conformance-kit.sh` — the kit builder.

## 10. Changelog

* **v0.19** (this document, 2026-05-26) — initial publication. Closes
  v1.0-freeze blocker #3.
