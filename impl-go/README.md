# Mighty: Go 3rd-party implementation

A pure-Go lexer + parser for [Mighty](https://github.com/hassard0/Mighty),
built from the v1.0-RC3 normative spec alone — no Rust, Python, or
selfhost source peeking. This is the 3rd independent implementation
the v1.0 spec-freeze process requires (after the Rust toolchain and
the Python impl in `impl-py/`).

**Status:** PARTIAL — v0.12 shipped the source code (~4800 LOC across
lexer + parser + tests). README, notes, and CI workflow landed
later in cleanup. Not yet cross-validated against the Python impl
on all 20 examples; that's a v0.13 task.

## Run

Requires Go 1.22+.

```bash
cd impl-go
go test ./...
go run ./cmd/mty-go lex   ../examples/01_hello.mty
go run ./cmd/mty-go parse ../examples/01_hello.mty
```

## Layout

```
impl-go/
├── go.mod
├── mty/
│   ├── lexer.go
│   ├── lexer_test.go
│   ├── parser.go               # ~2800 LOC; recursive descent + Pratt
│   ├── parser_test.go
│   ├── examples_test.go        # sweep examples/*.mty
│   └── diagnostics.go
└── cmd/mty-go/main.go
```

## What's covered

- §3 lexical: full token surface (literals, keywords, contextual
  keywords, comments, punctuation)
- §4 syntactic: top-level items (fn, struct, enum, type, use,
  mod, const, impl, trait, extern, macro), expressions (Pratt
  precedence per §11.1.1), patterns, types
- Likely-deferred per the scope-tightness: agents/protocols/
  supervisors/budgets/arenas/sandboxes/macros body parsing (cf.
  Python impl which captures them as opaque blocks)

## License

MIT — see [LICENSE](../LICENSE).
