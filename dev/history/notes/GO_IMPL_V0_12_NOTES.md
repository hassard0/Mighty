# Go 3rd-impl — v0.12 swarm notes

## What landed

A Go 1.22+ lexer + parser for Mighty, written from v1.0-RC3 alone:

- `impl-go/mty/lexer.go` — full token surface
- `impl-go/mty/parser.go` — ~2800 LOC, recursive descent with Pratt
- `impl-go/mty/{lexer,parser,examples}_test.go` — unit tests + example sweep
- `impl-go/cmd/mty-go/main.go` — CLI: `mty-go lex|parse <file>`
- `impl-go/go.mod` — module `github.com/hassard0/mighty-impl-go`

**Total: 7 files, ~4848 LOC.**

## Status

The Go implementation agent shipped source code matching the briefed
scope (lexer + parser + tests). It did NOT land a README, CI
workflow, or this findings notes file — they were added in cleanup
when the v0.12 integrator picked up the WIP.

Since the on-disk machine running these swarm builds doesn't have a
Go toolchain installed (`go: command not found`), `go test ./...`
was not run as part of the swarm verification. The structural
inventory (file layout, LOC counts) is the only check applied.

## What's pending for v0.13

1. **Run `go test ./...`** on a host with Go 1.22+ to confirm the
   tests actually pass.
2. **Add `.github/workflows/go-impl.yml`** — mirror the Python
   workflow shape; trigger on push/PR to `impl-go/**` or
   `docs/spec/**`.
3. **Cross-validation pass:** for each example in `examples/`,
   compare the Go impl's lex + parse output against the Python impl
   (`impl-py/`) and the Rust impl (via `mty dump --cst`). Document
   divergences in this file. Any 3-way agreement is high-confidence
   spec validation; 2-of-3 marks a probable spec ambiguity.
4. **Findings from spec implementation:** record any v1.0-RC3
   ambiguities the agent surfaced (analogous to Python impl's 16
   findings). These should land alongside the Python findings in
   `docs/spec/v1.0-rc.md` for v1.0-RC4.

## Independence rationale

This is the third spec-driven implementation. The spec-freeze process
treats 2nd impls as the bar; a 3rd impl strengthens the evidence
that the spec is unambiguous enough to implement from prose alone.

The agent's brief required zero peeking at `crates/mty-*` source,
`selfhost/`, and `impl-py/`. Audit-trail for that discipline lives in
the commit message (forthcoming with this commit).
