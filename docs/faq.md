# Frequently Asked Questions

## Why another systems language?

Mighty bets that "agent-era" software needs a language where
concurrency, authority, failure, memory, and observability are
compiler-visible semantics rather than framework conventions. The
intent is *faster than idiomatic C++* on targeted workloads by making
optimization facts (no aliasing, bounded lifetimes, known effects,
known capabilities) explicit, while staying *easier than Go* by
inferring most local types and offering compact canonical forms for
the boilerplate.

See [spec §0 Executive Definition](spec/v0.1.md).

## Why is the compiler in Rust?

Phase 1 prioritized implementation velocity and memory safety. Per
spec §31.2: *"Rust is preferred for implementation velocity and memory
safety."* The compiler itself is not user-visible — once the language
self-hosts (post v0.2), the bootstrap host stops mattering.

## Why `.sd`?

Short, unambiguous, and the spec recommends it. The avoided acronym is
`SDL`, which already means Simple DirectMedia Layer.

## Why `mty` and not `mighty`?

A short binary name pays for itself every time you type it. `mty` is
unambiguous and consistent with the other ecosystem identifiers
(`mighty.toml`, `.sd`, `mty pkg`).

## What works today (slice 1)?

- Lexing, parsing, CST and AST views.
- HIR lowering with desugarings.
- Diagnostics rendered through ariadne.
- The `mty` CLI with `new`, `fmt`, `check`, `dump`.
- 20 canonical example programs.

## What does *not* work today?

- Type checking, borrow checking, effect/capability checking — none of
  them are implemented yet.
- Codegen — no LLVM, no Cranelift, no Wasm backend.
- The runtime — no scheduler, no mailboxes, no supervisors.
- `mty build`, `run`, `test`, `lint`, `doc`, `bench`, `pkg`, `lsp` —
  these CLI commands are spec'd but not implemented.

See the [roadmap](https://github.com/hassard0/Mighty#roadmap) and [SLICE1.md](https://github.com/hassard0/Mighty/blob/main/SLICE1.md).

## Why doesn't `mty check` catch this obviously wrong program?

In slice 1, `check` only catches lexical, syntactic, and HIR-lowering
errors. Type errors, borrow violations, missing effects, and capability
misuse all pass through. They are caught in slices 3–5.

## Why are generics in square brackets?

To keep parsing unambiguous with comparison operators and to remove the
need for the turbofish. `Vec[Str]` is unambiguous; `Vec<Str>` requires
lookahead in some contexts.

## What's the difference between an agent and an Erlang process?

Conceptually very similar — isolated state, mailbox-driven, supervised
by a tree. The main differences are static typing (protocols),
capability-based authority (no ambient I/O), and compile-time visible
effects.

## What's the difference between an agent and an actor?

Mighty agents have all four: isolation, asynchrony, typed protocols,
and capability boundaries. Most actor libraries pick two or three.
Spec §2.3 lists the full set: "isolated state owner, concurrency unit,
failure boundary, capability boundary, observability boundary,
scheduling boundary."

## Can I use Mighty for embedded or kernel work?

That is the `core` profile. It forbids global GC, dynamic dispatch by
default, and a managed heap. Slice 1 does not yet enforce the profile
constraints, but the design is targeted from day one.

## Where do I report bugs or request features?

Open an issue on
[github.com/hassard0/Mighty](https://github.com/hassard0/Mighty)
using one of the templates in `.github/ISSUE_TEMPLATE/`.

## How can I help?

See [contributing.md](contributing.md). Tests, examples, parser fixes,
and docs are always welcome.
