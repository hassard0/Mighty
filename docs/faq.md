# Frequently Asked Questions

A grab-bag of questions that come up often. If yours isn't here,
open an issue on
[github.com/hassard0/Mighty](https://github.com/hassard0/Mighty).

## Language and design

### Why another systems language?

Mighty bets that "agent-era" software needs a language where
concurrency, authority, failure, memory, and observability are
compiler-visible semantics rather than framework conventions. The
intent is *faster than idiomatic C++* on targeted workloads by
making optimization facts (no aliasing, bounded lifetimes, known
effects, known capabilities) explicit, while staying *easier than
Go* by inferring most local types and offering compact canonical
forms for the boilerplate.

See [spec §0 Executive Definition](spec/v1.0-rc.md).

### Why is the compiler in Rust?

Phase 1 prioritized implementation velocity and memory safety. Per
spec §31.2: *"Rust is preferred for implementation velocity and
memory safety."* The compiler itself is not user-visible — the
language self-hosts the lexer, parser, HIR, and minimal typeck as
of v0.9 (40 self-host tests passing); once it self-hosts the rest,
the bootstrap host stops mattering.

### Why `.mty`?

Short, unambiguous, and globally unused. The v0.7 rebrand renamed
the historical `.sd` extension (originally chosen for
*Stardust*) to `.mty` to match the new project name. Both the source
extension and the diagnostic prefix changed; see the next two
entries.

### What happened to `.sd` and `SD####` codes?

The v0.7 rebrand renamed:

| Was              | Is                |
|------------------|-------------------|
| `stardust` repo  | `mighty` repo     |
| `sdust-*` crates | `mty-*` crates    |
| `star.toml`      | `mighty.toml`     |
| `.sd` source ext | `.mty` source ext |
| `SD####` codes   | `MT####` codes    |

The `SD` diagnostic prefix is preserved as an alias inside
`mty explain` per amendment **A107** — your bookmarks to
`mty explain SD0001` keep working. The `.sd` extension was *not*
preserved as an alias; old files have to be renamed. Per amendment
**A107** the byte-for-byte renamed v0.7.0 release passes all 885
tests of v0.6 with zero behavioural deltas.

### Why `mty` and not `mighty`?

A short binary name pays for itself every time you type it. `mty`
is unambiguous and consistent with the other ecosystem identifiers
(`mighty.toml`, `.mty`, `mty pkg`).

### Why are generics in square brackets?

To keep parsing unambiguous with comparison operators and to remove
the need for the turbofish. `Vec[Str]` is unambiguous; `Vec<Str>`
requires lookahead in some contexts. See tour
[chapter 3](tour/03-generics.md) and amendment **A2** for the
expression-position `::[T]` turbofish.

### What's the difference between `1k`, `1K`, `1KiB`, `1KB`?

Per amendment **A1**:

- **Binary suffixes** (`1KiB` = 1024, `1MiB` = 1048576,
  `1GiB` = 2^30) — the only suffixes accepted in memory and storage
  contexts.
- **Decimal suffixes** (`1k` = 1000, `1M` = 1000000) — accepted in
  count contexts (e.g. `mb 1k` for mailbox depth).
- `KB` / `MB` / `GB` (uppercase decimal) and lowercase `kib` etc.
  are rejected by the lexer. Use the canonical forms above.

`mty explain MT0004` covers duration units in the same vein.

### What's the difference between an agent and an Erlang process?

Conceptually very similar — isolated state, mailbox-driven,
supervised by a tree. The main differences are static typing
(protocols), capability-based authority (no ambient I/O), and
compile-time visible effects.

### What's the difference between an agent and an actor?

Mighty agents have all four: isolation, asynchrony, typed
protocols, and capability boundaries. Most actor libraries pick
two or three. Spec §2.3 lists the full set: "isolated state owner,
concurrency unit, failure boundary, capability boundary,
observability boundary, scheduling boundary."

### What's the difference between an agent and a task?

A *task* is a one-shot future scheduled by the runtime — it runs to
completion and yields a value. An *agent* is long-lived, owns
state, has an inbox, and answers messages over its lifetime. Use
tasks for fire-and-forget background work; use agents for anything
with state or identity.

### Can I use Mighty for embedded or kernel work?

That is the `core` profile. It forbids global GC, dynamic dispatch
by default, and a managed heap. The profile constraints are
enforced; see tour
[chapter 10](tour/10-capabilities.md) for the capability story and
amendment **A82** (DEFER-V1.1) for the planned no-`std` polish.

## Installation

### Why does `mty` fail to link on Windows?

If you see an error mentioning `link.exe` not found or a missing
DLL during `cargo install --path crates/mty-cli`, you're hitting
the MSVC-toolchain dependency. Two fixes:

1. Install the Visual Studio Build Tools (the "Desktop development
   with C++" workload).
2. Or, install the GNU toolchain target:
   `rustup toolchain install stable-x86_64-pc-windows-gnu` and use
   that instead.

If the build succeeds but `mty build` (the *user-facing build
command*) can't find a linker, point `STARDUST_LINKER` at one or
install a C toolchain — see `mty explain MT8008`.

### What is the macOS `LC_BUILD_VERSION` fix?

macOS 11+ requires every Mach-O object to carry an
`LC_BUILD_VERSION` load command. Cranelift didn't emit one until
v0.10.0 (commit `7f2feab`). If you see a linker warning like
"object file does not contain platform information" when running
`mty build`, upgrade to v0.10.0 or later; the warning is benign on
older releases but blocks notarisation.

### What MSRV does Mighty require?

Rust **1.85+** (slice 8 bumped the MSRV from the previous floor of
1.78). The CI MSRV gate runs `cargo test --no-run` + a bedrock test
subset to guarantee the floor stays accurate. If you need an older
Rust, file an issue — but expect the answer to be "use rustup,
it's free".

### Where are the binary releases?

There are none yet. v0.10 is the last pre-release; binary releases
ship alongside **v1.0**. Until then, `cargo install --path
crates/mty-cli` is the only supported install path.

## Status

### What works today (v0.10)?

- Lexing, parsing, formatting, CST/AST/HIR/SIR dumps.
- Type checker, borrow checker, effect/capability checker.
- Codegen: Cranelift native + WebAssembly (a narrow but growing
  MtyIR subset). LLVM backend is stubbed.
- Runtime: tokio executor + per-agent mailboxes + supervisors +
  budgets + sandboxes.
- 20 canonical examples + 81 conformance cases (88% FROZEN
  coverage).
- Self-host: lexer, parser, HIR, minimal typeck — **40 self-host
  tests passing**.
- 977 tests passing across the workspace as of v0.10.0.

### What's still on the v1.0 backlog?

The six tracked RFCs (in
[`docs/spec/rfcs/`](https://github.com/hassard0/Mighty/tree/main/docs/spec/rfcs)):

- **RFC-001** second-implementation effort
- **RFC-002** MT0001 funnel split
- **RFC-003** mty-pkg cross-file resolution
- **RFC-004** parametric newtypes for self-host arena ids
- **RFC-005** set-of-scopes hygiene in LSP completion (A111)
- **RFC-006** normative conformance suite kit publication

`mty doc` and `mty bench` are spec'd but not implemented; `mty pkg
publish` is RC2-deferred to v1.1.

### Why doesn't `mty check` catch this obviously wrong program?

If a snippet looks wrong but `mty check` says `ok`, check whether
it's a spec-only feature (rare today — most are enforced) or a
known-deferred amendment. The
[conformance corpus](https://github.com/hassard0/Mighty/tree/main/tests/conformance)
is the canonical list of what's enforced.

## Using Mighty

### Is Mighty production ready?

**No.** The spec is RC2 and the toolchain is v0.10 (pre-1.0). It
is stable enough for hobby projects, learning, and demos. It is
not yet stable enough to bet a paycheck on. Open issues if you
find rough edges and they will be triaged for v1.0.

### Can I use Mighty in a hobby project?

Yes, with two warnings:

1. There's no semver guarantee until v1.0. A `git pull` may break
   your project — pin to a commit hash if that matters to you.
2. There's no binary release. Anyone else who wants to run your
   code has to build the toolchain themselves.

### How do I report a bug?

Open an issue on
[github.com/hassard0/Mighty](https://github.com/hassard0/Mighty)
using one of the templates in `.github/ISSUE_TEMPLATE/`. A
minimal `.mty` reproducer plus the output of `mty --version` makes
triage 10× faster.

### How do I become an early adopter?

There's no formal program. The way to participate:

1. Build a thing in Mighty and write up what worked / what didn't.
2. Open issues for the rough edges.
3. Watch the [v1.0 milestone](https://github.com/hassard0/Mighty/milestone/1)
   for the freeze date.

Pull requests are welcome — see
[contributing.md](contributing.md) for the workflow.

### How can I help?

See [contributing.md](contributing.md). The highest-leverage
contributions today:

- **Conformance cases** — every new test case adds normative
  ground truth.
- **Real-world `.mty` programs** — we need a few more
  end-to-end examples in `examples/` and `demos/`.
- **Doc fixes** — if the tour or getting-started misled you,
  open a PR to fix it.
- **Parser fuzz finds** — the four-target fuzz harness lives at
  `fuzz/`; run it and report any panic.

### Where do I find spec amendments?

The amendments register is at
[`docs/spec/v0.1-amendments.md`](spec/v0.1-amendments.md). 88
numbered amendments (A1..A109, with gaps) are the historical
record. The consolidated v1.0-RC2 spec at
[`docs/spec/v1.0-rc.md`](spec/v1.0-rc.md) is what RC2 normatively
specifies; the amendments file remains the per-decision archive.
