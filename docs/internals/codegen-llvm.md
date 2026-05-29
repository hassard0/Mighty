# Internals — LLVM backend (v0.2)

`mty-codegen-llvm` provides an opt-in LLVM 17 backend behind the
`llvm` cargo feature flag. v0.1 shipped a *scaffold only* (A46) because
the slice-8 build host had no LLVM/`llvm-config` installed; v0.2 keeps
the opt-in stance — the host is still LLVM-free — but lands a real
SIR→LLVM IR lowerer that activates whenever the feature compiles.

```rust
use sdust_codegen_llvm::{compile, LlvmError, LlvmOptLevel, OutputKind};

let prog: sdust_sir::sir::Program = /* lowered from your Mighty source */;
compile(&prog)?;                                     // verify only
compile_to_path(&prog, Path::new("out.o"),
    OutputKind::Object, LlvmOptLevel::O2)?;          // emit object
```

## Build prerequisites

The crate's `llvm` feature depends on `inkwell`, which depends on
`llvm-sys 170.x`. `llvm-sys` insists on locating a matching LLVM 17
installation at build time. Install it per platform:

### macOS

```bash
brew install llvm@17
export LLVM_SYS_170_PREFIX=$(brew --prefix llvm@17)
# Verify llvm-config is on PATH and reports v17:
"$LLVM_SYS_170_PREFIX/bin/llvm-config" --version
cargo build -p mty-codegen-llvm --features llvm
```

If `brew` reports a Homebrew Apple Silicon / Intel mismatch, prefer
the architecture-native install (`arch -arm64 brew install llvm@17`
on Apple Silicon).

### Ubuntu 22.04+ / Debian 12+

```bash
# Distro-packaged LLVM 17 (Ubuntu 22.04 jammy-updates, 24.04 noble,
# Debian 12 bookworm-backports). Older releases need the upstream
# LLVM apt repo at https://apt.llvm.org/.
sudo apt install llvm-17-dev libpolly-17-dev libz-dev libzstd-dev
export LLVM_SYS_170_PREFIX=/usr/lib/llvm-17
llvm-config-17 --version          # → 17.0.x
cargo build -p mty-codegen-llvm --features llvm
```

For Ubuntu < 22.04 use the upstream apt repo:

```bash
wget https://apt.llvm.org/llvm.sh
chmod +x llvm.sh
sudo ./llvm.sh 17 all
sudo apt install libpolly-17-dev
export LLVM_SYS_170_PREFIX=/usr/lib/llvm-17
```

### Windows

Pick one of:

**Chocolatey (recommended for CI / scripted installs):**

```powershell
choco install llvm --version=17.0.6 -y
$env:LLVM_SYS_170_PREFIX = "C:\Program Files\LLVM"
& "$env:LLVM_SYS_170_PREFIX\bin\llvm-config.exe" --version
cargo build -p mty-codegen-llvm --features llvm
```

**Official LLVM 17 Windows installer** (from
[releases.llvm.org](https://releases.llvm.org/) — pick a 17.0.x
`.exe`):

1. Install LLVM 17 (with `Add LLVM to the system PATH for all users`
   checked).
2. Set `LLVM_SYS_170_PREFIX` to the install root (e.g.
   `C:\Program Files\LLVM`).
3. Build:

```powershell
$env:LLVM_SYS_170_PREFIX = "C:\Program Files\LLVM"
cargo build -p mty-codegen-llvm --features llvm
```

### Verifying the install

After setting `LLVM_SYS_170_PREFIX`, sanity-check the build before
running the wider test suite:

```bash
cargo build -p mty-codegen-llvm --features llvm
cargo test  -p mty-codegen-llvm --features llvm
# Smoke-test against a real Mighty source:
cargo run -p mty-cli --features llvm-backend -- build --release \
    examples/01_hello.sd
```

If `cargo build --features llvm` fails with
`No suitable version of LLVM was found system-wide or pointed to by
LLVM_SYS_170_PREFIX`, that's the documented condition — the feature
stays off and the driver falls back to Cranelift cleanly. Common
causes:

- `LLVM_SYS_170_PREFIX` points to an LLVM 16/18/19/20 install
  (`llvm-sys 170.x` only accepts 17.x). Run
  `$LLVM_SYS_170_PREFIX/bin/llvm-config --version` to confirm.
- On Linux, libpolly is missing → install `libpolly-17-dev`.
- On macOS, Xcode CLT is stale → `xcode-select --install`.

## Backend coverage

The LLVM lowerer mirrors what the Cranelift backend supports (so the
two backends ship the same source-language surface):

- integer / float / bool / char arithmetic
- locals → LLVM `alloca`s; loads/stores go through them
- aggregates lower to pointer-sized values (i64 indirection into a
  caller-allocated buffer), matching the Cranelift ABI
- direct fn-to-fn calls (monomorphized; see [monomorphization](#monomorphization))
- ADT construction (struct + enum tag+payload via Rvalue::AdtInit)
- struct destructuring (FieldRead, TupleRead)
- enum destructuring (VariantField projection)
- `if` / `goto` / `return` / `unreachable`
- `SwitchInt` / `SwitchVariant` lowered as native LLVM `switch`
- `?` propagation via `Term::TryReturnErr`
- `log` / `print` / `panic` via C-ABI runtime calls
- agent send / ask / spawn route through runtime stubs
- arena push/pop via runtime calls
- best-effort fallback for `MethodCall`, `IndexRead`, and unresolved
  externs (return zero / null pointer)

Out of scope (same as Cranelift):

- effect-system call dispatch (compiled inline)
- `dyn Trait` vtables
- closure capture (lambdas-with-environment)

## Optimization

The LLVM `PassBuilder` runs at the chosen `LlvmOptLevel`:

| Level | Meaning | Pipeline |
|-------|---------|----------|
| `O0`  | no optimization (fast compile) | `default<O0>` |
| `O2`  | release standard | `default<O2>` |
| `O3`  | aggressive | `default<O3>` |

The default for `mty build --release` is `O2`. Custom pipelines
(PGO, ThinLTO) are post-v0.2 work.

## Verification

Each function is verified with `FunctionValue::verify(true)` after
lowering; the whole module is verified with `Module::verify()` before
optimization. Verifier failures bubble up as
`LlvmError::VerifierFailed` so the driver can surface them as
diagnostics instead of crashing.

## Runtime ABI

The runtime ABI (`mty_runtime_*` symbols) is identical for LLVM
and Cranelift — they both link against the same C-ABI fns provided by
`mty-runtime::codegen_abi`. The `runtime_imports::RUNTIME_IMPORTS`
table in the Cranelift crate is the canonical source of truth; the
LLVM lowerer declares the same set with matching signatures.

## When to choose LLVM over Cranelift

| Use case | Recommended |
|----------|-------------|
| `mty run` JIT | Cranelift (faster compile, lower mem) |
| `mty build --debug` | Cranelift |
| `mty build --release` (host with LLVM 17) | LLVM (better code gen) |
| `mty build --release` (host without LLVM) | Cranelift (the only option) |
| Targeting an exotic triple Cranelift doesn't support | LLVM |

## Monomorphization

The LLVM backend reuses the same monomorphization pass as Cranelift
(`sdust_codegen_cranelift::mono::Monomorphizer`). For each
(fn, type-args) tuple, a specialized MtyIR function is produced; the
LLVM lowerer then treats each specialization as ordinary monomorphic
code.

Name mangling: `<fn>__<T1>_<T2>_...`. Cached to avoid duplicate
emission.

## Future work

- LLVM-specific PassBuilder pipelines (ThinLTO, PGO, AutoFDO).
- Cross-compilation triples (currently hardcoded to host).
- Debug info (LLVM `DWARF` emission via `inkwell::debug_info`).
- LLVM JIT mode (currently object-file emission only).
