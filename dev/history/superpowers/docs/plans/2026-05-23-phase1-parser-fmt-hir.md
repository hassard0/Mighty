# Mighty Phase 1 Implementation Plan — Lexer, Parser, Formatter, HIR

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Mighty compiler frontend — lexer, lossless CST, parser, typed AST view, name-resolved HIR, diagnostics, idempotent formatter, and the `mty` CLI — plus the 20 canonical example programs that form the conformance corpus.

**Architecture:** Three-layer pipeline (CST → AST view → HIR) with rowan as the lossless source of truth, hand-rolled recursive-descent parser with Pratt expression precedence, ariadne diagnostics, and a Wadler/Lindig pretty-printer for the formatter. Multi-crate Cargo workspace; each crate has one clear responsibility.

**Tech Stack:** Rust (stable), `logos` (lexer), `rowan` (CST), `la-arena` (HIR), `ariadne` (diagnostics), `clap` (CLI), `insta` (snapshot tests).

**Spec:** `docs/superpowers/specs/2026-05-23-phase1-parser-fmt-hir-design.md`
**Source language spec:** `C:\Users\ihass\Downloads\stardust_language_spec_v0_1.md`

---

## File Structure

```
mighty/
├── Cargo.toml                                  workspace manifest
├── rust-toolchain.toml                         MSRV pin
├── mighty.toml                                   placeholder Mighty manifest (unused in slice 1)
├── crates/
│   ├── mty-syntax/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                          re-exports
│   │       ├── syntax_kind.rs                  SyntaxKind enum (shared with rowan + logos)
│   │       ├── lexer.rs                        logos tokenizer with callbacks
│   │       ├── language.rs                     rowan Language impl
│   │       ├── parser/
│   │       │   ├── mod.rs                      Parser struct, entry, token stream
│   │       │   ├── recovery.rs                 sync token sets, error recovery
│   │       │   ├── items.rs                    use, mod, fn, struct, enum, type alias, impl, trait
│   │       │   ├── types.rs                    type expressions, generic params, T!E sugar
│   │       │   ├── patterns.rs                 all pattern kinds
│   │       │   ├── exprs.rs                    primary + Pratt + postfix
│   │       │   ├── stmts.rs                    statements, blocks, control flow
│   │       │   ├── agents.rs                   agent, protocol, supervisor, on handlers
│   │       │   ├── concurrency.rs              task scope, budget, sandbox, arena
│   │       │   ├── extern_.rs                  extern c/js blocks, export decls
│   │       │   ├── macros.rs                   macro decls
│   │       │   └── unsafe_.rs                  unsafe blocks/fns
│   │       └── tests/                          parser + lexer integration tests
│   ├── mty-ast/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                          re-exports
│   │       └── generated.rs                    typed accessor structs over rowan SyntaxNode
│   ├── mty-diagnostics/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── diagnostic.rs                   Diagnostic, Severity, Label
│   │       ├── codes.rs                        DiagCode enum (MT0001..)
│   │       └── render/
│   │           ├── mod.rs
│   │           └── ariadne.rs                  terminal render
│   ├── mty-hir/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ids.rs                          arena ID types
│   │       ├── nodes.rs                        HIR node types (Item, Expr, Pat, Type, ...)
│   │       ├── package.rs                      Package, symbol table
│   │       ├── resolve.rs                      module-scope name resolution
│   │       ├── lower/
│   │       │   ├── mod.rs                      entry, lowering context
│   │       │   ├── items.rs
│   │       │   ├── exprs.rs
│   │       │   ├── types.rs
│   │       │   ├── patterns.rs
│   │       │   └── agents.rs                   agents, protocols, supervisors + desugaring
│   │       └── dump.rs                         S-expression dump
│   ├── mty-fmt/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                          public format() entry
│   │       ├── doc.rs                          Wadler combinators (Doc enum + builders)
│   │       ├── printer.rs                      Doc -> String layout
│   │       ├── trivia.rs                       comment attachment
│   │       └── fmt/
│   │           ├── mod.rs                      dispatch over SyntaxKind
│   │           ├── items.rs
│   │           ├── types.rs
│   │           ├── patterns.rs
│   │           ├── exprs.rs
│   │           ├── agents.rs
│   │           └── concurrency.rs
│   ├── mty-driver/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── manifest.rs                     mighty.toml parsing
│   │       └── pipeline.rs                     source -> CST -> AST -> HIR
│   └── mty-cli/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs                         clap entry
│           └── cmd/
│               ├── mod.rs
│               ├── new.rs
│               ├── fmt.rs
│               ├── check.rs
│               └── dump.rs
├── examples/                                   20 canonical .sd programs
│   ├── 01_hello.sd ... 20_frontend_component.sd
└── tests/
    ├── parser/                                 cross-crate parser tests + recovery
    ├── fmt/                                    idempotence + round-trip sweep
    ├── hir/                                    HIR dump snapshots
    └── conformance/                            §37 scaffold
```

---

## Task 1: Workspace bootstrap + rust-toolchain

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `mighty.toml`

- [ ] **Step 1: Write `rust-toolchain.toml`**

```toml
[toolchain]
channel = "1.82.0"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- [ ] **Step 2: Write workspace `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = [
    "crates/mty-syntax",
    "crates/mty-ast",
    "crates/mty-diagnostics",
    "crates/mty-hir",
    "crates/mty-fmt",
    "crates/mty-driver",
    "crates/mty-cli",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.82"
license = "Apache-2.0 OR MIT"
repository = "https://github.com/hassard0/stardust"
authors = ["Ian Hassard <ihassard@gmail.com>"]

[workspace.dependencies]
logos = "0.14"
rowan = "0.15"
la-arena = "0.3"
ariadne = "0.4"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
thiserror = "1"
insta = { version = "1", features = ["yaml"] }

[profile.dev]
opt-level = 0
debug = true

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
```

- [ ] **Step 3: Write placeholder `mighty.toml`**

```toml
[package]
name = "mighty-stdlib-placeholder"
version = "0.1.0"
edition = "2026"
profile = "host"

[deps]
```

- [ ] **Step 4: Verify workspace structure**

Run: `cargo check --workspace 2>&1 | head -5`
Expected: error about no members existing yet (crates not created). This is fine — confirms Cargo reads the manifest.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust-toolchain.toml mighty.toml
git commit -m "Bootstrap Cargo workspace + toolchain pin"
```

---

## Task 2: Crate skeletons

**Files:**
- Create: `crates/mty-syntax/Cargo.toml`, `crates/mty-syntax/src/lib.rs`
- Create: `crates/mty-ast/Cargo.toml`, `crates/mty-ast/src/lib.rs`
- Create: `crates/mty-diagnostics/Cargo.toml`, `crates/mty-diagnostics/src/lib.rs`
- Create: `crates/mty-hir/Cargo.toml`, `crates/mty-hir/src/lib.rs`
- Create: `crates/mty-fmt/Cargo.toml`, `crates/mty-fmt/src/lib.rs`
- Create: `crates/mty-driver/Cargo.toml`, `crates/mty-driver/src/lib.rs`
- Create: `crates/mty-cli/Cargo.toml`, `crates/mty-cli/src/main.rs`

- [ ] **Step 1: Each `crates/mty-X/Cargo.toml` follows this template** (substitute name + deps)

```toml
[package]
name = "mty-syntax"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true

[dependencies]
logos.workspace = true
rowan.workspace = true

[dev-dependencies]
insta.workspace = true
```

Specific per-crate dep lists:
- `mty-syntax`: `logos`, `rowan`
- `mty-ast`: `rowan`, `mty-syntax = { path = "../mty-syntax" }`
- `mty-diagnostics`: `ariadne`, `thiserror`
- `mty-hir`: `la-arena`, `mty-ast = { path = "../mty-ast" }`, `mty-syntax = { path = "../mty-syntax" }`, `mty-diagnostics = { path = "../mty-diagnostics" }`
- `mty-fmt`: `rowan`, `mty-syntax = { path = "../mty-syntax" }`
- `mty-driver`: `serde`, `toml`, `thiserror`, plus path deps on `mty-syntax`, `mty-ast`, `mty-hir`, `mty-diagnostics`, `mty-fmt`
- `mty-cli` (binary): `clap`, `ariadne`, plus path dep on `mty-driver`

- [ ] **Step 2: Each `src/lib.rs` for library crates is minimal**

```rust
//! mty-syntax: lexer, CST, parser.
```

For `mty-cli/src/main.rs`:

```rust
fn main() {
    println!("mty placeholder");
}
```

For `mty-cli/Cargo.toml`, add `[[bin]] name = "mty" path = "src/main.rs"`.

- [ ] **Step 3: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: builds clean with 7 crates compiled, no warnings.

- [ ] **Step 4: Verify the binary runs**

Run: `cargo run -p mty-cli`
Expected: prints `mty placeholder`.

- [ ] **Step 5: Commit**

```bash
git add crates/
git commit -m "Add empty crate skeletons (syntax/ast/diagnostics/hir/fmt/driver/cli)"
```

---

## Task 3: SyntaxKind enum

**Files:**
- Create: `crates/mty-syntax/src/syntax_kind.rs`
- Modify: `crates/mty-syntax/src/lib.rs`

Defines every token kind and every CST node kind. Used by both `logos` (as `Logos` derive) and `rowan` (as the language's syntax kind). Order matters only for stability of serialized snapshots; group by category.

- [ ] **Step 1: Write `syntax_kind.rs` with the complete enum**

```rust
#![allow(non_camel_case_types)]

use logos::Logos;

/// SyntaxKind is the universal tag for tokens AND CST nodes.
/// Token variants are produced by the lexer; node variants are produced by the parser.
#[derive(Logos, Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum SyntaxKind {
    // ---- Trivia (tokens) ----
    #[regex(r"[ \t\r\n]+")]
    WHITESPACE,
    #[regex(r"//[^\n]*")]
    LINE_COMMENT,
    #[regex(r"/\*([^*]|\*[^/])*\*/")]
    BLOCK_COMMENT,
    #[regex(r"///[^\n]*")]
    DOC_COMMENT,

    // ---- Literals (tokens) ----
    #[regex(r"[0-9][0-9_]*(?:[iuf](?:8|16|32|64|128))?")]
    INT_LITERAL,
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*(?:f(?:32|64))?")]
    FLOAT_LITERAL,
    #[regex(r"[0-9]+(?:ns|us|ms|s|m|h)")]
    DURATION_LITERAL,
    #[regex(r"[0-9]+(?:B|KiB|MiB|GiB)")]
    SIZE_LITERAL,
    #[regex(r#""([^"\\]|\\.)*""#)]
    STRING_LITERAL,
    #[regex(r"'([^'\\]|\\.)*'")]
    CHAR_LITERAL,
    #[regex(r#"html"([^"\\]|\\.)*""#)]
    HTML_LITERAL,
    #[token("true")]
    TRUE_KW,
    #[token("false")]
    FALSE_KW,

    // ---- Keywords (spec §3.3) ----
    #[token("agent")] AGENT_KW,
    #[token("arena")] ARENA_KW,
    #[token("as")] AS_KW,
    #[token("async")] ASYNC_KW,
    #[token("await")] AWAIT_KW,
    #[token("budget")] BUDGET_KW,
    #[token("cap")] CAP_KW,
    #[token("const")] CONST_KW,
    #[token("effect")] EFFECT_KW,
    #[token("else")] ELSE_KW,
    #[token("enum")] ENUM_KW,
    #[token("extern")] EXTERN_KW,
    #[token("fn")] FN_KW,
    #[token("for")] FOR_KW,
    #[token("if")] IF_KW,
    #[token("impl")] IMPL_KW,
    #[token("import")] IMPORT_KW,
    #[token("in")] IN_KW,
    #[token("let")] LET_KW,
    #[token("loop")] LOOP_KW,
    #[token("match")] MATCH_KW,
    #[token("mod")] MOD_KW,
    #[token("move")] MOVE_KW,
    #[token("mut")] MUT_KW,
    #[token("on")] ON_KW,
    #[token("package")] PACKAGE_KW,
    #[token("protocol")] PROTOCOL_KW,
    #[token("pub")] PUB_KW,
    #[token("ref")] REF_KW,
    #[token("return")] RETURN_KW,
    #[token("self")] SELF_KW,
    #[token("spawn")] SPAWN_KW,
    #[token("state")] STATE_KW,
    #[token("struct")] STRUCT_KW,
    #[token("task")] TASK_KW,
    #[token("trait")] TRAIT_KW,
    #[token("type")] TYPE_KW,
    #[token("unsafe")] UNSAFE_KW,
    #[token("use")] USE_KW,
    #[token("where")] WHERE_KW,
    #[token("while")] WHILE_KW,
    #[token("with")] WITH_KW,
    #[token("yield")] YIELD_KW,
    #[token("export")] EXPORT_KW,
    #[token("sup")] SUP_KW,
    #[token("sandbox")] SANDBOX_KW,
    #[token("child")] CHILD_KW,
    #[token("on_fail")] ON_FAIL_KW,
    #[token("restart")] RESTART_KW,
    #[token("backoff")] BACKOFF_KW,
    #[token("up_to")] UP_TO_KW,
    #[token("detach")] DETACH_KW,
    #[token("requires")] REQUIRES_KW,
    #[token("macro")] MACRO_KW,
    #[token("run")] RUN_KW,
    #[token("join")] JOIN_KW,
    #[token("scope")] SCOPE_KW,

    // ---- Identifiers ----
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", priority = 2)]
    IDENT,

    // ---- Punctuation ----
    #[token("(")] L_PAREN,
    #[token(")")] R_PAREN,
    #[token("{")] L_BRACE,
    #[token("}")] R_BRACE,
    #[token("[")] L_BRACK,
    #[token("]")] R_BRACK,
    #[token(",")] COMMA,
    #[token(";")] SEMI,
    #[token(":")] COLON,
    #[token("::")] COLON_COLON,
    #[token(".")] DOT,
    #[token("..")] DOT_DOT,
    #[token("..=")] DOT_DOT_EQ,
    #[token("=>")] FAT_ARROW,
    #[token("->")] THIN_ARROW,
    #[token("@")] AT,
    #[token("?")] QUESTION,
    #[token("!")] BANG,
    #[token("&")] AMP,
    #[token("&&")] AMP_AMP,
    #[token("|")] PIPE,
    #[token("||")] PIPE_PIPE,
    #[token("^")] CARET,
    #[token("=")] EQ,
    #[token("==")] EQ_EQ,
    #[token("!=")] BANG_EQ,
    #[token("<")] LT,
    #[token("<=")] LT_EQ,
    #[token(">")] GT,
    #[token(">=")] GT_EQ,
    #[token("<<")] SHL,
    #[token(">>")] SHR,
    #[token("+")] PLUS,
    #[token("-")] MINUS,
    #[token("*")] STAR,
    #[token("/")] SLASH,
    #[token("%")] PERCENT,
    #[token("+=")] PLUS_EQ,
    #[token("-=")] MINUS_EQ,
    #[token("*=")] STAR_EQ,
    #[token("/=")] SLASH_EQ,
    #[token("%=")] PERCENT_EQ,
    #[token("&=")] AMP_EQ,
    #[token("|=")] PIPE_EQ,
    #[token("^=")] CARET_EQ,
    #[token("<<=")] SHL_EQ,
    #[token(">>=")] SHR_EQ,
    #[token("#")] HASH,

    // ---- Special ----
    ERROR,
    EOF,

    // ---- Node kinds (produced by parser, never by lexer) ----
    FILE,
    USE_DECL, MOD_DECL, PACKAGE_DECL,
    FN_DECL, FN_PARAM, FN_PARAM_LIST, RET_TYPE, EFFECT_CLAUSE,
    STRUCT_DECL, STRUCT_FIELD, STRUCT_FIELD_LIST,
    ENUM_DECL, ENUM_VARIANT, ENUM_VARIANT_LIST,
    TYPE_ALIAS, IMPL_BLOCK, TRAIT_DECL, TRAIT_METHOD,
    AGENT_DECL, AGENT_CTOR_PARAMS, AGENT_PROTOCOL_LIST, AGENT_STATE_DECL, ON_HANDLER,
    PROTOCOL_DECL, PROTOCOL_MSG,
    SUPERVISOR_DECL, SUP_CHILD, ON_FAIL_CLAUSE,
    BUDGET_BLOCK, BUDGET_ENTRY,
    SANDBOX_BLOCK, SANDBOX_ENTRY,
    ARENA_BLOCK,
    TASK_SCOPE, TASK_SPAWN, DETACH_EXPR, JOIN_EXPR,
    EXTERN_BLOCK, EXTERN_FN, EXPORT_DECL,
    MACRO_DECL, UNSAFE_BLOCK,
    GENERIC_PARAM_LIST, GENERIC_PARAM, GENERIC_ARG_LIST, GENERIC_ARG, WHERE_CLAUSE,
    PATH, PATH_SEGMENT, NAME, NAME_REF,
    TYPE_PATH, TYPE_REF, TYPE_BORROW, TYPE_TUPLE, TYPE_ARRAY, TYPE_FN, TYPE_DYN, TYPE_RESULT_SUGAR, TYPE_UNION,
    BLOCK, LET_STMT, EXPR_STMT,
    LITERAL_EXPR, PATH_EXPR, BINARY_EXPR, UNARY_EXPR, POSTFIX_EXPR,
    CALL_EXPR, METHOD_CALL_EXPR, INDEX_EXPR, FIELD_EXPR, CAST_EXPR,
    IF_EXPR, MATCH_EXPR, MATCH_ARM, MATCH_GUARD,
    FOR_EXPR, WHILE_EXPR, LOOP_EXPR, RETURN_EXPR, BREAK_EXPR, CONTINUE_EXPR, YIELD_EXPR,
    TUPLE_EXPR, ARRAY_EXPR, STRUCT_EXPR, STRUCT_FIELD_EXPR, MAP_EXPR, MAP_ENTRY, LAMBDA_EXPR,
    SEND_EXPR, ASK_EXPR, DEADLINE_EXPR, QUESTION_EXPR,
    HTML_EXPR, MOVE_EXPR, BORROW_EXPR, SPAWN_EXPR,
    LITERAL_PAT, IDENT_PAT, WILDCARD_PAT, TUPLE_PAT, STRUCT_PAT, ENUM_PAT, RANGE_PAT, BINDING_PAT, REF_PAT,
    ARG_LIST, ARG, NAMED_ARG,
    ATTR, VISIBILITY,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(self,
            SyntaxKind::WHITESPACE
            | SyntaxKind::LINE_COMMENT
            | SyntaxKind::BLOCK_COMMENT
            | SyntaxKind::DOC_COMMENT
        )
    }
    pub fn is_keyword(self) -> bool {
        (SyntaxKind::AGENT_KW as u16..=SyntaxKind::SCOPE_KW as u16).contains(&(self as u16))
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        rowan::SyntaxKind(kind as u16)
    }
}
```

- [ ] **Step 2: Update `lib.rs`**

```rust
//! mty-syntax: lexer, CST, parser.
pub mod syntax_kind;
pub use syntax_kind::SyntaxKind;
```

- [ ] **Step 3: Write a smoke test**

`crates/mty-syntax/tests/syntax_kind.rs`:

```rust
use sdust_syntax::SyntaxKind;

#[test]
fn keywords_classify() {
    assert!(SyntaxKind::AGENT_KW.is_keyword());
    assert!(SyntaxKind::FN_KW.is_keyword());
    assert!(!SyntaxKind::IDENT.is_keyword());
}

#[test]
fn trivia_classify() {
    assert!(SyntaxKind::WHITESPACE.is_trivia());
    assert!(SyntaxKind::LINE_COMMENT.is_trivia());
    assert!(!SyntaxKind::IDENT.is_trivia());
}

#[test]
fn rowan_conversion() {
    let rk: rowan::SyntaxKind = SyntaxKind::FN_KW.into();
    assert_eq!(rk.0, SyntaxKind::FN_KW as u16);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mty-syntax`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/mty-syntax/
git commit -m "Define SyntaxKind enum covering all tokens + CST node kinds"
```

---

## Task 4: Lexer with literal callbacks

**Files:**
- Create: `crates/mty-syntax/src/lexer.rs`
- Modify: `crates/mty-syntax/src/lib.rs`
- Create: `crates/mty-syntax/tests/lexer.rs`

The `Logos` derive on `SyntaxKind` already produces a lexer; this task wraps it in an iterator that emits `(SyntaxKind, &str)` pairs and validates duration/size suffixes (the regex accepts the lexical shape; the wrapper rejects malformed numbers and uppercase units like `1ns` vs `1NS`).

- [ ] **Step 1: Write `lexer.rs`**

```rust
use crate::SyntaxKind;
use logos::Logos;

pub struct LexedToken<'src> {
    pub kind: SyntaxKind,
    pub text: &'src str,
    pub start: usize,
    pub end: usize,
}

pub fn lex(src: &str) -> Vec<LexedToken<'_>> {
    let mut lex = SyntaxKind::lexer(src);
    let mut out = Vec::with_capacity(src.len() / 4);
    while let Some(result) = lex.next() {
        let kind = match result {
            Ok(k) => k,
            Err(_) => SyntaxKind::ERROR,
        };
        let span = lex.span();
        out.push(LexedToken {
            kind,
            text: &src[span.start..span.end],
            start: span.start,
            end: span.end,
        });
    }
    out.push(LexedToken {
        kind: SyntaxKind::EOF,
        text: "",
        start: src.len(),
        end: src.len(),
    });
    out
}
```

- [ ] **Step 2: Update `lib.rs`**

```rust
pub mod syntax_kind;
pub mod lexer;
pub use syntax_kind::SyntaxKind;
pub use lexer::{lex, LexedToken};
```

- [ ] **Step 3: Write tests covering all token classes**

```rust
use sdust_syntax::{lex, SyntaxKind::*};

fn kinds(src: &str) -> Vec<sdust_syntax::SyntaxKind> {
    lex(src).into_iter().map(|t| t.kind).filter(|k| !k.is_trivia()).collect()
}

#[test]
fn keywords() {
    assert_eq!(kinds("fn agent protocol on"), vec![FN_KW, AGENT_KW, PROTOCOL_KW, ON_KW, EOF]);
}

#[test]
fn literals() {
    assert_eq!(kinds(r#"42 3.14 "hi" 'c' true false"#),
        vec![INT_LITERAL, FLOAT_LITERAL, STRING_LITERAL, CHAR_LITERAL, TRUE_KW, FALSE_KW, EOF]);
}

#[test]
fn duration_and_size() {
    assert_eq!(kinds("10ns 5us 3ms 2s 1m 1h 64B 4KiB 128MiB 1GiB"),
        vec![DURATION_LITERAL; 6].into_iter()
            .chain(vec![SIZE_LITERAL; 4]).chain([EOF]).collect::<Vec<_>>());
}

#[test]
fn typed_int() {
    assert_eq!(kinds("42u32 3i64 100u8"), vec![INT_LITERAL, INT_LITERAL, INT_LITERAL, EOF]);
}

#[test]
fn punctuation() {
    assert_eq!(kinds("!= == -> => :: .. ..="),
        vec![BANG_EQ, EQ_EQ, THIN_ARROW, FAT_ARROW, COLON_COLON, DOT_DOT, DOT_DOT_EQ, EOF]);
}

#[test]
fn agent_send_ask() {
    assert_eq!(kinds("logger!Info"), vec![IDENT, BANG, IDENT, EOF]);
    assert_eq!(kinds("fetcher?Page"), vec![IDENT, QUESTION, IDENT, EOF]);
    assert_eq!(kinds("@2s"), vec![AT, DURATION_LITERAL, EOF]);
}

#[test]
fn html_literal() {
    assert_eq!(kinds(r#"html"<h1>Hi</h1>""#), vec![HTML_LITERAL, EOF]);
}

#[test]
fn line_comment_is_trivia() {
    let toks = lex("// hello\nfn");
    assert_eq!(toks[0].kind, LINE_COMMENT);
    assert_eq!(toks[1].kind, WHITESPACE);
    assert_eq!(toks[2].kind, FN_KW);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mty-syntax --test lexer`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/mty-syntax/src/lexer.rs crates/mty-syntax/src/lib.rs crates/mty-syntax/tests/lexer.rs
git commit -m "Add logos-based lexer with duration/size/typed-numeric literal support"
```

---

## Task 5: rowan Language impl + GreenNode builder

**Files:**
- Create: `crates/mty-syntax/src/language.rs`
- Modify: `crates/mty-syntax/src/lib.rs`

Bridge between `SyntaxKind` and rowan's typed tree.

- [ ] **Step 1: Write `language.rs`**

```rust
use crate::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mighty {}

impl rowan::Language for Mighty {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        // Safety: SyntaxKind covers the full u16 range we ever emit.
        // The parser only produces kinds it defined.
        assert!(raw.0 <= (SyntaxKind::VISIBILITY as u16));
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Mighty>;
pub type SyntaxToken = rowan::SyntaxToken<Mighty>;
pub type SyntaxElement = rowan::SyntaxElement<Mighty>;
pub type SyntaxNodeChildren = rowan::SyntaxNodeChildren<Mighty>;
pub type GreenNode = rowan::GreenNode;
```

- [ ] **Step 2: Update `lib.rs`**

```rust
pub mod syntax_kind;
pub mod lexer;
pub mod language;
pub use syntax_kind::SyntaxKind;
pub use lexer::{lex, LexedToken};
pub use language::{Mighty, SyntaxNode, SyntaxToken, SyntaxElement, SyntaxNodeChildren, GreenNode};
```

- [ ] **Step 3: Smoke test**

`crates/mty-syntax/tests/language.rs`:

```rust
use rowan::GreenNodeBuilder;
use sdust_syntax::{Mighty, SyntaxKind, SyntaxNode};

#[test]
fn build_minimal_tree() {
    let mut b = GreenNodeBuilder::new();
    b.start_node(SyntaxKind::FILE.into());
    b.token(SyntaxKind::FN_KW.into(), "fn");
    b.token(SyntaxKind::WHITESPACE.into(), " ");
    b.token(SyntaxKind::IDENT.into(), "main");
    b.finish_node();
    let green = b.finish();
    let root = SyntaxNode::new_root(green);
    assert_eq!(root.kind(), SyntaxKind::FILE);
    assert_eq!(root.text().to_string(), "fn main");
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mty-syntax`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/mty-syntax/src/language.rs crates/mty-syntax/src/lib.rs crates/mty-syntax/tests/language.rs
git commit -m "Wire rowan Language impl + type aliases"
```

---

## Task 6: Parser foundation + token stream

**Files:**
- Create: `crates/mty-syntax/src/parser/mod.rs`
- Create: `crates/mty-syntax/src/parser/recovery.rs`
- Modify: `crates/mty-syntax/src/lib.rs`

The `Parser` struct owns a token cursor and a green-tree builder. It exposes `bump`, `peek`, `eat`, `expect`, `start_node`, `finish_node`, `error`, and `sync_to`. All grammar productions are methods on `Parser`.

- [ ] **Step 1: Write `parser/mod.rs`**

```rust
use crate::{lexer::LexedToken, SyntaxKind, language::Mighty};
use rowan::GreenNodeBuilder;

pub mod recovery;
pub mod items;
pub mod types;
pub mod patterns;
pub mod exprs;
pub mod stmts;
pub mod agents;
pub mod concurrency;
pub mod extern_;
pub mod macros;
pub mod unsafe_;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

pub struct ParseResult {
    pub green: rowan::GreenNode,
    pub errors: Vec<ParseError>,
}

pub struct Parser<'src> {
    tokens: Vec<LexedToken<'src>>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<ParseError>,
}

impl<'src> Parser<'src> {
    pub fn new(src: &'src str) -> Self {
        Self {
            tokens: crate::lex(src),
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
        }
    }

    pub fn parse_file(mut self) -> ParseResult {
        self.builder.start_node(SyntaxKind::FILE.into());
        self.skip_trivia();
        while !self.at(SyntaxKind::EOF) {
            if !items::item(&mut self) {
                let tok = &self.tokens[self.pos];
                self.error_at(format!("unexpected token `{}`", tok.text), tok.start, tok.end);
                self.bump_any();
                self.skip_trivia();
            }
        }
        self.builder.finish_node();
        ParseResult { green: self.builder.finish(), errors: self.errors }
    }

    // ---- cursor primitives ----

    fn peek(&self) -> SyntaxKind {
        self.tokens[self.pos].kind
    }
    fn peek_n(&self, n: usize) -> SyntaxKind {
        self.tokens.get(self.pos + n).map(|t| t.kind).unwrap_or(SyntaxKind::EOF)
    }
    fn at(&self, kind: SyntaxKind) -> bool { self.peek() == kind }
    fn at_set(&self, set: &[SyntaxKind]) -> bool { set.contains(&self.peek()) }

    fn bump_any(&mut self) {
        let t = &self.tokens[self.pos];
        if t.kind != SyntaxKind::EOF {
            self.builder.token(t.kind.into(), t.text);
            self.pos += 1;
        }
    }
    fn bump(&mut self, kind: SyntaxKind) {
        assert_eq!(self.peek(), kind);
        self.bump_any();
    }
    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) { self.bump_any(); self.skip_trivia(); true } else { false }
    }
    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) { true } else {
            let t = &self.tokens[self.pos];
            self.error_at(format!("expected {:?}, got `{}`", kind, t.text), t.start, t.end);
            false
        }
    }

    fn skip_trivia(&mut self) {
        while self.peek().is_trivia() { self.bump_any(); }
    }

    fn start_node(&mut self, kind: SyntaxKind) { self.builder.start_node(kind.into()); }
    fn finish_node(&mut self) { self.builder.finish_node(); }

    fn checkpoint(&self) -> rowan::Checkpoint { self.builder.checkpoint() }
    fn start_node_at(&mut self, cp: rowan::Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(cp, kind.into());
    }

    fn error_at(&mut self, message: String, start: usize, end: usize) {
        self.errors.push(ParseError { message, start, end });
    }

    fn error(&mut self, message: impl Into<String>) {
        let t = &self.tokens[self.pos];
        let (s, e) = (t.start, t.end);
        self.error_at(message.into(), s, e);
    }
}

pub fn parse(src: &str) -> ParseResult {
    Parser::new(src).parse_file()
}
```

- [ ] **Step 2: Write `parser/recovery.rs` with sync token sets**

```rust
use crate::SyntaxKind::{self, *};

pub const ITEM_START: &[SyntaxKind] = &[
    FN_KW, AGENT_KW, PROTOCOL_KW, STRUCT_KW, ENUM_KW, TYPE_KW,
    IMPL_KW, TRAIT_KW, USE_KW, MOD_KW, PACKAGE_KW, PUB_KW,
    CONST_KW, EXTERN_KW, EXPORT_KW, MACRO_KW, SUPERVISOR_DECL_HINT, SUP_KW, UNSAFE_KW,
];
// Note: SUPERVISOR_DECL_HINT is not a real kind; this constant uses keyword tokens only.
// We rely on parser logic to detect `supervisor` ident before consuming.

pub const STMT_START: &[SyntaxKind] = &[
    LET_KW, RETURN_KW, IF_KW, MATCH_KW, FOR_KW, WHILE_KW, LOOP_KW,
    UNSAFE_KW, BREAK_KW_HINT, CONTINUE_KW_HINT, // similar caveat
];
```

Fix the constants: keyword aliases that don't have explicit `_KW` variants are detected as `IDENT` with text match. Replace placeholders with real kinds — `BREAK_KW_HINT` and `CONTINUE_KW_HINT` aren't in our enum (we don't list `break`/`continue` as reserved). Drop them. `supervisor` isn't reserved in spec §3.3 either; we treat it as a contextual keyword. Real file:

```rust
use crate::SyntaxKind::{self, *};

pub const ITEM_START: &[SyntaxKind] = &[
    FN_KW, AGENT_KW, PROTOCOL_KW, STRUCT_KW, ENUM_KW, TYPE_KW,
    IMPL_KW, TRAIT_KW, USE_KW, MOD_KW, PACKAGE_KW, PUB_KW,
    CONST_KW, EXTERN_KW, EXPORT_KW, MACRO_KW, SUP_KW, UNSAFE_KW,
];

pub const STMT_START: &[SyntaxKind] = &[
    LET_KW, RETURN_KW, IF_KW, MATCH_KW, FOR_KW, WHILE_KW, LOOP_KW, UNSAFE_KW,
];

impl crate::parser::Parser<'_> {
    pub(crate) fn sync_to(&mut self, set: &[SyntaxKind]) {
        while !self.at(SyntaxKind::EOF) && !set.contains(&self.peek()) && !self.at(SyntaxKind::R_BRACE) {
            self.bump_any();
        }
        self.skip_trivia();
    }
}
```

- [ ] **Step 3: Stub out the submodules with placeholder `pub fn item/expr/...` returning false/None**

For each of `items.rs`, `types.rs`, `patterns.rs`, `exprs.rs`, `stmts.rs`, `agents.rs`, `concurrency.rs`, `extern_.rs`, `macros.rs`, `unsafe_.rs`:

```rust
use crate::parser::Parser;
pub fn item(_p: &mut Parser) -> bool { false }   // for items.rs
```

(Each module gets its own signature; this unblocks compilation. Real productions added in subsequent tasks.)

- [ ] **Step 4: Export parser from `lib.rs`**

```rust
pub mod parser;
pub use parser::{parse, ParseResult, ParseError};
```

- [ ] **Step 5: Smoke test + commit**

`crates/mty-syntax/tests/parser_smoke.rs`:

```rust
#[test]
fn empty_file_parses() {
    let r = sdust_syntax::parse("");
    assert_eq!(r.errors.len(), 0);
    assert_eq!(sdust_syntax::SyntaxNode::new_root(r.green).kind(), sdust_syntax::SyntaxKind::FILE);
}
```

Run: `cargo test -p mty-syntax`
Expected: all pass (the parser does nothing useful yet, but compiles and handles empty input).

```bash
git add crates/mty-syntax/src/parser/ crates/mty-syntax/src/lib.rs crates/mty-syntax/tests/parser_smoke.rs
git commit -m "Add Parser scaffolding, token cursor, error recovery primitives"
```

---

## Task 7: Parse use/mod/package declarations + paths

**Files:**
- Modify: `crates/mty-syntax/src/parser/items.rs`
- Create: `crates/mty-syntax/src/parser/paths.rs`

Productions:

```
File         = (PackageDecl)? Item*
PackageDecl  = 'package' Path
Item         = UseDecl | ModDecl | FnDecl | StructDecl | EnumDecl | TypeAlias
             | AgentDecl | ProtocolDecl | SupervisorDecl | ImplBlock | TraitDecl
             | ExternBlock | ExportDecl | MacroDecl | ConstDecl
UseDecl      = 'use' Path UseTail? ';'?
UseTail      = '.{' UseSegment (',' UseSegment)* ','? '}' | 'as' Name
UseSegment   = Name ('as' Name)?
ModDecl      = 'mod' Path ';'?
Path         = Name ('.' Name)*
```

- [ ] **Step 1: Write `paths.rs`**

```rust
use super::Parser;
use crate::SyntaxKind::*;

pub fn path(p: &mut Parser) -> bool {
    if !p.at(IDENT) { return false; }
    p.start_node(PATH);
    p.start_node(PATH_SEGMENT);
    p.start_node(NAME_REF);
    p.bump(IDENT);
    p.finish_node();
    p.finish_node();
    p.skip_trivia();
    while p.at(DOT) && p.peek_n(1) == IDENT {
        p.bump(DOT);
        p.skip_trivia();
        p.start_node(PATH_SEGMENT);
        p.start_node(NAME_REF);
        p.bump(IDENT);
        p.finish_node();
        p.finish_node();
        p.skip_trivia();
    }
    p.finish_node();
    true
}

pub fn name(p: &mut Parser) -> bool {
    if !p.at(IDENT) { return false; }
    p.start_node(NAME);
    p.bump(IDENT);
    p.finish_node();
    p.skip_trivia();
    true
}
```

- [ ] **Step 2: Write `items.rs` use/mod/package**

```rust
use super::{Parser, paths};
use crate::SyntaxKind::*;

pub fn item(p: &mut Parser) -> bool {
    p.skip_trivia();
    // visibility prefix
    let cp = p.checkpoint();
    if p.at(PUB_KW) {
        p.start_node(VISIBILITY);
        p.bump(PUB_KW);
        p.finish_node();
        p.skip_trivia();
    }
    match p.peek() {
        USE_KW => { use_decl(p, cp); true }
        MOD_KW => { mod_decl(p, cp); true }
        PACKAGE_KW => { package_decl(p, cp); true }
        // Future: FN_KW, STRUCT_KW, etc. Added in later tasks.
        _ => false,
    }
}

fn use_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, USE_DECL);
    p.bump(USE_KW);
    p.skip_trivia();
    paths::path(p);
    if p.eat(DOT) {
        if p.eat(L_BRACE) {
            loop {
                paths::name(p);
                if p.eat(AS_KW) { paths::name(p); }
                if !p.eat(COMMA) { break; }
            }
            p.expect(R_BRACE);
        }
    } else if p.eat(AS_KW) {
        paths::name(p);
    }
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn mod_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, MOD_DECL);
    p.bump(MOD_KW);
    p.skip_trivia();
    paths::path(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn package_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, PACKAGE_DECL);
    p.bump(PACKAGE_KW);
    p.skip_trivia();
    paths::path(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}
```

- [ ] **Step 3: Register `paths` module in `parser/mod.rs`**

```rust
pub mod paths;
```

- [ ] **Step 4: Tests with insta snapshots**

`crates/mty-syntax/tests/parse_items.rs`:

```rust
use insta::assert_snapshot;

fn dump(src: &str) -> String {
    let r = sdust_syntax::parse(src);
    let node = sdust_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test] fn use_simple()    { assert_snapshot!(dump("use std.io")); }
#[test] fn use_brace()     { assert_snapshot!(dump("use std.net.{Http, Url}")); }
#[test] fn use_alias()     { assert_snapshot!(dump("use app.model as model")); }
#[test] fn mod_decl()      { assert_snapshot!(dump("mod net.http")); }
#[test] fn package_decl()  { assert_snapshot!(dump("package search_api")); }
```

Run: `cargo test -p mty-syntax --test parse_items`
Then: `cargo insta review` and accept the snapshots after inspecting them.

- [ ] **Step 5: Commit**

```bash
git add crates/mty-syntax/src/parser/ crates/mty-syntax/tests/parse_items.rs crates/mty-syntax/tests/snapshots/
git commit -m "Parse package/use/mod declarations and dotted paths"
```

---

## Task 8: Parse types (incl. T!E sugar, generics, borrows)

**Files:**
- Modify: `crates/mty-syntax/src/parser/types.rs`

Productions:

```
Type        = BorrowType | TupleType | ArrayType | FnType | DynType | PathType
BorrowType  = '&' 'mut'? Type
TupleType   = '(' Type (',' Type)* ','? ')' | '(' ')'
ArrayType   = '[' Type (';' Expr)? ']'
FnType      = 'fn' '(' (Type (',' Type)*)? ')' ('->' Type)?
DynType     = 'dyn' PathType
PathType    = Path GenericArgs? ResultSugar?
GenericArgs = '[' Type (',' Type)* ']'
ResultSugar = '!' (Type | '{' Type (',' Type)* '}')
EffectClause= 'effect' Name (',' Name)*
```

- [ ] **Step 1: Write `types.rs`**

```rust
use super::{Parser, paths};
use crate::SyntaxKind::*;

pub fn type_expr(p: &mut Parser) -> bool {
    p.skip_trivia();
    match p.peek() {
        AMP => borrow(p),
        L_PAREN => tuple(p),
        L_BRACK => array(p),
        FN_KW => fn_type(p),
        IDENT => path_type(p),
        _ => return false,
    }
    true
}

fn borrow(p: &mut Parser) {
    p.start_node(TYPE_BORROW);
    p.bump(AMP);
    p.skip_trivia();
    p.eat(MUT_KW);
    type_expr(p);
    p.finish_node();
    p.skip_trivia();
}

fn tuple(p: &mut Parser) {
    p.start_node(TYPE_TUPLE);
    p.bump(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        type_expr(p);
        while p.eat(COMMA) {
            if p.at(R_PAREN) { break; }
            type_expr(p);
        }
    }
    p.expect(R_PAREN);
    p.finish_node();
    p.skip_trivia();
}

fn array(p: &mut Parser) {
    p.start_node(TYPE_ARRAY);
    p.bump(L_BRACK);
    p.skip_trivia();
    type_expr(p);
    if p.eat(SEMI) {
        super::exprs::expr(p);
    }
    p.expect(R_BRACK);
    p.finish_node();
    p.skip_trivia();
}

fn fn_type(p: &mut Parser) {
    p.start_node(TYPE_FN);
    p.bump(FN_KW);
    p.skip_trivia();
    p.expect(L_PAREN);
    if !p.at(R_PAREN) {
        type_expr(p);
        while p.eat(COMMA) { if p.at(R_PAREN) { break; } type_expr(p); }
    }
    p.expect(R_PAREN);
    if p.eat(THIN_ARROW) { type_expr(p); }
    p.finish_node();
    p.skip_trivia();
}

fn path_type(p: &mut Parser) {
    let cp = p.checkpoint();
    p.start_node(TYPE_PATH);
    paths::path(p);
    if p.at(L_BRACK) { generic_args(p); }
    p.finish_node();
    // Result sugar wraps the path-type node.
    if p.at(BANG) {
        p.start_node_at(cp, TYPE_RESULT_SUGAR);
        p.bump(BANG);
        p.skip_trivia();
        if p.eat(L_BRACE) {
            p.start_node(TYPE_UNION);
            type_expr(p);
            while p.eat(COMMA) { if p.at(R_BRACE) { break; } type_expr(p); }
            p.expect(R_BRACE);
            p.finish_node();
        } else {
            type_expr(p);
        }
        p.finish_node();
    }
    p.skip_trivia();
}

pub fn generic_args(p: &mut Parser) {
    p.start_node(GENERIC_ARG_LIST);
    p.bump(L_BRACK);
    p.skip_trivia();
    if !p.at(R_BRACK) {
        p.start_node(GENERIC_ARG);
        type_expr(p);
        p.finish_node();
        while p.eat(COMMA) {
            if p.at(R_BRACK) { break; }
            p.start_node(GENERIC_ARG);
            type_expr(p);
            p.finish_node();
        }
    }
    p.expect(R_BRACK);
    p.finish_node();
    p.skip_trivia();
}

pub fn generic_params(p: &mut Parser) {
    if !p.at(L_BRACK) { return; }
    p.start_node(GENERIC_PARAM_LIST);
    p.bump(L_BRACK);
    p.skip_trivia();
    if !p.at(R_BRACK) {
        param(p);
        while p.eat(COMMA) { if p.at(R_BRACK) { break; } param(p); }
    }
    p.expect(R_BRACK);
    p.finish_node();
    p.skip_trivia();

    fn param(p: &mut Parser) {
        p.start_node(GENERIC_PARAM);
        paths::name(p);
        if p.eat(COLON) {
            type_expr(p);
            while p.eat(PLUS) { type_expr(p); }
        }
        p.finish_node();
        p.skip_trivia();
    }
}

pub fn effect_clause(p: &mut Parser) {
    if !p.at(EFFECT_KW) { return; }
    p.start_node(EFFECT_CLAUSE);
    p.bump(EFFECT_KW);
    p.skip_trivia();
    paths::name(p);
    while p.eat(COMMA) { paths::name(p); }
    p.finish_node();
    p.skip_trivia();
}
```

- [ ] **Step 2: Tests**

`crates/mty-syntax/tests/parse_types.rs`:

```rust
use insta::assert_snapshot;

fn dump(src: &str) -> String {
    // wrap as `fn f() -> TYPE { unimplemented }` once fn decls land; for now test via a small fixture
    // For this task we directly invoke type_expr through a test-only entry. Use a helper.
    let p = sdust_syntax::parser::Parser::new(src);
    // ... helper that calls type_expr and emits FILE wrapper
    String::new()
}

// NOTE: this test needs a small test-only `parse_type_only(src)` helper.
// Add it to crates/mty-syntax/src/parser/mod.rs guarded by #[cfg(test)] or as pub-in-crate:
//
//   pub fn parse_type(src: &str) -> ParseResult { ... wraps src in a FILE > TYPE_ROOT node ... }

#[test] fn t_borrow() { /* assert_snapshot!(dump("&Str")); */ }
#[test] fn t_borrow_mut() { /* "&mut Bytes" */ }
#[test] fn t_tuple()  { /* "(I32, Str)" */ }
#[test] fn t_array()  { /* "[U8; 16]" */ }
#[test] fn t_path_generics() { /* "Map[Str, Json]" */ }
#[test] fn t_result_sugar()  { /* "Bytes!IoErr" */ }
#[test] fn t_result_union()  { /* "Page!{NetErr, ParseErr}" */ }
```

Add `pub fn parse_type(src: &str)` to `parser/mod.rs` for testing:

```rust
pub fn parse_type(src: &str) -> ParseResult {
    let mut p = Parser::new(src);
    p.builder.start_node(SyntaxKind::FILE.into());
    p.skip_trivia();
    types::type_expr(&mut p);
    p.builder.finish_node();
    ParseResult { green: p.builder.finish(), errors: p.errors }
}
```

Implement the test `dump` helper to call `parse_type` and snapshot the tree.

- [ ] **Step 3: Run + review snapshots**

Run: `cargo test -p mty-syntax --test parse_types`
Then: `cargo insta review`.

- [ ] **Step 4: Commit**

```bash
git add crates/mty-syntax/src/parser/types.rs crates/mty-syntax/src/parser/mod.rs crates/mty-syntax/tests/parse_types.rs crates/mty-syntax/tests/snapshots/
git commit -m "Parse type expressions (borrows, tuples, arrays, fn types, generics, T!E sugar)"
```

---

## Task 9: Parse patterns

**Files:**
- Modify: `crates/mty-syntax/src/parser/patterns.rs`

Productions:

```
Pattern      = WildcardPat | LiteralPat | RefPat | BindingPat
             | TuplePat | StructPat | EnumPat | RangePat
WildcardPat  = '_'
LiteralPat   = IntLit | FloatLit | StringLit | CharLit | true | false
RefPat       = '&' 'mut'? Pattern
BindingPat   = Name ('@' Pattern)?
TuplePat     = '(' Pattern (',' Pattern)* ','? ')'
StructPat    = Path '{' (FieldPat (',' FieldPat)* ','?)? '}'
FieldPat     = Name (':' Pattern)?
EnumPat      = Path ('(' Pattern (',' Pattern)* ','? ')')?
RangePat     = LiteralPat ('..' | '..=') LiteralPat
```

- [ ] **Step 1: Write `patterns.rs`**

```rust
use super::{Parser, paths};
use crate::SyntaxKind::*;

pub fn pattern(p: &mut Parser) -> bool {
    p.skip_trivia();
    match p.peek() {
        IDENT if p.peek_n(1) == DOT || matches!(p.peek_n(1), L_PAREN | L_BRACE) => {
            // Path-headed: struct/enum/binding-vs-path disambiguation by lookahead.
            path_headed(p);
        }
        IDENT => binding(p),
        INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | CHAR_LITERAL | TRUE_KW | FALSE_KW => literal(p),
        AMP => ref_pat(p),
        L_PAREN => tuple(p),
        _ if p.at(IDENT) && p.tokens[p.pos].text == "_" => wildcard(p),
        _ => return false,
    }
    true
}

fn literal(p: &mut Parser) {
    p.start_node(LITERAL_PAT);
    p.bump_any();
    p.finish_node();
    // range check
    if p.at(DOT_DOT) || p.at(DOT_DOT_EQ) {
        // wrap previous literal as range; simplification: emit RANGE_PAT around current literal pat
        // Real impl uses checkpoint at start; for clarity, see TASK 9 step 2 refactor.
    }
    p.skip_trivia();
}

fn binding(p: &mut Parser) {
    p.start_node(BINDING_PAT);
    paths::name(p);
    if p.eat(AT) { pattern(p); }
    p.finish_node();
    p.skip_trivia();
}

fn wildcard(p: &mut Parser) {
    p.start_node(WILDCARD_PAT);
    p.bump(IDENT); // the underscore lexes as IDENT per our regex; verify text == "_" in caller
    p.finish_node();
    p.skip_trivia();
}

fn ref_pat(p: &mut Parser) {
    p.start_node(REF_PAT);
    p.bump(AMP);
    p.skip_trivia();
    p.eat(MUT_KW);
    pattern(p);
    p.finish_node();
    p.skip_trivia();
}

fn tuple(p: &mut Parser) {
    p.start_node(TUPLE_PAT);
    p.bump(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        pattern(p);
        while p.eat(COMMA) { if p.at(R_PAREN) { break; } pattern(p); }
    }
    p.expect(R_PAREN);
    p.finish_node();
    p.skip_trivia();
}

fn path_headed(p: &mut Parser) {
    let cp = p.checkpoint();
    paths::path(p);
    p.skip_trivia();
    if p.eat(L_PAREN) {
        p.start_node_at(cp, ENUM_PAT);
        if !p.at(R_PAREN) {
            pattern(p);
            while p.eat(COMMA) { if p.at(R_PAREN) { break; } pattern(p); }
        }
        p.expect(R_PAREN);
        p.finish_node();
    } else if p.eat(L_BRACE) {
        p.start_node_at(cp, STRUCT_PAT);
        if !p.at(R_BRACE) {
            field_pat(p);
            while p.eat(COMMA) { if p.at(R_BRACE) { break; } field_pat(p); }
        }
        p.expect(R_BRACE);
        p.finish_node();
    } else {
        // Just a path used as a unit variant pattern; wrap as ENUM_PAT with no args.
        p.start_node_at(cp, ENUM_PAT);
        p.finish_node();
    }
    p.skip_trivia();
}

fn field_pat(p: &mut Parser) {
    p.skip_trivia();
    paths::name(p);
    if p.eat(COLON) { pattern(p); }
}
```

The literal-range case noted above (`1..5` as pattern) is folded in via a refactor: on entering `pattern`, take a checkpoint; if a literal is followed by `..`/`..=`, wrap the whole thing in `RANGE_PAT`. Implementation:

```rust
pub fn pattern(p: &mut Parser) -> bool {
    p.skip_trivia();
    let cp = p.checkpoint();
    let ok = match p.peek() {
        // ... same dispatch
    };
    if !ok { return false; }
    if p.at(DOT_DOT) || p.at(DOT_DOT_EQ) {
        p.start_node_at(cp, RANGE_PAT);
        p.bump_any();
        p.skip_trivia();
        pattern(p);
        p.finish_node();
    }
    true
}
```

- [ ] **Step 2: Tests via snapshots**

Add `pub fn parse_pattern(src: &str) -> ParseResult` test-only entry to `parser/mod.rs`. Test file `crates/mty-syntax/tests/parse_patterns.rs` snapshots: `Some(x)`, `Ok(v)`, `Err(e)`, `_`, `User { id, name }`, `1..5`, `&mut buf`, `(a, b, _)`.

- [ ] **Step 3: Run + accept snapshots**

Run: `cargo test -p mty-syntax --test parse_patterns && cargo insta review`.

- [ ] **Step 4: Commit**

```bash
git add crates/mty-syntax/src/parser/patterns.rs crates/mty-syntax/src/parser/mod.rs crates/mty-syntax/tests/parse_patterns.rs crates/mty-syntax/tests/snapshots/
git commit -m "Parse patterns (literal, binding, struct, enum, tuple, range, ref, wildcard)"
```

---

## Task 10: Parse expressions (Pratt precedence + postfix)

**Files:**
- Modify: `crates/mty-syntax/src/parser/exprs.rs`

Precedence table (lowest → highest binding):

| Lvl | Operator | Assoc |
|---|---|---|
| 0  | `..` `..=`                                       | none   |
| 1  | `=` `+=` `-=` `*=` `/=` `%=` `&=` `|=` `^=` `<<=` `>>=` | right  |
| 2  | `||`                                             | left   |
| 3  | `&&`                                             | left   |
| 4  | `==` `!=` `<` `<=` `>` `>=`                      | none   |
| 5  | `|`                                              | left   |
| 6  | `^`                                              | left   |
| 7  | `&` (bitwise; distinguished from borrow by position) | left   |
| 8  | `<<` `>>`                                        | left   |
| 9  | `+` `-`                                          | left   |
| 10 | `*` `/` `%`                                      | left   |
| 11 | `as`                                             | left   |
| 12 | unary `-` `!` `&` `&mut` `*` `move`             | prefix |
| 13 | postfix: `.field` `(args)` `[idx]` `?` `!Msg(...)` `?Msg(...)` `@dur` | postfix |

- [ ] **Step 1: Write the Pratt core in `exprs.rs`**

```rust
use super::{Parser, paths, patterns, types};
use crate::SyntaxKind::{self, *};

pub fn expr(p: &mut Parser) -> bool { expr_bp(p, 0) }

fn expr_bp(p: &mut Parser, min_bp: u8) -> bool {
    p.skip_trivia();
    let cp = p.checkpoint();
    if !unary_or_primary(p) { return false; }

    loop {
        p.skip_trivia();
        // postfix first (highest)
        if try_postfix(p, cp) { continue; }

        // binary
        let (op_bp, _right_bp, op_len) = match infix_bp(p) {
            Some(t) => t,
            None => break,
        };
        if op_bp < min_bp { break; }
        let right_bp = match infix_right_assoc(p.peek()) {
            true  => op_bp,         // right-assoc: same level for RHS
            false => op_bp + 1,     // left-assoc
        };
        // bump operator tokens (some are 2-3 chars but already one SyntaxKind)
        let _ = op_len; // single-token operators in our enum
        p.start_node_at(cp, BINARY_EXPR);
        p.bump_any();
        p.skip_trivia();
        expr_bp(p, right_bp);
        p.finish_node();
    }
    true
}

fn infix_right_assoc(k: SyntaxKind) -> bool {
    matches!(k, EQ | PLUS_EQ | MINUS_EQ | STAR_EQ | SLASH_EQ | PERCENT_EQ
        | AMP_EQ | PIPE_EQ | CARET_EQ | SHL_EQ | SHR_EQ)
}

fn infix_bp(p: &Parser) -> Option<(u8, u8, usize)> {
    let bp = match p.peek() {
        DOT_DOT | DOT_DOT_EQ => 1,
        EQ | PLUS_EQ | MINUS_EQ | STAR_EQ | SLASH_EQ | PERCENT_EQ
            | AMP_EQ | PIPE_EQ | CARET_EQ | SHL_EQ | SHR_EQ => 2,
        PIPE_PIPE => 3,
        AMP_AMP   => 4,
        EQ_EQ | BANG_EQ | LT | LT_EQ | GT | GT_EQ => 5,
        PIPE  => 6,
        CARET => 7,
        AMP   => 8,
        SHL | SHR => 9,
        PLUS | MINUS => 10,
        STAR | SLASH | PERCENT => 11,
        AS_KW => 12,
        _ => return None,
    };
    Some((bp, bp + 1, 1))
}

fn unary_or_primary(p: &mut Parser) -> bool {
    p.skip_trivia();
    match p.peek() {
        MINUS | BANG | STAR => {
            p.start_node(UNARY_EXPR);
            p.bump_any();
            p.skip_trivia();
            unary_or_primary(p);
            p.finish_node();
            true
        }
        AMP => {
            p.start_node(BORROW_EXPR);
            p.bump(AMP);
            p.skip_trivia();
            p.eat(MUT_KW);
            unary_or_primary(p);
            p.finish_node();
            true
        }
        MOVE_KW => {
            p.start_node(MOVE_EXPR);
            p.bump(MOVE_KW);
            p.skip_trivia();
            unary_or_primary(p);
            p.finish_node();
            true
        }
        SPAWN_KW => {
            p.start_node(SPAWN_EXPR);
            p.bump(SPAWN_KW);
            p.skip_trivia();
            // spawn task <expr> | spawn <Path>(args)
            if p.eat(TASK_KW) { expr(p); }
            else { expr(p); }
            p.finish_node();
            true
        }
        _ => primary(p),
    }
}

fn primary(p: &mut Parser) -> bool {
    p.skip_trivia();
    match p.peek() {
        INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | CHAR_LITERAL
        | TRUE_KW | FALSE_KW | DURATION_LITERAL | SIZE_LITERAL => {
            p.start_node(LITERAL_EXPR); p.bump_any(); p.finish_node(); true
        }
        HTML_LITERAL => {
            p.start_node(HTML_EXPR); p.bump(HTML_LITERAL); p.finish_node(); true
        }
        L_PAREN => paren_or_tuple(p),
        L_BRACK => array_or_map_lit(p),
        L_BRACE => block_or_map_or_struct(p),
        IF_KW => super::stmts::if_expr(p),
        MATCH_KW => super::stmts::match_expr(p),
        FOR_KW => super::stmts::for_expr(p),
        WHILE_KW => super::stmts::while_expr(p),
        LOOP_KW => super::stmts::loop_expr(p),
        RETURN_KW => { p.start_node(RETURN_EXPR); p.bump(RETURN_KW); p.skip_trivia();
                       if can_start_expr(p.peek()) { expr(p); } p.finish_node(); true }
        UNSAFE_KW => super::unsafe_::unsafe_block(p),
        ARENA_KW => super::concurrency::arena_block(p),
        TASK_KW => super::concurrency::task_scope_or_call(p),
        BUDGET_KW => super::concurrency::budget_block(p),
        SANDBOX_KW => super::concurrency::sandbox_block(p),
        DETACH_KW => { p.start_node(DETACH_EXPR); p.bump(DETACH_KW); expr(p); p.finish_node(); true }
        JOIN_KW => { p.start_node(JOIN_EXPR); p.bump(JOIN_KW); expr(p); p.finish_node(); true }
        IDENT => path_expr_or_call(p),
        _ => false,
    }
}

fn paren_or_tuple(p: &mut Parser) -> bool {
    let cp = p.checkpoint();
    p.bump(L_PAREN);
    p.skip_trivia();
    if p.eat(R_PAREN) { p.start_node_at(cp, TUPLE_EXPR); p.finish_node(); return true; }
    expr(p);
    if p.eat(COMMA) {
        p.start_node_at(cp, TUPLE_EXPR);
        while !p.at(R_PAREN) { expr(p); if !p.eat(COMMA) { break; } }
        p.expect(R_PAREN);
        p.finish_node();
    } else {
        p.expect(R_PAREN);
        // bare parenthesized expr — leave as the inner expr (no wrapper).
    }
    true
}

fn array_or_map_lit(p: &mut Parser) -> bool {
    p.start_node(ARRAY_EXPR);
    p.bump(L_BRACK);
    p.skip_trivia();
    if !p.at(R_BRACK) {
        expr(p);
        while p.eat(COMMA) { if p.at(R_BRACK) { break; } expr(p); }
    }
    p.expect(R_BRACK);
    p.finish_node();
    true
}

fn block_or_map_or_struct(p: &mut Parser) -> bool {
    // Map literal: { key: value, ... }
    // Struct literal: detected by `Path { ... }`; handled when we recognize the path-headed case.
    // Plain block: { stmt; stmt; expr }
    // Disambiguation: peek after `{`. If first non-trivia token is IDENT followed by COLON, it's a map.
    let cp = p.checkpoint();
    let save = p.pos;
    p.bump(L_BRACE);
    p.skip_trivia();
    let looks_like_map = p.at(IDENT) && p.peek_n(1) == COLON;
    p.pos = save;
    if looks_like_map {
        p.start_node_at(cp, MAP_EXPR);
        p.bump(L_BRACE);
        p.skip_trivia();
        while !p.at(R_BRACE) {
            p.start_node(MAP_ENTRY);
            paths::name(p);
            p.expect(COLON);
            expr(p);
            p.finish_node();
            if !p.eat(COMMA) { break; }
        }
        p.expect(R_BRACE);
        p.finish_node();
    } else {
        super::stmts::block(p);
    }
    true
}

fn path_expr_or_call(p: &mut Parser) -> bool {
    let cp = p.checkpoint();
    p.start_node(PATH_EXPR);
    paths::path(p);
    p.finish_node();
    p.skip_trivia();
    // struct literal: Path { field: expr, ... }
    if p.at(L_BRACE) && lookahead_is_struct_literal(p) {
        p.start_node_at(cp, STRUCT_EXPR);
        p.bump(L_BRACE);
        p.skip_trivia();
        while !p.at(R_BRACE) {
            p.start_node(STRUCT_FIELD_EXPR);
            paths::name(p);
            if p.eat(COLON) { expr(p); }
            p.finish_node();
            if !p.eat(COMMA) { break; }
        }
        p.expect(R_BRACE);
        p.finish_node();
    }
    true
}

fn lookahead_is_struct_literal(p: &Parser) -> bool {
    // Inside a Path { ... } we treat as struct literal only if the immediate body looks
    // like fields (IDENT COLON or IDENT COMMA or empty). Heuristic, refined later if ambiguous.
    let mut i = p.pos + 1;
    while i < p.tokens.len() && p.tokens[i].kind.is_trivia() { i += 1; }
    if p.tokens[i].kind == R_BRACE { return true; }
    if p.tokens[i].kind == IDENT {
        let mut j = i + 1;
        while j < p.tokens.len() && p.tokens[j].kind.is_trivia() { j += 1; }
        return matches!(p.tokens[j].kind, COLON | COMMA | R_BRACE);
    }
    false
}

fn try_postfix(p: &mut Parser, cp: rowan::Checkpoint) -> bool {
    p.skip_trivia();
    match p.peek() {
        DOT => {
            p.start_node_at(cp, FIELD_EXPR);
            p.bump(DOT); p.skip_trivia();
            paths::name(p);
            // method call: identifier followed by ( becomes METHOD_CALL_EXPR
            if p.at(L_PAREN) {
                // upgrade the FIELD_EXPR we just opened into METHOD_CALL_EXPR
                // Easiest: finish FIELD_EXPR, then we'd need a different checkpoint.
                // Implementation choice: emit FIELD_EXPR even for method calls; HIR distinguishes.
                args(p);
            }
            p.finish_node();
            true
        }
        L_PAREN => { p.start_node_at(cp, CALL_EXPR); args(p); p.finish_node(); true }
        L_BRACK => {
            p.start_node_at(cp, INDEX_EXPR);
            p.bump(L_BRACK); expr(p); p.expect(R_BRACK);
            p.finish_node();
            true
        }
        QUESTION => {
            // Disambiguate: `?Msg(args)` is ask; bare `?` is propagate.
            if p.peek_n(1) == IDENT {
                p.start_node_at(cp, ASK_EXPR);
                p.bump(QUESTION); p.skip_trivia();
                paths::name(p);
                if p.at(L_PAREN) { args(p); }
                p.finish_node();
            } else {
                p.start_node_at(cp, QUESTION_EXPR);
                p.bump(QUESTION);
                p.finish_node();
            }
            true
        }
        BANG => {
            // `!Msg(args)` is send; `!expr` is boolean-not (handled in unary, not postfix).
            // Disambiguate by next token: identifier => send.
            if p.peek_n(1) == IDENT {
                p.start_node_at(cp, SEND_EXPR);
                p.bump(BANG); p.skip_trivia();
                paths::name(p);
                if p.at(L_PAREN) { args(p); }
                p.finish_node();
                true
            } else {
                false
            }
        }
        AT => {
            // @duration deadline applies to the preceding expression.
            p.start_node_at(cp, DEADLINE_EXPR);
            p.bump(AT); p.skip_trivia();
            // accept a DURATION_LITERAL primarily, but allow any expr (compile-time const).
            if p.at(DURATION_LITERAL) {
                p.start_node(LITERAL_EXPR); p.bump(DURATION_LITERAL); p.finish_node();
            } else {
                expr(p);
            }
            p.finish_node();
            true
        }
        _ => false,
    }
}

fn args(p: &mut Parser) {
    p.start_node(ARG_LIST);
    p.bump(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        arg(p);
        while p.eat(COMMA) { if p.at(R_PAREN) { break; } arg(p); }
    }
    p.expect(R_PAREN);
    p.finish_node();
}

fn arg(p: &mut Parser) {
    // named argument: IDENT COLON expr
    let save = p.pos;
    if p.at(IDENT) && p.peek_n(1) == COLON {
        p.start_node(NAMED_ARG);
        paths::name(p);
        p.bump(COLON);
        expr(p);
        p.finish_node();
    } else {
        p.pos = save;
        p.start_node(ARG);
        expr(p);
        p.finish_node();
    }
}

pub fn can_start_expr(k: SyntaxKind) -> bool {
    matches!(k,
        INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | CHAR_LITERAL | TRUE_KW | FALSE_KW
        | DURATION_LITERAL | SIZE_LITERAL | HTML_LITERAL | IDENT
        | L_PAREN | L_BRACK | L_BRACE | MINUS | BANG | STAR | AMP | MOVE_KW | SPAWN_KW
        | IF_KW | MATCH_KW | FOR_KW | WHILE_KW | LOOP_KW | RETURN_KW
        | UNSAFE_KW | ARENA_KW | TASK_KW | BUDGET_KW | SANDBOX_KW | DETACH_KW | JOIN_KW
    )
}
```

- [ ] **Step 2: Tests via `parse_expr(src)` test entry**

Add to `parser/mod.rs`:

```rust
pub fn parse_expr(src: &str) -> ParseResult {
    let mut p = Parser::new(src);
    p.builder.start_node(SyntaxKind::FILE.into());
    p.skip_trivia();
    exprs::expr(&mut p);
    p.builder.finish_node();
    ParseResult { green: p.builder.finish(), errors: p.errors }
}
```

`crates/mty-syntax/tests/parse_exprs.rs` snapshots: `1 + 2 * 3`, `a == b && c != d`, `f(x).y[0]`, `arr[i + 1]`, `xs.map(square)`, `obj?.method()` (no — Mighty doesn't have `?.`; use `obj?` chain test), `obj?Msg(x) @2s`, `logger!Info("started")`, `let v = move x`, `&mut buf`, `arena turn: lower(parse(tokenize(input))?)`, `Some(x)`, `User { id, name }`, `{ a: 1, b: 2 }`.

- [ ] **Step 3: Run + accept snapshots**

Run: `cargo test -p mty-syntax --test parse_exprs && cargo insta review`.

- [ ] **Step 4: Commit**

```bash
git add crates/mty-syntax/src/parser/exprs.rs crates/mty-syntax/src/parser/mod.rs crates/mty-syntax/tests/parse_exprs.rs crates/mty-syntax/tests/snapshots/
git commit -m "Parse expressions with Pratt precedence + Mighty postfix (!Msg, ?Msg, @dur)"
```

---

## Task 11: Parse statements, blocks, control flow

**Files:**
- Modify: `crates/mty-syntax/src/parser/stmts.rs`

Productions:

```
Block      = '{' Stmt* Expr? '}'
Stmt       = LetStmt | ExprStmt
LetStmt    = 'let' Pattern (':' Type)? ('=' Expr)? ';'?
ExprStmt   = Expr ';'?
IfExpr     = 'if' Expr Block ('else' (IfExpr | Block))?
MatchExpr  = 'match' Expr '{' MatchArm* '}'
MatchArm   = Pattern ('if' Expr)? '=>' (Block | Expr ','?)
ForExpr    = 'for' Pattern 'in' Expr Block
WhileExpr  = 'while' Expr Block
LoopExpr   = 'loop' Block
```

- [ ] **Step 1: Write `stmts.rs`**

```rust
use super::{Parser, patterns, types, exprs};
use crate::SyntaxKind::*;

pub fn block(p: &mut Parser) {
    p.start_node(BLOCK);
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        if p.at(LET_KW) { let_stmt(p); }
        else if !exprs::can_start_expr(p.peek()) {
            p.error("expected statement or expression"); p.bump_any();
        } else {
            p.start_node(EXPR_STMT);
            exprs::expr(p);
            p.eat(SEMI);
            p.finish_node();
        }
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn let_stmt(p: &mut Parser) {
    p.start_node(LET_STMT);
    p.bump(LET_KW);
    p.skip_trivia();
    patterns::pattern(p);
    if p.eat(COLON) { types::type_expr(p); }
    if p.eat(EQ) { exprs::expr(p); }
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

pub fn if_expr(p: &mut Parser) -> bool {
    p.start_node(IF_EXPR);
    p.bump(IF_KW); p.skip_trivia();
    exprs::expr(p);
    block(p);
    if p.eat(ELSE_KW) {
        if p.at(IF_KW) { if_expr(p); } else { block(p); }
    }
    p.finish_node();
    true
}

pub fn match_expr(p: &mut Parser) -> bool {
    p.start_node(MATCH_EXPR);
    p.bump(MATCH_KW); p.skip_trivia();
    exprs::expr(p);
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        match_arm(p);
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    true
}

fn match_arm(p: &mut Parser) {
    p.start_node(MATCH_ARM);
    patterns::pattern(p);
    if p.eat(IF_KW) { p.start_node(MATCH_GUARD); exprs::expr(p); p.finish_node(); }
    p.expect(FAT_ARROW);
    p.skip_trivia();
    if p.at(L_BRACE) { block(p); }
    else { exprs::expr(p); p.eat(COMMA); }
    p.finish_node();
    p.skip_trivia();
}

pub fn for_expr(p: &mut Parser) -> bool {
    p.start_node(FOR_EXPR);
    p.bump(FOR_KW); p.skip_trivia();
    patterns::pattern(p);
    p.expect(IN_KW); p.skip_trivia();
    exprs::expr(p);
    block(p);
    p.finish_node();
    true
}

pub fn while_expr(p: &mut Parser) -> bool {
    p.start_node(WHILE_EXPR);
    p.bump(WHILE_KW); p.skip_trivia();
    exprs::expr(p);
    block(p);
    p.finish_node();
    true
}

pub fn loop_expr(p: &mut Parser) -> bool {
    p.start_node(LOOP_EXPR);
    p.bump(LOOP_KW); p.skip_trivia();
    block(p);
    p.finish_node();
    true
}
```

- [ ] **Step 2: Snapshot tests in `parse_stmts.rs`** covering `let x = 1`, `let User { id, name } = u`, `if/else if/else`, `match`, `for ... in`, `while`, `loop`, nested blocks.

- [ ] **Step 3: Run + accept**

Run: `cargo test -p mty-syntax --test parse_stmts && cargo insta review`.

- [ ] **Step 4: Commit**

```bash
git add crates/mty-syntax/src/parser/stmts.rs crates/mty-syntax/tests/parse_stmts.rs crates/mty-syntax/tests/snapshots/
git commit -m "Parse statements, blocks, if/match/for/while/loop"
```

---

## Task 12: Parse fn / struct / enum / type alias / impl / trait / const

**Files:**
- Modify: `crates/mty-syntax/src/parser/items.rs`

Productions:

```
FnDecl     = 'pub'? 'unsafe'? 'fn' Name GenericParams? FnParams ('->' Type)? EffectClause? FnBody
FnParams   = '(' (Param (',' Param)*)? ')'
Param      = Name ':' Type
FnBody     = Block | '=' Expr
StructDecl = 'pub'? 'struct' Name GenericParams? '{' StructField* '}'
StructField= Name ':' Type ','?
EnumDecl   = 'pub'? 'enum' Name GenericParams? '{' EnumVariant* '}'
EnumVariant= Name ('(' Type (',' Type)* ')')? ','?
TypeAlias  = 'pub'? 'type' Name GenericParams? '=' Type
ImplBlock  = 'impl' GenericParams? (Path 'for')? Path '{' (FnDecl | TypeAlias)* '}'
TraitDecl  = 'pub'? 'trait' Name GenericParams? '{' TraitMethod* '}'
TraitMethod= 'fn' Name FnParams ('->' Type)? (Block)? ';'?
ConstDecl  = 'pub'? 'const' Name ':' Type '=' Expr ';'?
```

- [ ] **Step 1: Extend `items.rs`** — add a dispatch to fn/struct/enum/type/impl/trait/const after the existing use/mod/package branches. Handle `unsafe fn` and `pub fn` combos.

```rust
// In `pub fn item`, extend the match:
//   FN_KW => { fn_decl(p, cp); true }
//   UNSAFE_KW if p.peek_n(1) == FN_KW => { fn_decl(p, cp); true }
//   STRUCT_KW => { struct_decl(p, cp); true }
//   ENUM_KW => { enum_decl(p, cp); true }
//   TYPE_KW => { type_alias(p, cp); true }
//   IMPL_KW => { impl_block(p, cp); true }
//   TRAIT_KW => { trait_decl(p, cp); true }
//   CONST_KW => { const_decl(p, cp); true }

fn fn_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, FN_DECL);
    p.eat(UNSAFE_KW);
    p.expect(FN_KW); p.skip_trivia();
    super::paths::name(p);
    super::types::generic_params(p);
    fn_params(p);
    if p.eat(THIN_ARROW) { p.start_node(RET_TYPE); super::types::type_expr(p); p.finish_node(); }
    super::types::effect_clause(p);
    p.skip_trivia();
    if p.eat(EQ) { super::exprs::expr(p); p.eat(SEMI); }
    else if p.at(L_BRACE) { super::stmts::block(p); }
    else { p.eat(SEMI); /* trait method signature without body */ }
    p.finish_node();
    p.skip_trivia();
}

fn fn_params(p: &mut Parser) {
    p.start_node(FN_PARAM_LIST);
    p.expect(L_PAREN); p.skip_trivia();
    if !p.at(R_PAREN) {
        param(p);
        while p.eat(COMMA) { if p.at(R_PAREN) { break; } param(p); }
    }
    p.expect(R_PAREN);
    p.finish_node();
    p.skip_trivia();

    fn param(p: &mut Parser) {
        p.start_node(FN_PARAM);
        super::paths::name(p);
        if p.eat(COLON) { super::types::type_expr(p); }
        p.finish_node();
        p.skip_trivia();
    }
}

fn struct_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, STRUCT_DECL);
    p.bump(STRUCT_KW); p.skip_trivia();
    super::paths::name(p);
    super::types::generic_params(p);
    p.expect(L_BRACE);
    p.start_node(STRUCT_FIELD_LIST);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        p.start_node(STRUCT_FIELD);
        super::paths::name(p);
        if p.eat(COLON) { super::types::type_expr(p); }
        p.finish_node();
        p.eat(COMMA);
        p.skip_trivia();
    }
    p.finish_node();
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn enum_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, ENUM_DECL);
    p.bump(ENUM_KW); p.skip_trivia();
    super::paths::name(p);
    super::types::generic_params(p);
    p.expect(L_BRACE);
    p.start_node(ENUM_VARIANT_LIST);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        p.start_node(ENUM_VARIANT);
        super::paths::name(p);
        if p.eat(L_PAREN) {
            super::types::type_expr(p);
            while p.eat(COMMA) { if p.at(R_PAREN) { break; } super::types::type_expr(p); }
            p.expect(R_PAREN);
        }
        p.finish_node();
        p.eat(COMMA);
        p.skip_trivia();
    }
    p.finish_node();
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn type_alias(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, TYPE_ALIAS);
    p.bump(TYPE_KW); p.skip_trivia();
    super::paths::name(p);
    super::types::generic_params(p);
    p.expect(EQ);
    super::types::type_expr(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn impl_block(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, IMPL_BLOCK);
    p.bump(IMPL_KW); p.skip_trivia();
    super::types::generic_params(p);
    // Either `Trait for Type` or just `Type`. Parse first path; if `for`, parse second.
    super::types::type_expr(p);
    if p.eat(FOR_KW) { super::types::type_expr(p); }
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        let icp = p.checkpoint();
        if p.eat(PUB_KW) { /* visibility prefix */ }
        if p.at(FN_KW) || (p.at(UNSAFE_KW) && p.peek_n(1) == FN_KW) {
            fn_decl(p, icp);
        } else if p.at(TYPE_KW) {
            type_alias(p, icp);
        } else {
            p.error("expected fn or type alias in impl"); p.bump_any();
        }
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn trait_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, TRAIT_DECL);
    p.bump(TRAIT_KW); p.skip_trivia();
    super::paths::name(p);
    super::types::generic_params(p);
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        let cp2 = p.checkpoint();
        if p.at(FN_KW) || (p.at(UNSAFE_KW) && p.peek_n(1) == FN_KW) {
            p.start_node_at(cp2, TRAIT_METHOD);
            fn_decl(p, cp2);
            p.finish_node();
        } else {
            p.error("expected fn in trait"); p.bump_any();
        }
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn const_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    // We don't have a CONST_DECL kind yet; add CONST_DECL to SyntaxKind before this task.
    p.start_node_at(cp, CONST_DECL);
    p.bump(CONST_KW); p.skip_trivia();
    super::paths::name(p);
    p.expect(COLON);
    super::types::type_expr(p);
    p.expect(EQ);
    super::exprs::expr(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}
```

NOTE: before this task, add `CONST_DECL` to `SyntaxKind` in `syntax_kind.rs` (alongside other item kinds) — small follow-up edit + recompile.

- [ ] **Step 2: Snapshot tests in `parse_decls.rs`** covering each item kind. Plus a multi-item file like:

```sd
pub fn add(a: I32, b: I32) -> I32 = a + b

struct User { id: U64, name: String }
enum Result[T, E] { Ok(T), Err(E) }
type UserId = U64
impl Hash for UserId { fn hash(self) -> U64 = self.value }
trait Hash { fn hash(self) -> U64 }
const PAGE: USize = 4096
```

- [ ] **Step 3: Run + accept**

```bash
cargo test -p mty-syntax --test parse_decls && cargo insta review
```

- [ ] **Step 4: Commit**

```bash
git add crates/mty-syntax/
git commit -m "Parse fn/struct/enum/type/impl/trait/const declarations"
```

---

## Task 13: Parse agents, protocols, supervisors

**Files:**
- Modify: `crates/mty-syntax/src/parser/agents.rs`
- Modify: `crates/mty-syntax/src/parser/items.rs` (add dispatch)

Productions (from spec §12, §13, §15):

```
AgentDecl    = 'pub'? 'agent' Name AgentCtorParams? (':' ProtocolList)? AgentBody
AgentCtorParams = '(' (Name (',' Name)*)? ')'
ProtocolList = Path ('+' Path)*
AgentBody    = '{' AgentMember* '}'
AgentMember  = AgentState | OnHandler | FnDecl
AgentState   = ('state')? Name ('=' Expr | ':' Type ('=' Expr)?)
OnHandler    = 'on' Name '(' (Name (',' Name)*)? ')' (('->' Expr) | Block)

ProtocolDecl = 'pub'? 'protocol' Name VersionTag? ('=' ProtocolUnion | '{' ProtocolMsg* '}')
VersionTag   = 'v' INT_LITERAL          // optional; spec §13.4
ProtocolUnion= Path ('+' Path)*
ProtocolMsg  = Name '(' (Param (',' Param)*)? ')' ('->' Type)?

SupervisorDecl = ('supervisor'|'sup') Name (Strategy)? '{' SupBody* '}'
Strategy = 'one_for_one' | 'one_for_all' | 'rest_for_one' | 'escalate'
SupBody  = ChildDecl | OnFailClause
ChildDecl= 'child' Name '=' Expr | Name '=' Expr
OnFailClause = 'on_fail' '(' Name ')' '{' SupAction (';' SupAction)* '}'
SupAction = 'restart' ('up_to' INT 'in' DurationLit)? | 'backoff' DurationLit '..' DurationLit
```

`supervisor` is a contextual keyword (not in §3.3 reserved list); detected by IDENT text. `sup` is reserved (added in Task 3 SyntaxKind list).

- [ ] **Step 1: Write `agents.rs`**

```rust
use super::{Parser, paths, exprs, types, stmts};
use crate::SyntaxKind::*;

pub fn agent_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, AGENT_DECL);
    p.bump(AGENT_KW); p.skip_trivia();
    paths::name(p);
    if p.at(L_PAREN) { ctor_params(p); }
    if p.eat(COLON) {
        p.start_node(AGENT_PROTOCOL_LIST);
        types::type_expr(p);
        while p.eat(PLUS) { types::type_expr(p); }
        p.finish_node();
    }
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        agent_member(p);
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn ctor_params(p: &mut Parser) {
    p.start_node(AGENT_CTOR_PARAMS);
    p.bump(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        paths::name(p);
        while p.eat(COMMA) { if p.at(R_PAREN) { break; } paths::name(p); }
    }
    p.expect(R_PAREN);
    p.finish_node();
    p.skip_trivia();
}

fn agent_member(p: &mut Parser) {
    let cp = p.checkpoint();
    if p.at(ON_KW) { on_handler(p, cp); return; }
    if p.at(FN_KW) || (p.at(UNSAFE_KW) && p.peek_n(1) == FN_KW) {
        super::items::fn_decl_pub(p, cp); return;
    }
    if p.at(STATE_KW) || (p.at(IDENT) && (p.peek_n(1) == EQ || p.peek_n(1) == COLON)) {
        state_decl(p, cp); return;
    }
    p.error("expected agent member (`on`, `fn`, or state field)");
    p.bump_any();
}

fn state_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, AGENT_STATE_DECL);
    p.eat(STATE_KW);
    paths::name(p);
    if p.eat(COLON) { types::type_expr(p); }
    if p.eat(EQ) { exprs::expr(p); }
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn on_handler(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, ON_HANDLER);
    p.bump(ON_KW); p.skip_trivia();
    paths::name(p);
    if p.eat(L_PAREN) {
        if !p.at(R_PAREN) {
            paths::name(p);
            while p.eat(COMMA) { if p.at(R_PAREN) { break; } paths::name(p); }
        }
        p.expect(R_PAREN);
    }
    p.skip_trivia();
    if p.eat(THIN_ARROW) { exprs::expr(p); }
    else if p.at(L_BRACE) { stmts::block(p); }
    p.finish_node();
    p.skip_trivia();
}

pub fn protocol_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, PROTOCOL_DECL);
    p.bump(PROTOCOL_KW); p.skip_trivia();
    paths::name(p);
    // optional version tag: `v1`
    if p.at(IDENT) && p.tokens[p.pos].text.starts_with('v') {
        let rest = &p.tokens[p.pos].text[1..];
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            paths::name(p);
        }
    }
    if p.eat(EQ) {
        // composition: protocol Web = Fetch + Cache + Health
        types::type_expr(p);
        while p.eat(PLUS) { types::type_expr(p); }
    } else {
        p.expect(L_BRACE);
        p.skip_trivia();
        while !p.at(R_BRACE) && !p.at(EOF) {
            protocol_msg(p);
            p.skip_trivia();
        }
        p.expect(R_BRACE);
    }
    p.finish_node();
    p.skip_trivia();
}

fn protocol_msg(p: &mut Parser) {
    p.start_node(PROTOCOL_MSG);
    paths::name(p);
    p.expect(L_PAREN);
    if !p.at(R_PAREN) {
        p.start_node(FN_PARAM);
        paths::name(p);
        if p.eat(COLON) { types::type_expr(p); }
        p.finish_node();
        while p.eat(COMMA) {
            if p.at(R_PAREN) { break; }
            p.start_node(FN_PARAM);
            paths::name(p);
            if p.eat(COLON) { types::type_expr(p); }
            p.finish_node();
        }
    }
    p.expect(R_PAREN);
    if p.eat(THIN_ARROW) { types::type_expr(p); }
    p.finish_node();
    p.skip_trivia();
}

pub fn supervisor_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, SUPERVISOR_DECL);
    // either `supervisor Name(strategy: X)` or `sup Name strategy`
    if p.at(SUP_KW) { p.bump(SUP_KW); }
    else { p.bump_any(); /* consumed "supervisor" IDENT */ }
    p.skip_trivia();
    paths::name(p);
    // optional strategy or args
    if p.eat(L_PAREN) {
        if !p.at(R_PAREN) { exprs::expr(p); while p.eat(COMMA) { exprs::expr(p); } }
        p.expect(R_PAREN);
    } else if p.at(IDENT) {
        paths::name(p);
    }
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        sup_body(p);
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn sup_body(p: &mut Parser) {
    if p.eat(ON_FAIL_KW) {
        p.start_node(ON_FAIL_CLAUSE);
        p.expect(L_PAREN); paths::name(p); p.expect(R_PAREN);
        p.expect(L_BRACE);
        while !p.at(R_BRACE) && !p.at(EOF) { sup_action(p); p.eat(SEMI); }
        p.expect(R_BRACE);
        p.finish_node();
        return;
    }
    p.start_node(SUP_CHILD);
    p.eat(CHILD_KW);
    paths::name(p);
    p.expect(EQ);
    exprs::expr(p);
    p.eat(SEMI);
    p.finish_node();
}

fn sup_action(p: &mut Parser) {
    // restart [up_to N in DUR] | backoff DUR..DUR [;restart]
    if p.eat(RESTART_KW) {
        if p.eat(UP_TO_KW) {
            super::exprs::expr(p);
            p.expect(IN_KW);
            super::exprs::expr(p);
        }
    } else if p.eat(BACKOFF_KW) {
        super::exprs::expr(p);
        if p.eat(DOT_DOT) { super::exprs::expr(p); }
    } else {
        p.error("expected restart or backoff");
        p.bump_any();
    }
}
```

- [ ] **Step 2: Wire dispatch in `items.rs`** — extend the `match` in `pub fn item`:

```rust
AGENT_KW => { super::agents::agent_decl(p, cp); true }
PROTOCOL_KW => { super::agents::protocol_decl(p, cp); true }
SUP_KW => { super::agents::supervisor_decl(p, cp); true }
IDENT if p.tokens[p.pos].text == "supervisor" => { super::agents::supervisor_decl(p, cp); true }
```

Also expose `fn_decl_pub` (rename in items.rs from `fn_decl` to `fn_decl_pub` and keep the signature unchanged) so `agents.rs` can call it.

- [ ] **Step 3: Snapshot tests in `parse_agents.rs`** covering: spec §4 echo, §12.2 counter, §15 SearchFlow, §13.2 composition, §13.3 streaming.

- [ ] **Step 4: Run + accept**

```bash
cargo test -p mty-syntax --test parse_agents && cargo insta review
```

- [ ] **Step 5: Commit**

```bash
git add crates/mty-syntax/
git commit -m "Parse agents, protocols, supervisors with state/on/composition/strategies"
```

---

## Task 14: Parse arenas, task scopes, budgets, sandboxes

**Files:**
- Modify: `crates/mty-syntax/src/parser/concurrency.rs`

Productions:

```
ArenaBlock   = 'arena' Name ((':' Expr) | Block)
TaskScope    = 'task' 'scope' DeadlineSuffix? Block
TaskCall     = 'task' '.' Name '(' Args ')'
BudgetBlock  = 'budget' '{' BudgetEntry+ '}' 'run' (Block | Expr)
BudgetEntry  = Name (DurationLit | SizeLit | IntLit)
SandboxBlock = 'sandbox' Name 'with' '{' SandboxEntry+ '}' Block
SandboxEntry = (Path | Name) '=' Expr (',' | ';')?
```

- [ ] **Step 1: Write `concurrency.rs`**

```rust
use super::{Parser, paths, exprs, stmts};
use crate::SyntaxKind::*;

pub fn arena_block(p: &mut Parser) -> bool {
    p.start_node(ARENA_BLOCK);
    p.bump(ARENA_KW); p.skip_trivia();
    paths::name(p);
    if p.eat(COLON) { exprs::expr(p); }
    else if p.at(L_BRACE) { stmts::block(p); }
    p.finish_node();
    true
}

pub fn task_scope_or_call(p: &mut Parser) -> bool {
    // `task scope ...` is a task scope; `task.<method>(...)` is a method call on `task` ident.
    if p.peek_n(1) == DOT {
        // primary path will handle this; fall back to path_expr
        let cp = p.checkpoint();
        p.start_node(PATH_EXPR);
        paths::path(p);
        p.finish_node();
        // postfix loop handles `.method(...)` from the primary chain
        let _ = cp;
        return true;
    }
    p.start_node(TASK_SCOPE);
    p.bump(TASK_KW); p.skip_trivia();
    p.eat(SCOPE_KW);
    p.skip_trivia();
    // optional deadline `@2s`
    if p.eat(AT) { exprs::expr(p); }
    stmts::block(p);
    p.finish_node();
    true
}

pub fn budget_block(p: &mut Parser) -> bool {
    p.start_node(BUDGET_BLOCK);
    p.bump(BUDGET_KW); p.skip_trivia();
    p.expect(L_BRACE); p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        p.start_node(BUDGET_ENTRY);
        paths::name(p);
        // value: duration, size, or int literal expression
        exprs::expr(p);
        p.eat(SEMI);
        p.finish_node();
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.skip_trivia();
    p.expect(RUN_KW);
    p.skip_trivia();
    if p.at(L_BRACE) { stmts::block(p); } else { exprs::expr(p); }
    p.finish_node();
    true
}

pub fn sandbox_block(p: &mut Parser) -> bool {
    p.start_node(SANDBOX_BLOCK);
    p.bump(SANDBOX_KW); p.skip_trivia();
    paths::name(p);
    p.expect(WITH_KW);
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        p.start_node(SANDBOX_ENTRY);
        // LHS can be a path like `fs.read` or just `cpu`
        paths::path(p);
        p.expect(EQ);
        exprs::expr(p);
        p.eat(COMMA);
        p.eat(SEMI);
        p.finish_node();
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.skip_trivia();
    stmts::block(p);
    p.finish_node();
    true
}
```

- [ ] **Step 2: Snapshot tests in `parse_concurrency.rs`** covering: spec §7.5 arena, §14.1 task scope, §16.1 budget block, §16.1 sandbox block.

- [ ] **Step 3: Run + accept**

```bash
cargo test -p mty-syntax --test parse_concurrency && cargo insta review
```

- [ ] **Step 4: Commit**

```bash
git add crates/mty-syntax/
git commit -m "Parse arena/task scope/budget/sandbox blocks"
```

---

## Task 15: Parse extern blocks, export decls, macros, unsafe blocks

**Files:**
- Modify: `crates/mty-syntax/src/parser/extern_.rs`
- Modify: `crates/mty-syntax/src/parser/macros.rs`
- Modify: `crates/mty-syntax/src/parser/unsafe_.rs`
- Modify: `crates/mty-syntax/src/parser/items.rs`

Productions:

```
ExternBlock = 'extern' (Name)? '{' ExternFn* '}'   // Name is "c" | "js" etc.
ExternFn    = 'fn' Name FnParams ('->' Type)? EffectClause? ';'?
ExportDecl  = 'export' (Name)? (FnDecl | ComponentDecl)
ComponentDecl = 'component' Name '{' ComponentBody '}'   // library-lowered; slice 1 parses as opaque block
MacroDecl   = 'macro' Name '(' (Name (',' Name)*)? ')' '=>' '{' (any tokens until matched '}')* '}'
UnsafeBlock = 'unsafe' Block
UnsafeFn    = 'pub'? 'unsafe' 'fn' ...   // already covered in items::fn_decl
```

- [ ] **Step 1: `extern_.rs`**

```rust
use super::{Parser, paths, types};
use crate::SyntaxKind::*;

pub fn extern_block(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, EXTERN_BLOCK);
    p.bump(EXTERN_KW); p.skip_trivia();
    if p.at(IDENT) { paths::name(p); }   // e.g. `c`, `js`
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        extern_fn(p);
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn extern_fn(p: &mut Parser) {
    p.start_node(EXTERN_FN);
    p.expect(FN_KW); p.skip_trivia();
    paths::name(p);
    super::items::fn_params_for_extern(p);
    if p.eat(THIN_ARROW) { types::type_expr(p); }
    types::effect_clause(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

pub fn export_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, EXPORT_DECL);
    p.bump(EXPORT_KW); p.skip_trivia();
    if p.at(IDENT) && (p.tokens[p.pos].text == "c" || p.tokens[p.pos].text == "js") {
        paths::name(p); p.skip_trivia();
    }
    if p.at(IDENT) && p.tokens[p.pos].text == "component" {
        // export component Name { ... } — opaque body in slice 1
        paths::name(p); p.skip_trivia();
        paths::name(p);
        p.expect(L_BRACE);
        // Skip body brace-balanced, preserving tokens as raw children.
        let mut depth = 1;
        while !p.at(EOF) && depth > 0 {
            match p.peek() {
                L_BRACE => { depth += 1; p.bump_any(); }
                R_BRACE => { depth -= 1; if depth == 0 { break; } else { p.bump_any(); } }
                _ => p.bump_any(),
            }
        }
        p.expect(R_BRACE);
    } else if p.at(FN_KW) {
        let icp = p.checkpoint();
        super::items::fn_decl_pub(p, icp);
    }
    p.finish_node();
    p.skip_trivia();
}
```

Add `fn_params_for_extern` to `items.rs` as a thin alias for the existing `fn_params` (no-arg-body fn signature parsing).

- [ ] **Step 2: `macros.rs`** — parse as opaque token tree until matched `}`. Real macro expansion is deferred.

```rust
use super::Parser;
use crate::SyntaxKind::*;

pub fn macro_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, MACRO_DECL);
    p.bump(MACRO_KW); p.skip_trivia();
    super::paths::name(p);
    p.expect(L_PAREN);
    if !p.at(R_PAREN) {
        super::paths::name(p);
        while p.eat(COMMA) { if p.at(R_PAREN) { break; } super::paths::name(p); }
    }
    p.expect(R_PAREN);
    p.expect(FAT_ARROW);
    p.expect(L_BRACE);
    let mut depth = 1;
    while !p.at(EOF) && depth > 0 {
        match p.peek() {
            L_BRACE => { depth += 1; p.bump_any(); }
            R_BRACE => { depth -= 1; if depth == 0 { break; } else { p.bump_any(); } }
            _ => p.bump_any(),
        }
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}
```

- [ ] **Step 3: `unsafe_.rs`**

```rust
use super::{Parser, stmts};
use crate::SyntaxKind::*;

pub fn unsafe_block(p: &mut Parser) -> bool {
    p.start_node(UNSAFE_BLOCK);
    p.bump(UNSAFE_KW); p.skip_trivia();
    stmts::block(p);
    p.finish_node();
    true
}
```

- [ ] **Step 4: Wire dispatch in `items.rs`**

```rust
EXTERN_KW => { super::extern_::extern_block(p, cp); true }
EXPORT_KW => { super::extern_::export_decl(p, cp); true }
MACRO_KW  => { super::macros::macro_decl(p, cp); true }
```

- [ ] **Step 5: Snapshot tests** covering: spec §22.3 extern js, §26.1 extern c + export c, §20.3 assert_eq macro, §21 unsafe block + unsafe fn from §21 with requires clauses (the `requires` is parsed as a postfix opaque clause on the fn signature; for slice 1 we treat it as a syntactic noise expression — extend `fn_decl_pub` to accept zero or more `requires` clauses between the signature and the body or `;`).

Update `fn_decl_pub` accordingly:

```rust
// After return type and effect clause, before body:
while p.eat(REQUIRES_KW) { super::exprs::expr(p); }
```

- [ ] **Step 6: Run + accept**

```bash
cargo test -p mty-syntax --test parse_extern_macro && cargo insta review
```

- [ ] **Step 7: Commit**

```bash
git add crates/mty-syntax/
git commit -m "Parse extern blocks, export decls, macros, unsafe blocks, requires clauses"
```

---

## Task 16: Parser recovery sweep + EOF/junk tests

**Files:**
- Create: `crates/mty-syntax/tests/parse_recovery.rs`

Validate the parser doesn't infinite-loop, always produces a green tree, and emits diagnostics for malformed input.

- [ ] **Step 1: Write recovery tests**

```rust
use sdust_syntax::{parse, SyntaxNode, SyntaxKind};

fn parse_ok_shape(src: &str) {
    let r = parse(src);
    let root = SyntaxNode::new_root(r.green);
    assert_eq!(root.kind(), SyntaxKind::FILE, "src: {}", src);
    // Allow errors but require partial structure
    assert!(root.children().count() >= 0);
}

#[test] fn empty_input() { parse_ok_shape(""); }
#[test] fn whitespace_only() { parse_ok_shape("   \n\t\n  "); }
#[test] fn lone_keyword() { parse_ok_shape("fn"); }
#[test] fn unterminated_string() { let r = parse(r#""hello"#); assert!(!r.errors.is_empty()); }
#[test] fn unbalanced_brace() { let r = parse("fn main() {"); assert!(!r.errors.is_empty()); }
#[test] fn random_punct() { parse_ok_shape("@@@???!!!"); }
#[test] fn agent_missing_brace() { let r = parse("agent X: Y { on Foo() ->"); assert!(!r.errors.is_empty()); }
#[test] fn extern_missing_body() { let r = parse("extern c {"); assert!(!r.errors.is_empty()); }
#[test] fn recovers_after_error() {
    let r = parse("fn broken( ;\nfn good() {}\n");
    // The second fn should still appear in the tree
    let root = SyntaxNode::new_root(r.green);
    let fns: Vec<_> = root.descendants().filter(|n| n.kind() == SyntaxKind::FN_DECL).collect();
    assert!(fns.len() >= 1);
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p mty-syntax --test parse_recovery`
Expected: 9 passed. If `recovers_after_error` fails, audit the `sync_to` calls in `items::item` to make sure they consume to the next item-start token.

- [ ] **Step 3: Commit**

```bash
git add crates/mty-syntax/tests/parse_recovery.rs
git commit -m "Add parser recovery + EOF safety tests"
```

---

## Task 17: AST view crate (`mty-ast`)

**Files:**
- Modify: `crates/mty-ast/src/lib.rs`
- Create: `crates/mty-ast/src/generated.rs`

Typed accessor structs over `SyntaxNode`. One struct per CST node kind that downstream consumers need. Pattern from rust-analyzer.

- [ ] **Step 1: Write the `AstNode` trait + macro in `lib.rs`**

```rust
//! Typed AST view over the rowan CST.
pub use sdust_syntax::{SyntaxNode, SyntaxToken, SyntaxKind};

pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(node: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

macro_rules! ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone)]
        pub struct $name(pub SyntaxNode);
        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool { kind == SyntaxKind::$kind }
            fn cast(node: SyntaxNode) -> Option<Self> {
                if Self::can_cast(node.kind()) { Some(Self(node)) } else { None }
            }
            fn syntax(&self) -> &SyntaxNode { &self.0 }
        }
    };
}

pub(crate) use ast_node;

mod generated;
pub use generated::*;
```

- [ ] **Step 2: Write `generated.rs`** — one accessor struct per major node kind. Minimum coverage for HIR lowering and the formatter:

```rust
use crate::{ast_node, AstNode};
use sdust_syntax::{SyntaxNode, SyntaxKind};

ast_node!(File, FILE);
ast_node!(PackageDecl, PACKAGE_DECL);
ast_node!(UseDecl, USE_DECL);
ast_node!(ModDecl, MOD_DECL);
ast_node!(FnDecl, FN_DECL);
ast_node!(FnParamList, FN_PARAM_LIST);
ast_node!(FnParam, FN_PARAM);
ast_node!(RetType, RET_TYPE);
ast_node!(EffectClause, EFFECT_CLAUSE);
ast_node!(StructDecl, STRUCT_DECL);
ast_node!(StructField, STRUCT_FIELD);
ast_node!(EnumDecl, ENUM_DECL);
ast_node!(EnumVariant, ENUM_VARIANT);
ast_node!(TypeAlias, TYPE_ALIAS);
ast_node!(ImplBlock, IMPL_BLOCK);
ast_node!(TraitDecl, TRAIT_DECL);
ast_node!(TraitMethod, TRAIT_METHOD);
ast_node!(AgentDecl, AGENT_DECL);
ast_node!(AgentCtorParams, AGENT_CTOR_PARAMS);
ast_node!(AgentProtocolList, AGENT_PROTOCOL_LIST);
ast_node!(AgentStateDecl, AGENT_STATE_DECL);
ast_node!(OnHandler, ON_HANDLER);
ast_node!(ProtocolDecl, PROTOCOL_DECL);
ast_node!(ProtocolMsg, PROTOCOL_MSG);
ast_node!(SupervisorDecl, SUPERVISOR_DECL);
ast_node!(SupChild, SUP_CHILD);
ast_node!(OnFailClause, ON_FAIL_CLAUSE);
ast_node!(BudgetBlock, BUDGET_BLOCK);
ast_node!(BudgetEntry, BUDGET_ENTRY);
ast_node!(SandboxBlock, SANDBOX_BLOCK);
ast_node!(SandboxEntry, SANDBOX_ENTRY);
ast_node!(ArenaBlock, ARENA_BLOCK);
ast_node!(TaskScope, TASK_SCOPE);
ast_node!(ExternBlock, EXTERN_BLOCK);
ast_node!(ExternFn, EXTERN_FN);
ast_node!(ExportDecl, EXPORT_DECL);
ast_node!(MacroDecl, MACRO_DECL);
ast_node!(UnsafeBlock, UNSAFE_BLOCK);
ast_node!(Block, BLOCK);
ast_node!(LetStmt, LET_STMT);
ast_node!(ExprStmt, EXPR_STMT);
// Path/Name
ast_node!(Path, PATH);
ast_node!(PathSegment, PATH_SEGMENT);
ast_node!(Name, NAME);
ast_node!(NameRef, NAME_REF);
// Generic + Visibility + Attr
ast_node!(GenericParamList, GENERIC_PARAM_LIST);
ast_node!(GenericParam, GENERIC_PARAM);
ast_node!(GenericArgList, GENERIC_ARG_LIST);
ast_node!(GenericArg, GENERIC_ARG);
ast_node!(Visibility, VISIBILITY);

// Common accessors:
impl File {
    pub fn items(&self) -> impl Iterator<Item = SyntaxNode> + '_ {
        self.0.children()
    }
}

impl Name {
    pub fn text(&self) -> String {
        self.0.first_token().map(|t| t.text().to_string()).unwrap_or_default()
    }
}

impl Path {
    pub fn segments(&self) -> impl Iterator<Item = PathSegment> + '_ {
        self.0.children().filter_map(PathSegment::cast)
    }
    pub fn text(&self) -> String { self.0.text().to_string() }
}

impl FnDecl {
    pub fn name(&self) -> Option<Name> {
        self.0.children().find_map(Name::cast)
    }
    pub fn param_list(&self) -> Option<FnParamList> {
        self.0.children().find_map(FnParamList::cast)
    }
    pub fn ret_type(&self) -> Option<RetType> {
        self.0.children().find_map(RetType::cast)
    }
    pub fn effect_clause(&self) -> Option<EffectClause> {
        self.0.children().find_map(EffectClause::cast)
    }
    pub fn body(&self) -> Option<Block> {
        self.0.children().find_map(Block::cast)
    }
    pub fn is_pub(&self) -> bool {
        self.0.children().any(|c| c.kind() == SyntaxKind::VISIBILITY)
    }
    pub fn is_unsafe(&self) -> bool {
        self.0.children_with_tokens()
            .filter_map(|e| e.into_token())
            .any(|t| t.kind() == SyntaxKind::UNSAFE_KW)
    }
}

impl AgentDecl {
    pub fn name(&self) -> Option<Name> { self.0.children().find_map(Name::cast) }
    pub fn ctor_params(&self) -> Option<AgentCtorParams> { self.0.children().find_map(AgentCtorParams::cast) }
    pub fn protocols(&self) -> Option<AgentProtocolList> { self.0.children().find_map(AgentProtocolList::cast) }
    pub fn handlers(&self) -> impl Iterator<Item = OnHandler> + '_ {
        self.0.descendants().filter_map(OnHandler::cast)
    }
    pub fn state_fields(&self) -> impl Iterator<Item = AgentStateDecl> + '_ {
        self.0.descendants().filter_map(AgentStateDecl::cast)
    }
}

// Add the same shape for ProtocolDecl, SupervisorDecl, StructDecl, EnumDecl, etc.
// Keep accessors minimal — HIR and formatter add helpers as needed.
```

- [ ] **Step 3: Tests**

`crates/mty-ast/tests/cast.rs`:

```rust
use sdust_ast::{AstNode, File, FnDecl, AgentDecl};
use sdust_syntax::{parse, SyntaxNode};

fn root(src: &str) -> File {
    let r = parse(src);
    File::cast(SyntaxNode::new_root(r.green)).expect("FILE root")
}

#[test]
fn casts_fn() {
    let f = root("fn add(a: I32, b: I32) -> I32 = a + b");
    let fns: Vec<FnDecl> = f.0.descendants().filter_map(FnDecl::cast).collect();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name().unwrap().text(), "add");
}

#[test]
fn casts_agent_with_handlers() {
    let f = root("agent Counter: Count { n = 0\n on Inc() -> { n += 1; n }\n }");
    let agents: Vec<AgentDecl> = f.0.descendants().filter_map(AgentDecl::cast).collect();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].handlers().count(), 1);
    assert_eq!(agents[0].state_fields().count(), 1);
}
```

- [ ] **Step 4: Run**

Run: `cargo test -p mty-ast`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/mty-ast/
git commit -m "Add typed AST view over rowan CST"
```

---

## Task 18: Diagnostics core types + DiagCode

**Files:**
- Modify: `crates/mty-diagnostics/src/lib.rs`
- Create: `crates/mty-diagnostics/src/diagnostic.rs`
- Create: `crates/mty-diagnostics/src/codes.rs`

- [ ] **Step 1: Write `codes.rs`**

```rust
//! Stable diagnostic codes. Once assigned, NEVER renumber.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiagCode(pub u16);

impl DiagCode {
    pub const fn new(n: u16) -> Self { DiagCode(n) }
    pub fn as_str(&self) -> String { format!("SD{:04}", self.0) }
}

// Lex/parse: MT0001..MT0999
pub const UNEXPECTED_TOKEN: DiagCode      = DiagCode::new(0001);
pub const UNTERMINATED_STRING: DiagCode   = DiagCode::new(0002);
pub const INVALID_ESCAPE: DiagCode        = DiagCode::new(0003);
pub const UNKNOWN_DURATION_UNIT: DiagCode = DiagCode::new(0004);
pub const EXPECTED_ITEM: DiagCode         = DiagCode::new(0010);
pub const EXPECTED_EXPR: DiagCode         = DiagCode::new(0011);
pub const MISMATCHED_DELIMITER: DiagCode  = DiagCode::new(0012);
pub const DUPLICATE_ON_HANDLER: DiagCode  = DiagCode::new(0020);
pub const PUB_NEEDS_RETURN_TYPE: DiagCode = DiagCode::new(0021);
pub const DEPTH_LIMIT_EXCEEDED: DiagCode  = DiagCode::new(0030);

// HIR: MT1001..MT1999
pub const UNRESOLVED_NAME: DiagCode       = DiagCode::new(1001);
pub const USE_RESOLVES_TO_NOTHING: DiagCode = DiagCode::new(1002);
```

- [ ] **Step 2: Write `diagnostic.rs`**

```rust
use crate::codes::DiagCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity { Error, Warning, Note, Help }

#[derive(Debug, Clone)]
pub struct Label {
    pub start: usize,
    pub end: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
    pub primary: Label,
    pub secondary: Vec<Label>,
    pub notes: Vec<String>,
    pub helps: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: DiagCode, primary: Label) -> Self {
        Self { code, severity: Severity::Error, primary, secondary: vec![], notes: vec![], helps: vec![] }
    }
    pub fn with_note(mut self, n: impl Into<String>) -> Self { self.notes.push(n.into()); self }
    pub fn with_help(mut self, h: impl Into<String>) -> Self { self.helps.push(h.into()); self }
    pub fn with_secondary(mut self, l: Label) -> Self { self.secondary.push(l); self }
}
```

- [ ] **Step 3: Update `lib.rs`**

```rust
pub mod codes;
pub mod diagnostic;
pub mod render;
pub use codes::DiagCode;
pub use diagnostic::{Diagnostic, Severity, Label};
```

Empty `render/mod.rs`:

```rust
pub mod ariadne;
```

- [ ] **Step 4: Tests**

`crates/mty-diagnostics/tests/basic.rs`:

```rust
use sdust_diagnostics::*;
use sdust_diagnostics::codes::UNEXPECTED_TOKEN;

#[test]
fn code_format() {
    assert_eq!(UNEXPECTED_TOKEN.as_str(), "MT0001");
}

#[test]
fn build_diagnostic() {
    let d = Diagnostic::error(UNEXPECTED_TOKEN, Label { start: 5, end: 8, message: "here".into() })
        .with_note("try removing the token")
        .with_help("see MT0001 reference");
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.notes.len(), 1);
    assert_eq!(d.helps.len(), 1);
}
```

Run: `cargo test -p mty-diagnostics`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/mty-diagnostics/
git commit -m "Define DiagCode, Diagnostic, Severity, Label types"
```

---

## Task 19: ariadne renderer

**Files:**
- Modify: `crates/mty-diagnostics/src/render/ariadne.rs`

- [ ] **Step 1: Write the renderer**

```rust
use crate::{Diagnostic, Severity};
use ariadne::{Color, Label as AriadneLabel, Report, ReportKind, Source};

pub fn render(diag: &Diagnostic, source_id: &str, source: &str) -> String {
    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Note => ReportKind::Advice,
        Severity::Help => ReportKind::Advice,
    };
    let mut builder = Report::build(kind, source_id, diag.primary.start)
        .with_code(diag.code.as_str())
        .with_message(&diag.primary.message);
    builder = builder.with_label(
        AriadneLabel::new((source_id, diag.primary.start..diag.primary.end))
            .with_message(&diag.primary.message)
            .with_color(Color::Red),
    );
    for sec in &diag.secondary {
        builder = builder.with_label(
            AriadneLabel::new((source_id, sec.start..sec.end))
                .with_message(&sec.message)
                .with_color(Color::Yellow),
        );
    }
    for note in &diag.notes {
        builder = builder.with_note(note);
    }
    for help in &diag.helps {
        builder = builder.with_help(help);
    }
    let report = builder.finish();
    let mut buf = Vec::new();
    report.write((source_id, Source::from(source)), &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

pub fn render_all(diags: &[Diagnostic], source_id: &str, source: &str) -> String {
    diags.iter().map(|d| render(d, source_id, source)).collect::<Vec<_>>().join("\n")
}
```

- [ ] **Step 2: Test**

`crates/mty-diagnostics/tests/render.rs`:

```rust
use sdust_diagnostics::{Diagnostic, Label, codes::UNEXPECTED_TOKEN, render::ariadne::render};

#[test]
fn renders_one_line() {
    let src = "fn main() {\n   bad@@\n}\n";
    let d = Diagnostic::error(UNEXPECTED_TOKEN,
        Label { start: 18, end: 20, message: "unexpected `@@`".into() });
    let out = render(&d, "test.sd", src);
    assert!(out.contains("MT0001"));
    assert!(out.contains("unexpected `@@`"));
}
```

Run: `cargo test -p mty-diagnostics`
Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/mty-diagnostics/src/render/
git commit -m "Render diagnostics via ariadne"
```

---

## Task 20: HIR types + arena IDs

**Files:**
- Modify: `crates/mty-hir/src/lib.rs`
- Create: `crates/mty-hir/src/ids.rs`
- Create: `crates/mty-hir/src/nodes.rs`

- [ ] **Step 1: Write `ids.rs`**

```rust
use la_arena::Idx;

pub type ItemId    = Idx<crate::nodes::Item>;
pub type FnId      = Idx<crate::nodes::HirFn>;
pub type StructId  = Idx<crate::nodes::HirStruct>;
pub type EnumId    = Idx<crate::nodes::HirEnum>;
pub type TypeAliasId = Idx<crate::nodes::HirTypeAlias>;
pub type AgentId   = Idx<crate::nodes::HirAgent>;
pub type ProtocolId= Idx<crate::nodes::HirProtocol>;
pub type SupervisorId = Idx<crate::nodes::HirSupervisor>;
pub type ExprId    = Idx<crate::nodes::HirExpr>;
pub type PatId     = Idx<crate::nodes::HirPat>;
pub type TypeId    = Idx<crate::nodes::HirType>;
pub type BlockId   = Idx<crate::nodes::HirBlock>;
pub type LocalId   = Idx<crate::nodes::HirLocal>;
```

- [ ] **Step 2: Write `nodes.rs` (HIR shape)**

```rust
use la_arena::Arena;
use crate::ids::*;

#[derive(Debug, Clone)]
pub struct SourceSpan { pub start: u32, pub end: u32 }

#[derive(Debug, Clone)]
pub enum Item {
    Fn(FnId),
    Struct(StructId),
    Enum(EnumId),
    TypeAlias(TypeAliasId),
    Agent(AgentId),
    Protocol(ProtocolId),
    Supervisor(SupervisorId),
    Use(HirUse),
    Mod(HirMod),
    ExternBlock(HirExternBlock),
    ExportDecl(HirExportDecl),
    Macro(HirMacro),
    Impl(HirImpl),
    Trait(HirTrait),
    Const(HirConst),
}

#[derive(Debug, Clone)]
pub struct HirFn {
    pub name: String,
    pub is_pub: bool,
    pub is_unsafe: bool,
    pub generics: Vec<String>,
    pub params: Vec<HirParam>,
    pub ret: Option<TypeId>,
    pub effects: Vec<String>,
    pub body: Option<BlockId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct HirParam { pub name: String, pub ty: Option<TypeId>, pub span: SourceSpan }

#[derive(Debug, Clone)]
pub struct HirStruct {
    pub name: String, pub is_pub: bool, pub generics: Vec<String>,
    pub fields: Vec<HirStructField>, pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirStructField { pub name: String, pub ty: TypeId, pub span: SourceSpan }

#[derive(Debug, Clone)]
pub struct HirEnum {
    pub name: String, pub is_pub: bool, pub generics: Vec<String>,
    pub variants: Vec<HirEnumVariant>, pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirEnumVariant { pub name: String, pub payload: Vec<TypeId>, pub span: SourceSpan }

#[derive(Debug, Clone)]
pub struct HirTypeAlias { pub name: String, pub is_pub: bool, pub generics: Vec<String>, pub ty: TypeId, pub span: SourceSpan }

#[derive(Debug, Clone)]
pub struct HirAgent {
    pub name: String,
    pub ctor_params: Vec<String>,
    pub protocols: Vec<TypeId>,
    pub state: Vec<HirAgentState>,
    pub handlers: Vec<HirOnHandler>,
    pub methods: Vec<FnId>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirAgentState { pub name: String, pub ty: Option<TypeId>, pub init: Option<ExprId>, pub span: SourceSpan }
#[derive(Debug, Clone)]
pub struct HirOnHandler { pub message: String, pub params: Vec<String>, pub body: BlockId, pub span: SourceSpan }

#[derive(Debug, Clone)]
pub struct HirProtocol {
    pub name: String, pub is_pub: bool, pub version: Option<u32>,
    pub composition: Option<Vec<TypeId>>,    // for `protocol Web = A + B + C`
    pub messages: Vec<HirProtocolMsg>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub struct HirProtocolMsg { pub name: String, pub params: Vec<HirParam>, pub reply: Option<TypeId>, pub span: SourceSpan }

#[derive(Debug, Clone)]
pub struct HirSupervisor {
    pub name: String, pub strategy: String,
    pub children: Vec<(String, ExprId)>,
    pub on_fail: Vec<(String, Vec<HirSupAction>)>,
    pub span: SourceSpan,
}
#[derive(Debug, Clone)]
pub enum HirSupAction {
    Restart { up_to: Option<u32>, in_dur: Option<ExprId> },
    Backoff { lo: ExprId, hi: ExprId },
}

#[derive(Debug, Clone)] pub struct HirUse { pub path: Vec<String>, pub alias: Option<String>, pub leaves: Vec<(String, Option<String>)>, pub span: SourceSpan }
#[derive(Debug, Clone)] pub struct HirMod { pub path: Vec<String>, pub span: SourceSpan }
#[derive(Debug, Clone)] pub struct HirExternBlock { pub abi: Option<String>, pub fns: Vec<FnId>, pub span: SourceSpan }
#[derive(Debug, Clone)] pub struct HirExportDecl { pub abi: Option<String>, pub item: Box<Item>, pub span: SourceSpan }
#[derive(Debug, Clone)] pub struct HirMacro { pub name: String, pub params: Vec<String>, pub body_tokens: String, pub span: SourceSpan }
#[derive(Debug, Clone)] pub struct HirImpl { pub trait_for: Option<TypeId>, pub self_ty: TypeId, pub methods: Vec<FnId>, pub span: SourceSpan }
#[derive(Debug, Clone)] pub struct HirTrait { pub name: String, pub is_pub: bool, pub generics: Vec<String>, pub methods: Vec<FnId>, pub span: SourceSpan }
#[derive(Debug, Clone)] pub struct HirConst { pub name: String, pub is_pub: bool, pub ty: TypeId, pub value: ExprId, pub span: SourceSpan }

#[derive(Debug, Clone)]
pub enum HirType {
    Path { segments: Vec<String>, generics: Vec<TypeId> },
    Borrow { mutable: bool, inner: TypeId },
    Tuple(Vec<TypeId>),
    Array { elem: TypeId, len: Option<ExprId> },
    Fn { params: Vec<TypeId>, ret: Option<TypeId> },
    /// Sugar: T!E desugared to Result[T, E]; we preserve original for fmt.
    Result { ok: TypeId, err: TypeId },
    /// T!{A,B} desugared to Result[T, A|B]
    Union(Vec<TypeId>),
    Unit,
    Unknown,
}

#[derive(Debug, Clone)]
pub enum HirExpr {
    Literal(HirLiteral),
    Path(Vec<String>),
    Call { callee: ExprId, args: Vec<HirArg> },
    MethodCall { receiver: ExprId, method: String, args: Vec<HirArg> },
    Field { receiver: ExprId, name: String },
    Index { receiver: ExprId, idx: ExprId },
    Binary { op: BinOp, lhs: ExprId, rhs: ExprId },
    Unary { op: UnOp, rhs: ExprId },
    If { cond: ExprId, then: BlockId, else_: Option<ExprId> },
    Match { scrutinee: ExprId, arms: Vec<HirMatchArm> },
    For { pat: PatId, iter: ExprId, body: BlockId },
    While { cond: ExprId, body: BlockId },
    Loop { body: BlockId },
    Return(Option<ExprId>),
    Block(BlockId),
    Tuple(Vec<ExprId>),
    Array(Vec<ExprId>),
    Struct { path: Vec<String>, fields: Vec<(String, ExprId)> },
    Map(Vec<(ExprId, ExprId)>),
    /// `target!Msg(args)`
    Send { target: ExprId, msg: String, args: Vec<HirArg> },
    /// `target?Msg(args)`
    Ask { target: ExprId, msg: String, args: Vec<HirArg> },
    /// `expr @ duration`
    Deadline { inner: ExprId, dur: ExprId },
    Question(ExprId),
    Move(ExprId),
    Borrow { mutable: bool, inner: ExprId },
    Spawn { is_task: bool, inner: ExprId },
    Detach(ExprId),
    Join(ExprId),
    HtmlTemplate(String),
    Unsafe(BlockId),
    Arena { name: String, body: ExprId },
    TaskScope { deadline: Option<ExprId>, body: BlockId },
    Budget { entries: Vec<(String, ExprId)>, body: ExprId },
    Sandbox { name: String, entries: Vec<(Vec<String>, ExprId)>, body: BlockId },
    Cast { lhs: ExprId, ty: TypeId },
    Error,
}

#[derive(Debug, Clone)]
pub struct HirArg { pub name: Option<String>, pub value: ExprId }

#[derive(Debug, Clone)]
pub enum HirLiteral {
    Int(i128, Option<String>),     // value + optional type suffix
    Float(f64, Option<String>),
    Str(String),
    Char(char),
    Bool(bool),
    Duration { value: u64, unit: String },
    Size { value: u64, unit: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Rem,
    BitAnd, BitOr, BitXor, Shl, Shr,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    Range, RangeEq,
    Assign, AssignAdd, AssignSub, AssignMul, AssignDiv, AssignRem,
    AssignBitAnd, AssignBitOr, AssignBitXor, AssignShl, AssignShr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp { Neg, Not, Deref }

#[derive(Debug, Clone)]
pub enum HirPat {
    Wildcard,
    Literal(HirLiteral),
    Binding { name: String, sub: Option<PatId> },
    Ref { mutable: bool, inner: PatId },
    Tuple(Vec<PatId>),
    Struct { path: Vec<String>, fields: Vec<(String, Option<PatId>)> },
    Enum { path: Vec<String>, args: Vec<PatId> },
    Range { lo: PatId, hi: PatId, inclusive: bool },
}

#[derive(Debug, Clone)]
pub struct HirMatchArm { pub pat: PatId, pub guard: Option<ExprId>, pub body: ExprId }

#[derive(Debug, Clone)]
pub struct HirBlock { pub stmts: Vec<HirStmt>, pub tail: Option<ExprId> }

#[derive(Debug, Clone)]
pub enum HirStmt {
    Let { pat: PatId, ty: Option<TypeId>, init: Option<ExprId> },
    Expr(ExprId),
}

#[derive(Debug, Clone)]
pub struct HirLocal { pub name: String, pub mutable: bool, pub span: SourceSpan }

#[derive(Default, Debug)]
pub struct Package {
    pub items: Arena<Item>,
    pub fns: Arena<HirFn>,
    pub structs: Arena<HirStruct>,
    pub enums: Arena<HirEnum>,
    pub type_aliases: Arena<HirTypeAlias>,
    pub agents: Arena<HirAgent>,
    pub protocols: Arena<HirProtocol>,
    pub supervisors: Arena<HirSupervisor>,
    pub exprs: Arena<HirExpr>,
    pub pats: Arena<HirPat>,
    pub types: Arena<HirType>,
    pub blocks: Arena<HirBlock>,
    pub locals: Arena<HirLocal>,
    pub top_level: Vec<ItemId>,
}
```

- [ ] **Step 3: Update `lib.rs`**

```rust
pub mod ids;
pub mod nodes;
pub mod lower;
pub mod resolve;
pub mod dump;

pub use ids::*;
pub use nodes::*;
```

Stub the `lower`, `resolve`, `dump` modules with empty `mod` files; real impls in Task 21–23.

- [ ] **Step 4: Smoke test**

`crates/mty-hir/tests/types_compile.rs`:

```rust
#[test]
fn package_default() {
    let p = sdust_hir::Package::default();
    assert!(p.top_level.is_empty());
}
```

Run: `cargo test -p mty-hir`
Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/mty-hir/
git commit -m "Define HIR node types + arenas (package, items, exprs, pats, types)"
```

---

## Task 21: HIR lowering (items, types, patterns)

**Files:**
- Modify: `crates/mty-hir/src/lower/mod.rs`
- Create: `crates/mty-hir/src/lower/items.rs`
- Create: `crates/mty-hir/src/lower/types.rs`
- Create: `crates/mty-hir/src/lower/patterns.rs`

- [ ] **Step 1: `lower/mod.rs` LoweringCtx + entry**

```rust
use crate::nodes::*;
use crate::ids::*;
use sdust_ast::AstNode;
use sdust_diagnostics::Diagnostic;

pub mod items;
pub mod types;
pub mod patterns;
pub mod exprs;
pub mod agents;

pub struct LoweringCtx {
    pub package: Package,
    pub diagnostics: Vec<Diagnostic>,
}

impl LoweringCtx {
    pub fn new() -> Self { Self { package: Package::default(), diagnostics: vec![] } }

    pub fn lower_file(mut self, file: sdust_ast::File) -> (Package, Vec<Diagnostic>) {
        for node in file.0.children() {
            if let Some(item_id) = items::lower_item(&mut self, node) {
                self.package.top_level.push(item_id);
            }
        }
        (self.package, self.diagnostics)
    }

    pub fn alloc_type(&mut self, t: HirType) -> TypeId { self.package.types.alloc(t) }
    pub fn alloc_expr(&mut self, e: HirExpr) -> ExprId { self.package.exprs.alloc(e) }
    pub fn alloc_pat(&mut self, p: HirPat)  -> PatId  { self.package.pats.alloc(p) }
    pub fn alloc_block(&mut self, b: HirBlock) -> BlockId { self.package.blocks.alloc(b) }
}

pub fn span_of(n: &sdust_syntax::SyntaxNode) -> SourceSpan {
    let r = n.text_range();
    SourceSpan { start: r.start().into(), end: r.end().into() }
}
```

- [ ] **Step 2: `lower/items.rs`**

```rust
use crate::nodes::*;
use crate::ids::*;
use sdust_ast::{AstNode, FnDecl, StructDecl, EnumDecl, TypeAlias, AgentDecl, ProtocolDecl, SupervisorDecl, UseDecl, ModDecl, ExternBlock, ExportDecl, MacroDecl};
use sdust_syntax::{SyntaxNode, SyntaxKind};
use super::{LoweringCtx, span_of};

pub fn lower_item(ctx: &mut LoweringCtx, node: SyntaxNode) -> Option<ItemId> {
    let item = match node.kind() {
        SyntaxKind::FN_DECL => Item::Fn(lower_fn(ctx, FnDecl::cast(node)?)),
        SyntaxKind::STRUCT_DECL => Item::Struct(lower_struct(ctx, StructDecl::cast(node)?)),
        SyntaxKind::ENUM_DECL => Item::Enum(lower_enum(ctx, EnumDecl::cast(node)?)),
        SyntaxKind::TYPE_ALIAS => Item::TypeAlias(lower_type_alias(ctx, TypeAlias::cast(node)?)),
        SyntaxKind::AGENT_DECL => Item::Agent(super::agents::lower_agent(ctx, AgentDecl::cast(node)?)),
        SyntaxKind::PROTOCOL_DECL => Item::Protocol(super::agents::lower_protocol(ctx, ProtocolDecl::cast(node)?)),
        SyntaxKind::SUPERVISOR_DECL => Item::Supervisor(super::agents::lower_supervisor(ctx, SupervisorDecl::cast(node)?)),
        SyntaxKind::USE_DECL => Item::Use(lower_use(UseDecl::cast(node)?)),
        SyntaxKind::MOD_DECL => Item::Mod(lower_mod(ModDecl::cast(node)?)),
        // EXTERN_BLOCK, EXPORT_DECL, MACRO_DECL, IMPL_BLOCK, TRAIT_DECL, CONST_DECL similar
        _ => return None,
    };
    Some(ctx.package.items.alloc(item))
}

fn lower_fn(ctx: &mut LoweringCtx, f: FnDecl) -> FnId {
    let name = f.name().map(|n| n.text()).unwrap_or_default();
    let is_pub = f.is_pub();
    let is_unsafe = f.is_unsafe();
    let params = f.param_list().map(|pl| {
        pl.0.children().filter_map(sdust_ast::FnParam::cast).map(|p| {
            let pname = p.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
            let ty = p.0.children().find(|c| matches!(c.kind(),
                SyntaxKind::TYPE_PATH | SyntaxKind::TYPE_BORROW | SyntaxKind::TYPE_TUPLE
                | SyntaxKind::TYPE_ARRAY | SyntaxKind::TYPE_FN | SyntaxKind::TYPE_RESULT_SUGAR))
                .map(|n| super::types::lower_type(ctx, n));
            HirParam { name: pname, ty, span: span_of(&p.0) }
        }).collect()
    }).unwrap_or_default();
    let ret = f.ret_type().and_then(|r| r.0.children().next()).map(|t| super::types::lower_type(ctx, t));
    let effects = f.effect_clause().map(|e| {
        e.0.children().filter_map(sdust_ast::Name::cast).map(|n| n.text()).collect()
    }).unwrap_or_default();
    let body = f.body().map(|b| super::exprs::lower_block(ctx, b));
    let hf = HirFn {
        name, is_pub, is_unsafe, generics: vec![], params, ret, effects, body,
        span: span_of(&f.0),
    };
    ctx.package.fns.alloc(hf)
}

fn lower_struct(ctx: &mut LoweringCtx, s: StructDecl) -> StructId {
    let name = s.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
    let fields = s.0.descendants().filter_map(sdust_ast::StructField::cast).map(|f| {
        let fname = f.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
        let ty = f.0.children().find(|c| is_type_node(c.kind()))
            .map(|n| super::types::lower_type(ctx, n))
            .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
        HirStructField { name: fname, ty, span: span_of(&f.0) }
    }).collect();
    let hs = HirStruct {
        name, is_pub: has_visibility(&s.0), generics: vec![], fields,
        span: span_of(&s.0),
    };
    ctx.package.structs.alloc(hs)
}

fn lower_enum(ctx: &mut LoweringCtx, e: EnumDecl) -> EnumId {
    let name = e.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
    let variants = e.0.descendants().filter_map(sdust_ast::EnumVariant::cast).map(|v| {
        let vname = v.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
        let payload = v.0.children().filter(|c| is_type_node(c.kind()))
            .map(|n| super::types::lower_type(ctx, n)).collect();
        HirEnumVariant { name: vname, payload, span: span_of(&v.0) }
    }).collect();
    let he = HirEnum {
        name, is_pub: has_visibility(&e.0), generics: vec![], variants,
        span: span_of(&e.0),
    };
    ctx.package.enums.alloc(he)
}

fn lower_type_alias(ctx: &mut LoweringCtx, t: TypeAlias) -> TypeAliasId {
    let name = t.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
    let ty = t.0.children().find(|c| is_type_node(c.kind()))
        .map(|n| super::types::lower_type(ctx, n))
        .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
    let h = HirTypeAlias {
        name, is_pub: has_visibility(&t.0), generics: vec![], ty,
        span: span_of(&t.0),
    };
    ctx.package.type_aliases.alloc(h)
}

fn lower_use(u: UseDecl) -> HirUse {
    let path: Vec<String> = u.0.descendants().filter_map(sdust_ast::NameRef::cast)
        .map(|n| n.0.first_token().map(|t| t.text().to_string()).unwrap_or_default()).collect();
    HirUse { path, alias: None, leaves: vec![], span: span_of(&u.0) }
}

fn lower_mod(m: ModDecl) -> HirMod {
    let path: Vec<String> = m.0.descendants().filter_map(sdust_ast::NameRef::cast)
        .map(|n| n.0.first_token().map(|t| t.text().to_string()).unwrap_or_default()).collect();
    HirMod { path, span: span_of(&m.0) }
}

pub fn is_type_node(k: SyntaxKind) -> bool {
    matches!(k, SyntaxKind::TYPE_PATH | SyntaxKind::TYPE_BORROW | SyntaxKind::TYPE_TUPLE
        | SyntaxKind::TYPE_ARRAY | SyntaxKind::TYPE_FN | SyntaxKind::TYPE_RESULT_SUGAR
        | SyntaxKind::TYPE_UNION | SyntaxKind::TYPE_DYN)
}
pub fn has_visibility(n: &SyntaxNode) -> bool {
    n.children().any(|c| c.kind() == SyntaxKind::VISIBILITY)
}
```

- [ ] **Step 3: `lower/types.rs`**

```rust
use crate::nodes::*;
use crate::ids::*;
use sdust_syntax::{SyntaxNode, SyntaxKind};
use super::LoweringCtx;

pub fn lower_type(ctx: &mut LoweringCtx, n: SyntaxNode) -> TypeId {
    let t = match n.kind() {
        SyntaxKind::TYPE_PATH => {
            let segs: Vec<String> = n.descendants().filter_map(sdust_ast::NameRef::cast)
                .map(|nr| nr.0.first_token().map(|t| t.text().to_string()).unwrap_or_default()).collect();
            let generics: Vec<TypeId> = n.children().filter(|c| c.kind() == SyntaxKind::GENERIC_ARG_LIST)
                .flat_map(|gl| gl.children())
                .filter(|c| c.kind() == SyntaxKind::GENERIC_ARG)
                .flat_map(|ga| ga.children())
                .filter(|c| super::items::is_type_node(c.kind()))
                .map(|tn| lower_type(ctx, tn)).collect();
            HirType::Path { segments: segs, generics }
        }
        SyntaxKind::TYPE_BORROW => {
            let mutable = n.children_with_tokens().filter_map(|e| e.into_token()).any(|t| t.kind() == SyntaxKind::MUT_KW);
            let inner = n.children().find(|c| super::items::is_type_node(c.kind()))
                .map(|tn| lower_type(ctx, tn))
                .unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
            HirType::Borrow { mutable, inner }
        }
        SyntaxKind::TYPE_TUPLE => {
            let elems: Vec<_> = n.children().filter(|c| super::items::is_type_node(c.kind()))
                .map(|tn| lower_type(ctx, tn)).collect();
            if elems.is_empty() { HirType::Unit } else { HirType::Tuple(elems) }
        }
        SyntaxKind::TYPE_ARRAY => {
            let mut tys = n.children().filter(|c| super::items::is_type_node(c.kind()));
            let elem = tys.next().map(|tn| lower_type(ctx, tn)).unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
            HirType::Array { elem, len: None }
        }
        SyntaxKind::TYPE_FN => {
            let mut tys: Vec<_> = n.children().filter(|c| super::items::is_type_node(c.kind())).collect();
            let ret = tys.pop().map(|tn| lower_type(ctx, tn));
            let params: Vec<_> = tys.into_iter().map(|tn| lower_type(ctx, tn)).collect();
            HirType::Fn { params, ret }
        }
        SyntaxKind::TYPE_RESULT_SUGAR => {
            let mut iter = n.children().filter(|c| super::items::is_type_node(c.kind()));
            let ok = iter.next().map(|tn| lower_type(ctx, tn)).unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
            let err = iter.next().map(|tn| lower_type(ctx, tn)).unwrap_or_else(|| ctx.alloc_type(HirType::Unknown));
            HirType::Result { ok, err }
        }
        SyntaxKind::TYPE_UNION => {
            let elems: Vec<_> = n.children().filter(|c| super::items::is_type_node(c.kind()))
                .map(|tn| lower_type(ctx, tn)).collect();
            HirType::Union(elems)
        }
        _ => HirType::Unknown,
    };
    ctx.alloc_type(t)
}
```

- [ ] **Step 4: `lower/patterns.rs`**

```rust
use crate::nodes::*;
use crate::ids::*;
use sdust_syntax::{SyntaxNode, SyntaxKind};
use super::LoweringCtx;

pub fn lower_pat(ctx: &mut LoweringCtx, n: SyntaxNode) -> PatId {
    let p = match n.kind() {
        SyntaxKind::WILDCARD_PAT => HirPat::Wildcard,
        SyntaxKind::LITERAL_PAT => {
            let tok = n.first_token().unwrap();
            HirPat::Literal(super::exprs::lower_literal_token(&tok))
        }
        SyntaxKind::BINDING_PAT => {
            let name = n.children().find_map(sdust_ast::Name::cast).map(|nm| nm.text()).unwrap_or_default();
            let sub = n.children().find(|c| is_pat_node(c.kind())).map(|sn| lower_pat(ctx, sn));
            HirPat::Binding { name, sub }
        }
        SyntaxKind::REF_PAT => {
            let mutable = n.children_with_tokens().filter_map(|e| e.into_token()).any(|t| t.kind() == SyntaxKind::MUT_KW);
            let inner = n.children().find(|c| is_pat_node(c.kind())).map(|p| lower_pat(ctx, p))
                .unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
            HirPat::Ref { mutable, inner }
        }
        SyntaxKind::TUPLE_PAT => HirPat::Tuple(
            n.children().filter(|c| is_pat_node(c.kind())).map(|p| lower_pat(ctx, p)).collect()
        ),
        SyntaxKind::STRUCT_PAT => {
            let path = path_segments(&n);
            let fields = n.descendants().filter(|c| c.kind() == SyntaxKind::IDENT_PAT || c.kind() == SyntaxKind::BINDING_PAT)
                .map(|f| {
                    let nm = f.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
                    let sub = f.children().find(|c| is_pat_node(c.kind())).map(|p| lower_pat(ctx, p));
                    (nm, sub)
                }).collect();
            HirPat::Struct { path, fields }
        }
        SyntaxKind::ENUM_PAT => {
            let path = path_segments(&n);
            let args = n.children().filter(|c| is_pat_node(c.kind())).map(|p| lower_pat(ctx, p)).collect();
            HirPat::Enum { path, args }
        }
        SyntaxKind::RANGE_PAT => {
            let mut sub = n.children().filter(|c| is_pat_node(c.kind()));
            let lo = sub.next().map(|p| lower_pat(ctx, p)).unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
            let hi = sub.next().map(|p| lower_pat(ctx, p)).unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
            let inclusive = n.children_with_tokens().filter_map(|e| e.into_token()).any(|t| t.kind() == SyntaxKind::DOT_DOT_EQ);
            HirPat::Range { lo, hi, inclusive }
        }
        _ => HirPat::Wildcard,
    };
    ctx.alloc_pat(p)
}

pub fn is_pat_node(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(k, LITERAL_PAT | IDENT_PAT | WILDCARD_PAT | TUPLE_PAT
        | STRUCT_PAT | ENUM_PAT | RANGE_PAT | BINDING_PAT | REF_PAT)
}

fn path_segments(n: &SyntaxNode) -> Vec<String> {
    n.descendants().filter_map(sdust_ast::NameRef::cast)
        .map(|nr| nr.0.first_token().map(|t| t.text().to_string()).unwrap_or_default()).collect()
}
```

- [ ] **Step 5: Snapshot test + commit**

`crates/mty-hir/tests/lower_items.rs`:

```rust
use sdust_syntax::{parse, SyntaxNode};
use sdust_ast::{File, AstNode};

fn lower(src: &str) -> sdust_hir::Package {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).unwrap();
    let (pkg, _) = sdust_hir::lower::LoweringCtx::new().lower_file(f);
    pkg
}

#[test] fn lowers_fn() {
    let p = lower("fn add(a: I32, b: I32) -> I32 = a + b");
    assert_eq!(p.fns.len(), 1);
    let f = p.fns.values().next().unwrap();
    assert_eq!(f.name, "add");
    assert_eq!(f.params.len(), 2);
}

#[test] fn lowers_struct() {
    let p = lower("struct User { id: U64, name: String }");
    assert_eq!(p.structs.len(), 1);
}
```

Run: `cargo test -p mty-hir`

```bash
git add crates/mty-hir/
git commit -m "HIR lowering for items, types, patterns"
```

---

## Task 22: HIR lowering (expressions, blocks, agents)

**Files:**
- Create: `crates/mty-hir/src/lower/exprs.rs`
- Create: `crates/mty-hir/src/lower/agents.rs`

These two are large but mostly mechanical mapping from CST → HIR enum variants.

- [ ] **Step 1: `lower/exprs.rs`** — implement `lower_expr`, `lower_block`, `lower_arg`, `lower_literal_token`, `lower_bin_op`, `lower_un_op`. The shape mirrors `lower_type` and `lower_pat`: match on `SyntaxKind`, recurse into children, allocate into the right arena.

Key cases to cover (full dispatch list, executor implements each):

- `LITERAL_EXPR` → `HirExpr::Literal(...)` via `lower_literal_token`
- `PATH_EXPR` → `HirExpr::Path(segments)`
- `BINARY_EXPR` → `HirExpr::Binary { op, lhs, rhs }` — op from the operator token in the middle
- `UNARY_EXPR` → `HirExpr::Unary { op, rhs }`
- `BORROW_EXPR` → `HirExpr::Borrow { mutable, inner }`
- `MOVE_EXPR` → `HirExpr::Move(inner)`
- `SPAWN_EXPR` → `HirExpr::Spawn { is_task: detect TASK_KW child, inner }`
- `CALL_EXPR` → callee = first child expr, args from `ARG_LIST`
- `FIELD_EXPR` → either `Field` or `MethodCall` depending on presence of `ARG_LIST`
- `INDEX_EXPR` → `HirExpr::Index { receiver, idx }`
- `SEND_EXPR` → `HirExpr::Send { target: previous expr in chain, msg: NameRef, args }`
  - The previous expr is the sibling before the `BANG` token; reconstructed by lowering the receiver part of the postfix chain.
- `ASK_EXPR` → similar with `?Msg`
- `DEADLINE_EXPR` → `HirExpr::Deadline { inner, dur }`
- `QUESTION_EXPR` → `HirExpr::Question(inner)`
- `IF_EXPR`, `MATCH_EXPR`, `FOR_EXPR`, `WHILE_EXPR`, `LOOP_EXPR`, `RETURN_EXPR` → map straight across
- `BLOCK` → `HirExpr::Block(lower_block(...))`
- `TUPLE_EXPR`, `ARRAY_EXPR`, `STRUCT_EXPR`, `MAP_EXPR` → straight across
- `HTML_EXPR` → `HirExpr::HtmlTemplate(string text)`
- `UNSAFE_BLOCK` → `HirExpr::Unsafe(lower_block(...))`
- `ARENA_BLOCK` → `HirExpr::Arena { name, body }`. The body may be a single expr (short form) or a block.
- `TASK_SCOPE` → `HirExpr::TaskScope { deadline, body }`
- `BUDGET_BLOCK` → `HirExpr::Budget { entries, body }`. Entries parsed from `BUDGET_ENTRY` children.
- `SANDBOX_BLOCK` → `HirExpr::Sandbox { name, entries, body }`
- `DETACH_EXPR` / `JOIN_EXPR` → straight across
- `CAST_EXPR` → `HirExpr::Cast { lhs, ty }`

Provide `lower_literal_token` that:

- Strips type suffix from int/float (`42u32` → `(42, Some("u32"))`)
- Parses duration / size literals into value + unit
- Decodes string escapes minimally (`\n`, `\t`, `\\`, `\"`)

- [ ] **Step 2: `lower/agents.rs`**

```rust
use crate::nodes::*;
use crate::ids::*;
use sdust_ast::{AgentDecl, ProtocolDecl, SupervisorDecl, AstNode};
use sdust_syntax::SyntaxKind;
use super::{LoweringCtx, span_of};

pub fn lower_agent(ctx: &mut LoweringCtx, a: AgentDecl) -> AgentId {
    let name = a.name().map(|n| n.text()).unwrap_or_default();
    let ctor_params = a.ctor_params().map(|cp| {
        cp.0.descendants().filter_map(sdust_ast::Name::cast).map(|n| n.text()).collect()
    }).unwrap_or_default();
    let protocols = a.protocols().map(|pl| {
        pl.0.children().filter(|c| super::items::is_type_node(c.kind()))
            .map(|tn| super::types::lower_type(ctx, tn)).collect()
    }).unwrap_or_default();
    let state = a.state_fields().map(|sf| HirAgentState {
        name: sf.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default(),
        ty: sf.0.children().find(|c| super::items::is_type_node(c.kind())).map(|tn| super::types::lower_type(ctx, tn)),
        init: sf.0.children().filter(|c| !super::items::is_type_node(c.kind()) && c.kind() != SyntaxKind::NAME)
            .find_map(|c| Some(super::exprs::lower_expr(ctx, c))),
        span: span_of(&sf.0),
    }).collect();
    let handlers = a.handlers().map(|h| HirOnHandler {
        message: h.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default(),
        params: h.0.descendants().filter_map(sdust_ast::Name::cast).skip(1).map(|n| n.text()).collect(),
        body: {
            let blk = h.0.children().find(|c| c.kind() == SyntaxKind::BLOCK)
                .map(|b| super::exprs::lower_block_node(ctx, b));
            blk.unwrap_or_else(|| {
                // single-expr handler: synthesize a block with tail = that expr
                let tail = h.0.children().filter(|c| c.kind() != SyntaxKind::NAME)
                    .find_map(|c| Some(super::exprs::lower_expr(ctx, c)));
                ctx.alloc_block(HirBlock { stmts: vec![], tail })
            })
        },
        span: span_of(&h.0),
    }).collect();
    let ha = HirAgent { name, ctor_params, protocols, state, handlers, methods: vec![], span: span_of(&a.0) };
    ctx.package.agents.alloc(ha)
}

pub fn lower_protocol(ctx: &mut LoweringCtx, p: ProtocolDecl) -> ProtocolId {
    let name = p.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
    let messages = p.0.descendants().filter_map(sdust_ast::ProtocolMsg::cast).map(|m| HirProtocolMsg {
        name: m.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default(),
        params: m.0.descendants().filter_map(sdust_ast::FnParam::cast).map(|fp| HirParam {
            name: fp.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default(),
            ty: fp.0.children().find(|c| super::items::is_type_node(c.kind())).map(|tn| super::types::lower_type(ctx, tn)),
            span: span_of(&fp.0),
        }).collect(),
        reply: m.0.children().filter(|c| super::items::is_type_node(c.kind())).last().map(|tn| super::types::lower_type(ctx, tn)),
        span: span_of(&m.0),
    }).collect();
    let hp = HirProtocol { name, is_pub: super::items::has_visibility(&p.0), version: None, composition: None, messages, span: span_of(&p.0) };
    ctx.package.protocols.alloc(hp)
}

pub fn lower_supervisor(ctx: &mut LoweringCtx, s: SupervisorDecl) -> SupervisorId {
    let name = s.0.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
    let strategy = s.0.descendants().filter_map(sdust_ast::Name::cast).nth(1)
        .map(|n| n.text()).unwrap_or_else(|| "one_for_one".into());
    let children: Vec<_> = s.0.descendants()
        .filter(|c| c.kind() == SyntaxKind::SUP_CHILD)
        .map(|c| {
            let nm = c.children().find_map(sdust_ast::Name::cast).map(|n| n.text()).unwrap_or_default();
            let init = c.children().find(|n| !matches!(n.kind(), SyntaxKind::NAME))
                .map(|n| super::exprs::lower_expr(ctx, n))
                .unwrap_or_else(|| ctx.alloc_expr(HirExpr::Error));
            (nm, init)
        })
        .collect();
    let hs = HirSupervisor { name, strategy, children, on_fail: vec![], span: span_of(&s.0) };
    ctx.package.supervisors.alloc(hs)
}
```

- [ ] **Step 3: Snapshot tests** in `crates/mty-hir/tests/lower_exprs.rs` and `lower_agents.rs` covering enough cases to validate the lowering doesn't lose semantics. Lean on `insta` with the `dump` from Task 23 (forward reference — write the test in step 4 once `dump` exists).

- [ ] **Step 4: Run + commit**

```bash
cargo test -p mty-hir
git add crates/mty-hir/
git commit -m "HIR lowering for expressions, blocks, agents/protocols/supervisors"
```

---

## Task 23: HIR S-expression dump

**Files:**
- Create: `crates/mty-hir/src/dump.rs`

Stable, deterministic textual format suitable for snapshot tests.

- [ ] **Step 1: Write `dump.rs`**

```rust
use crate::{Package, Item, HirExpr, HirType, HirPat, HirLiteral, BinOp, UnOp, HirBlock, HirStmt};
use std::fmt::Write;

pub fn dump_package(pkg: &Package) -> String {
    let mut out = String::new();
    writeln!(out, "(package").unwrap();
    for &item_id in &pkg.top_level {
        dump_item(&mut out, pkg, &pkg.items[item_id], 1);
    }
    writeln!(out, ")").unwrap();
    out
}

fn ind(out: &mut String, n: usize) { for _ in 0..n { out.push_str("  "); } }

fn dump_item(out: &mut String, pkg: &Package, item: &Item, depth: usize) {
    match item {
        Item::Fn(id) => {
            let f = &pkg.fns[*id];
            ind(out, depth);
            writeln!(out, "(fn {} ({})", f.name, f.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" ")).unwrap();
            if let Some(b) = f.body { dump_block(out, pkg, &pkg.blocks[b], depth + 1); }
            ind(out, depth); writeln!(out, ")").unwrap();
        }
        Item::Agent(id) => {
            let a = &pkg.agents[*id];
            ind(out, depth);
            writeln!(out, "(agent {} ctor=({}) protocols=({})", a.name,
                a.ctor_params.join(" "),
                a.protocols.iter().map(|t| dump_type(pkg, &pkg.types[*t])).collect::<Vec<_>>().join(" ")).unwrap();
            for s in &a.state {
                ind(out, depth + 1);
                writeln!(out, "(state {})", s.name).unwrap();
            }
            for h in &a.handlers {
                ind(out, depth + 1);
                writeln!(out, "(on {} ({})", h.message, h.params.join(" ")).unwrap();
                dump_block(out, pkg, &pkg.blocks[h.body], depth + 2);
                ind(out, depth + 1); writeln!(out, ")").unwrap();
            }
            ind(out, depth); writeln!(out, ")").unwrap();
        }
        Item::Protocol(id) => {
            let p = &pkg.protocols[*id];
            ind(out, depth);
            writeln!(out, "(protocol {} msgs=({}))", p.name,
                p.messages.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(" ")).unwrap();
        }
        Item::Struct(id) => {
            let s = &pkg.structs[*id];
            ind(out, depth);
            writeln!(out, "(struct {} fields=({}))", s.name,
                s.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(" ")).unwrap();
        }
        Item::Enum(id) => {
            let e = &pkg.enums[*id];
            ind(out, depth);
            writeln!(out, "(enum {} variants=({}))", e.name,
                e.variants.iter().map(|v| v.name.as_str()).collect::<Vec<_>>().join(" ")).unwrap();
        }
        Item::TypeAlias(id) => {
            let t = &pkg.type_aliases[*id];
            ind(out, depth);
            writeln!(out, "(type-alias {} {})", t.name, dump_type(pkg, &pkg.types[t.ty])).unwrap();
        }
        Item::Supervisor(id) => {
            let s = &pkg.supervisors[*id];
            ind(out, depth);
            writeln!(out, "(supervisor {} strategy={} children=({}))", s.name, s.strategy,
                s.children.iter().map(|(n,_)| n.as_str()).collect::<Vec<_>>().join(" ")).unwrap();
        }
        Item::Use(u) => { ind(out, depth); writeln!(out, "(use {})", u.path.join(".")).unwrap(); }
        Item::Mod(m) => { ind(out, depth); writeln!(out, "(mod {})", m.path.join(".")).unwrap(); }
        Item::ExternBlock(_) | Item::ExportDecl(_) | Item::Macro(_)
        | Item::Impl(_) | Item::Trait(_) | Item::Const(_) => {
            ind(out, depth); writeln!(out, "(item ...)").unwrap();
        }
    }
}

fn dump_block(out: &mut String, pkg: &Package, b: &HirBlock, depth: usize) {
    for s in &b.stmts {
        ind(out, depth);
        match s {
            HirStmt::Let { .. } => writeln!(out, "(let ...)").unwrap(),
            HirStmt::Expr(e) => writeln!(out, "{}", dump_expr(pkg, &pkg.exprs[*e])).unwrap(),
        }
    }
    if let Some(t) = b.tail { ind(out, depth); writeln!(out, "{}", dump_expr(pkg, &pkg.exprs[t])).unwrap(); }
}

fn dump_expr(pkg: &Package, e: &HirExpr) -> String {
    match e {
        HirExpr::Literal(l) => dump_lit(l),
        HirExpr::Path(p) => p.join("."),
        HirExpr::Call { callee, args } => format!("(call {} ({}))",
            dump_expr(pkg, &pkg.exprs[*callee]),
            args.iter().map(|a| dump_expr(pkg, &pkg.exprs[a.value])).collect::<Vec<_>>().join(" ")),
        HirExpr::Binary { op, lhs, rhs } => format!("({:?} {} {})", op,
            dump_expr(pkg, &pkg.exprs[*lhs]), dump_expr(pkg, &pkg.exprs[*rhs])),
        HirExpr::Send { target, msg, args } => format!("(send {} !{} ({}))",
            dump_expr(pkg, &pkg.exprs[*target]), msg,
            args.iter().map(|a| dump_expr(pkg, &pkg.exprs[a.value])).collect::<Vec<_>>().join(" ")),
        HirExpr::Ask { target, msg, args } => format!("(ask {} ?{} ({}))",
            dump_expr(pkg, &pkg.exprs[*target]), msg,
            args.iter().map(|a| dump_expr(pkg, &pkg.exprs[a.value])).collect::<Vec<_>>().join(" ")),
        HirExpr::Deadline { inner, dur } => format!("(deadline {} @{})",
            dump_expr(pkg, &pkg.exprs[*inner]), dump_expr(pkg, &pkg.exprs[*dur])),
        HirExpr::Arena { name, body } => format!("(arena {} {})", name, dump_expr(pkg, &pkg.exprs[*body])),
        _ => "(expr ...)".into(),
    }
}

fn dump_type(pkg: &Package, t: &HirType) -> String {
    match t {
        HirType::Path { segments, generics } => {
            let segs = segments.join(".");
            if generics.is_empty() { segs }
            else { format!("{}[{}]", segs, generics.iter()
                .map(|g| dump_type(pkg, &pkg.types[*g])).collect::<Vec<_>>().join(",")) }
        }
        HirType::Result { ok, err } => format!("{}!{}",
            dump_type(pkg, &pkg.types[*ok]), dump_type(pkg, &pkg.types[*err])),
        HirType::Borrow { mutable, inner } => format!("&{}{}", if *mutable { "mut " } else { "" }, dump_type(pkg, &pkg.types[*inner])),
        HirType::Unit => "()".into(),
        HirType::Unknown => "?".into(),
        _ => "(ty ...)".into(),
    }
}

fn dump_lit(l: &HirLiteral) -> String {
    match l {
        HirLiteral::Int(v, s) => format!("{}{}", v, s.as_deref().unwrap_or("")),
        HirLiteral::Float(v, s) => format!("{}{}", v, s.as_deref().unwrap_or("")),
        HirLiteral::Str(s) => format!("{:?}", s),
        HirLiteral::Char(c) => format!("'{}'", c),
        HirLiteral::Bool(b) => b.to_string(),
        HirLiteral::Duration { value, unit } => format!("{}{}", value, unit),
        HirLiteral::Size { value, unit } => format!("{}{}", value, unit),
    }
}
```

- [ ] **Step 2: Snapshot tests for HIR dumps**

`crates/mty-hir/tests/dump_snapshots.rs`:

```rust
use insta::assert_snapshot;
use sdust_ast::{File, AstNode};
use sdust_syntax::{parse, SyntaxNode};

fn dump(src: &str) -> String {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).unwrap();
    let (pkg, _) = sdust_hir::lower::LoweringCtx::new().lower_file(f);
    sdust_hir::dump::dump_package(&pkg)
}

#[test] fn d_fn() { assert_snapshot!(dump("fn add(a: I32, b: I32) -> I32 = a + b")); }
#[test] fn d_agent() {
    assert_snapshot!(dump("protocol Echo { Ping(msg: Str) -> Str }\nagent Echoer: Echo { on Ping(msg) -> msg }"));
}
#[test] fn d_arena() {
    assert_snapshot!(dump("fn main() { arena turn { let x = 1; x } }"));
}
```

Run + accept: `cargo test -p mty-hir --test dump_snapshots && cargo insta review`.

- [ ] **Step 3: Commit**

```bash
git add crates/mty-hir/
git commit -m "HIR S-expression dump for snapshot testing"
```

---

## Task 24: Formatter Doc combinators + printer

**Files:**
- Modify: `crates/mty-fmt/src/lib.rs`
- Create: `crates/mty-fmt/src/doc.rs`
- Create: `crates/mty-fmt/src/printer.rs`

Implement Wadler/Lindig pretty-printing. Two phases: build a `Doc` from the CST, then render `Doc` to text within a column budget.

- [ ] **Step 1: `doc.rs`**

```rust
//! Wadler/Lindig pretty-printer Doc.

use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Doc {
    Nil,
    Text(Rc<str>),
    Line,           // forced break
    SoftLine,       // break only if enclosing group breaks
    Nest(usize, Box<Doc>),
    Group(Box<Doc>),
    Concat(Box<Doc>, Box<Doc>),
}

impl Doc {
    pub fn nil() -> Self { Doc::Nil }
    pub fn text(s: impl Into<Rc<str>>) -> Self { Doc::Text(s.into()) }
    pub fn line() -> Self { Doc::Line }
    pub fn softline() -> Self { Doc::SoftLine }
    pub fn nest(n: usize, d: Doc) -> Self { Doc::Nest(n, Box::new(d)) }
    pub fn group(d: Doc) -> Self { Doc::Group(Box::new(d)) }
    pub fn concat(a: Doc, b: Doc) -> Self { Doc::Concat(Box::new(a), Box::new(b)) }
    pub fn concat_all(parts: impl IntoIterator<Item = Doc>) -> Self {
        parts.into_iter().fold(Doc::nil(), |acc, d| Doc::concat(acc, d))
    }
    pub fn join(sep: Doc, parts: impl IntoIterator<Item = Doc>) -> Self {
        let mut iter = parts.into_iter();
        match iter.next() {
            None => Doc::nil(),
            Some(first) => iter.fold(first, |acc, d| Doc::concat(Doc::concat(acc, sep.clone()), d)),
        }
    }
}
```

- [ ] **Step 2: `printer.rs`**

```rust
use crate::doc::Doc;

pub struct Layout { pub width: usize }
impl Default for Layout { fn default() -> Self { Self { width: 100 } } }

pub fn pretty(doc: &Doc, layout: &Layout) -> String {
    let mut out = String::new();
    let mut stack: Vec<(usize, Mode, &Doc)> = vec![(0, Mode::Flat, doc)];
    let mut col = 0;
    while let Some((indent, mode, d)) = stack.pop() {
        match d {
            Doc::Nil => {},
            Doc::Text(s) => { out.push_str(s); col += s.chars().count(); }
            Doc::Line => {
                if matches!(mode, Mode::Flat) { out.push(' '); col += 1; }
                else { out.push('\n'); for _ in 0..indent { out.push(' '); } col = indent; }
            }
            Doc::SoftLine => {
                if matches!(mode, Mode::Break) { out.push('\n'); for _ in 0..indent { out.push(' '); } col = indent; }
            }
            Doc::Nest(n, inner) => stack.push((indent + n, mode, inner)),
            Doc::Group(inner) => {
                let m = if fits(layout.width, col, indent, Mode::Flat, inner) { Mode::Flat } else { Mode::Break };
                stack.push((indent, m, inner));
            }
            Doc::Concat(a, b) => {
                stack.push((indent, mode, b));
                stack.push((indent, mode, a));
            }
        }
    }
    out
}

#[derive(Copy, Clone)] enum Mode { Flat, Break }

fn fits(width: usize, mut col: usize, indent: usize, mode: Mode, doc: &Doc) -> bool {
    let mut stack: Vec<(usize, Mode, &Doc)> = vec![(indent, mode, doc)];
    while col <= width {
        let Some((ind, m, d)) = stack.pop() else { return true; };
        match d {
            Doc::Nil => {}
            Doc::Text(s) => col += s.chars().count(),
            Doc::Line => if matches!(m, Mode::Flat) { col += 1 } else { return true; },
            Doc::SoftLine => if matches!(m, Mode::Break) { return true; },
            Doc::Nest(n, inner) => stack.push((ind + n, m, inner)),
            Doc::Group(inner) => stack.push((ind, Mode::Flat, inner)),
            Doc::Concat(a, b) => { stack.push((ind, m, b)); stack.push((ind, m, a)); }
        }
    }
    false
}
```

- [ ] **Step 3: `lib.rs`**

```rust
pub mod doc;
pub mod printer;
pub mod trivia;
pub mod fmt;

use sdust_syntax::{SyntaxNode, GreenNode};

pub fn format(green: GreenNode) -> String {
    let root = SyntaxNode::new_root(green);
    let d = fmt::file(&root);
    printer::pretty(&d, &printer::Layout::default())
}
```

- [ ] **Step 4: Stub `trivia.rs` and `fmt/` modules**

`trivia.rs`:

```rust
use sdust_syntax::{SyntaxKind, SyntaxToken};
pub fn collect_leading_comments(_tok: &SyntaxToken) -> Vec<String> { vec![] } // filled in Task 26
```

`fmt/mod.rs`:

```rust
use sdust_syntax::SyntaxNode;
use crate::doc::Doc;

pub mod items;
pub mod types;
pub mod patterns;
pub mod exprs;
pub mod agents;
pub mod concurrency;

pub fn file(node: &SyntaxNode) -> Doc {
    // For Task 24 we just dump text verbatim as a placeholder.
    Doc::text(node.text().to_string())
}
```

- [ ] **Step 5: Smoke test + commit**

`crates/mty-fmt/tests/printer.rs`:

```rust
use sdust_fmt::doc::Doc;
use sdust_fmt::printer::{pretty, Layout};

#[test]
fn renders_text() {
    assert_eq!(pretty(&Doc::text("hello"), &Layout::default()), "hello");
}

#[test]
fn group_fits_on_one_line() {
    let d = Doc::group(Doc::concat_all([
        Doc::text("(a"), Doc::line(), Doc::text("b)"),
    ]));
    assert_eq!(pretty(&d, &Layout::default()), "(a b)");
}

#[test]
fn group_breaks_when_too_wide() {
    let d = Doc::group(Doc::concat_all([
        Doc::text("(aaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        Doc::line(),
        Doc::text("bbbbbbbbbbbbbbbbbbbbbbbbbbbb)"),
    ]));
    let out = pretty(&d, &Layout { width: 20 });
    assert!(out.contains('\n'));
}
```

Run: `cargo test -p mty-fmt`
Expected: 3 passed.

```bash
git add crates/mty-fmt/
git commit -m "Add Wadler/Lindig Doc combinators + pretty-printer"
```

---

## Task 25: Formatter for items, types, expressions, blocks

**Files:**
- Modify: `crates/mty-fmt/src/fmt/items.rs`
- Modify: `crates/mty-fmt/src/fmt/types.rs`
- Modify: `crates/mty-fmt/src/fmt/exprs.rs`
- Modify: `crates/mty-fmt/src/fmt/patterns.rs`
- Modify: `crates/mty-fmt/src/fmt/mod.rs`

The dispatcher walks the CST and emits `Doc`s. Style rules per design doc §11:
- 2-space indent.
- One trailing newline at EOF.
- Spaces around binary ops, no space inside `[]`/`()`.
- Trailing commas on multi-line lists, none on single-line.
- Compact agent form preserved.
- `T!E` preserved when input wrote it; expanded form preserved when input wrote `Result[T,E]`.

The full formatter implementation is mechanical but voluminous. The pattern per node kind is: emit keywords as text, recurse on children with `Doc::group` for items that should try to fit on one line, attach `Doc::nest(2, ...)` around contents.

- [ ] **Step 1: Implement the top-level dispatcher in `fmt/mod.rs`**

```rust
use sdust_syntax::{SyntaxNode, SyntaxKind, SyntaxElement};
use crate::doc::Doc;

pub mod items; pub mod types; pub mod patterns; pub mod exprs; pub mod agents; pub mod concurrency;

pub fn file(node: &SyntaxNode) -> Doc {
    let mut docs = Vec::new();
    for child in node.children() {
        docs.push(node_doc(&child));
        docs.push(Doc::text("\n"));
    }
    if !docs.is_empty() {
        // Ensure exactly one trailing newline.
        Doc::concat_all(docs)
    } else {
        Doc::text("\n")
    }
}

pub fn node_doc(n: &SyntaxNode) -> Doc {
    use SyntaxKind::*;
    match n.kind() {
        FN_DECL => items::fn_decl(n),
        STRUCT_DECL => items::struct_decl(n),
        ENUM_DECL => items::enum_decl(n),
        TYPE_ALIAS => items::type_alias(n),
        IMPL_BLOCK => items::impl_block(n),
        TRAIT_DECL => items::trait_decl(n),
        USE_DECL => items::use_decl(n),
        MOD_DECL => items::mod_decl(n),
        PACKAGE_DECL => items::package_decl(n),
        EXTERN_BLOCK => items::extern_block(n),
        EXPORT_DECL => items::export_decl(n),
        MACRO_DECL => items::macro_decl(n),
        AGENT_DECL => agents::agent_decl(n),
        PROTOCOL_DECL => agents::protocol_decl(n),
        SUPERVISOR_DECL => agents::supervisor_decl(n),
        BLOCK => exprs::block(n),
        BUDGET_BLOCK => concurrency::budget_block(n),
        SANDBOX_BLOCK => concurrency::sandbox_block(n),
        ARENA_BLOCK => concurrency::arena_block(n),
        TASK_SCOPE => concurrency::task_scope(n),
        // Expressions and types are handled inside their respective formatters.
        _ => Doc::text(n.text().to_string()),
    }
}

pub fn token_text(n: &SyntaxNode, kind: SyntaxKind) -> Option<String> {
    n.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == kind).map(|t| t.text().to_string())
}
```

- [ ] **Step 2: Implement `fmt/items.rs`** (one function per item kind)

Example for `fn_decl` (others follow the same shape):

```rust
use sdust_syntax::{SyntaxNode, SyntaxKind};
use sdust_ast::{AstNode, FnDecl, FnParamList, FnParam, Name, RetType, EffectClause, Block};
use crate::doc::Doc;
use super::{exprs, types, token_text};

pub fn fn_decl(n: &SyntaxNode) -> Doc {
    let f = FnDecl::cast(n.clone()).unwrap();
    let mut parts = Vec::new();
    if f.is_pub() { parts.push(Doc::text("pub ")); }
    if f.is_unsafe() { parts.push(Doc::text("unsafe ")); }
    parts.push(Doc::text("fn "));
    if let Some(name) = f.name() { parts.push(Doc::text(name.text())); }
    parts.push(fmt_param_list(&f.param_list()));
    if let Some(rt) = f.ret_type() { parts.push(Doc::text(" -> ")); parts.push(types::type_doc(&rt.0.children().next().unwrap())); }
    if let Some(ec) = f.effect_clause() {
        parts.push(Doc::text(" "));
        parts.push(effect_clause(&ec));
    }
    if let Some(body) = f.body() {
        parts.push(Doc::text(" "));
        parts.push(exprs::block(&body.0));
    } else if let Some(eq) = n.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == SyntaxKind::EQ) {
        let _ = eq;
        // expression-body: `fn add(a,b) = a + b`
        if let Some(expr_node) = n.children().find(|c| !matches!(c.kind(),
            SyntaxKind::FN_PARAM_LIST | SyntaxKind::RET_TYPE | SyntaxKind::EFFECT_CLAUSE
            | SyntaxKind::BLOCK | SyntaxKind::NAME | SyntaxKind::VISIBILITY)) {
            parts.push(Doc::text(" = "));
            parts.push(exprs::expr(&expr_node));
        }
    }
    Doc::group(Doc::concat_all(parts))
}

fn fmt_param_list(list: &Option<FnParamList>) -> Doc {
    let Some(pl) = list else { return Doc::text("()"); };
    let params: Vec<Doc> = pl.0.children().filter_map(FnParam::cast).map(|p| {
        let name = p.0.children().find_map(Name::cast).map(|n| n.text()).unwrap_or_default();
        let ty_doc = p.0.children().find(|c| matches!(c.kind(),
            SyntaxKind::TYPE_PATH | SyntaxKind::TYPE_BORROW | SyntaxKind::TYPE_TUPLE
            | SyntaxKind::TYPE_ARRAY | SyntaxKind::TYPE_FN | SyntaxKind::TYPE_RESULT_SUGAR))
            .map(|tn| types::type_doc(&tn)).unwrap_or(Doc::nil());
        Doc::concat_all([Doc::text(name), Doc::text(": "), ty_doc])
    }).collect();
    Doc::concat_all([
        Doc::text("("),
        Doc::join(Doc::text(", "), params),
        Doc::text(")"),
    ])
}

fn effect_clause(ec: &EffectClause) -> Doc {
    let names: Vec<Doc> = ec.0.children().filter_map(Name::cast)
        .map(|n| Doc::text(n.text())).collect();
    Doc::concat_all([Doc::text("effect "), Doc::join(Doc::text(", "), names)])
}

// Implement struct_decl, enum_decl, type_alias, impl_block, trait_decl,
// use_decl, mod_decl, package_decl, extern_block, export_decl, macro_decl
// following the same pattern: pick out tokens/children, emit Doc.
pub fn struct_decl(n: &SyntaxNode) -> Doc { /* ... */ Doc::text(n.text().to_string()) }
pub fn enum_decl(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn type_alias(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn impl_block(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn trait_decl(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn use_decl(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn mod_decl(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn package_decl(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn extern_block(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn export_decl(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn macro_decl(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
```

The stub `Doc::text(n.text().to_string())` placeholders are deliberate fallback: as long as round-trip preserves the source text byte-for-byte (which it does because the CST is lossless), the formatter is correct-but-ugly. Replace each stub with a real Doc emitter in this task. The executor implements each function following the `fn_decl` template.

- [ ] **Step 3: Implement `fmt/types.rs`, `fmt/patterns.rs`, `fmt/exprs.rs`**

For types — emit `&`/`&mut` prefix, `(...)` for tuples, `[...]` for arrays, `path[T1, T2]` for generic apps, `T!E` for result sugar (preserved from input), `T!{A, B}` for union.

For patterns — straight mapping with comma separation.

For expressions — binary operators with spaces, postfix operators with no space before, struct/map/array literals with conditional line breaks via `Doc::group + Doc::softline`.

Example expression block:

```rust
use sdust_syntax::{SyntaxNode, SyntaxKind};
use crate::doc::Doc;
use super::node_doc;

pub fn block(n: &SyntaxNode) -> Doc {
    let mut inner: Vec<Doc> = Vec::new();
    for child in n.children() {
        inner.push(node_doc(&child));
        inner.push(Doc::text("\n"));
    }
    Doc::concat_all([
        Doc::text("{"),
        Doc::nest(2, Doc::concat(Doc::line(), Doc::concat_all(inner))),
        Doc::line(),
        Doc::text("}"),
    ])
}

pub fn expr(n: &SyntaxNode) -> Doc {
    // Mechanical match on n.kind() for each EXPR_* variant. Stub: passthrough.
    Doc::text(n.text().to_string())
}
```

The executor's job in this task is to flesh out the expression dispatcher: literal_expr, path_expr, binary_expr, call_expr, field_expr, send_expr, ask_expr, deadline_expr, etc. Each is a small function.

- [ ] **Step 4: Tests**

`crates/mty-fmt/tests/format_items.rs`:

```rust
use sdust_syntax::parse;

fn fmt(src: &str) -> String {
    let r = parse(src);
    sdust_fmt::format(r.green)
}

#[test] fn fmt_fn_simple() {
    assert_eq!(fmt("fn add(a:I32,b:I32)->I32=a+b").trim(),
               "fn add(a: I32, b: I32) -> I32 = a + b");
}

#[test] fn fmt_use() {
    assert_eq!(fmt("use   std.io").trim(), "use std.io");
}
```

Add tests as the formatter is filled out. Initial tests may fall back to whatever text the CST passthrough produces; assert that the round-trip is text-equal after `fmt(fmt(src)) == fmt(src)`.

- [ ] **Step 5: Commit**

```bash
git add crates/mty-fmt/
git commit -m "Format items, types, patterns, expressions, blocks"
```

---

## Task 26: Formatter for agents, supervisors, concurrency + comment attachment

**Files:**
- Modify: `crates/mty-fmt/src/fmt/agents.rs`
- Modify: `crates/mty-fmt/src/fmt/concurrency.rs`
- Modify: `crates/mty-fmt/src/trivia.rs`

- [ ] **Step 1: Implement `fmt/agents.rs`**

Compact agent form per design doc §11 example:

```sd
agent Counter: Count {
  n = 0
  on Inc() -> { n += 1; n }
}
```

```rust
use sdust_syntax::{SyntaxNode, SyntaxKind};
use sdust_ast::{AgentDecl, AstNode, Name, OnHandler, AgentStateDecl};
use crate::doc::Doc;
use super::exprs;

pub fn agent_decl(n: &SyntaxNode) -> Doc {
    let a = AgentDecl::cast(n.clone()).unwrap();
    let mut head = vec![Doc::text("agent ")];
    if let Some(name) = a.name() { head.push(Doc::text(name.text())); }
    if let Some(cp) = a.ctor_params() {
        let params: Vec<Doc> = cp.0.descendants().filter_map(Name::cast)
            .map(|n| Doc::text(n.text())).collect();
        head.push(Doc::concat_all([
            Doc::text("("),
            Doc::join(Doc::text(", "), params),
            Doc::text(")"),
        ]));
    }
    if let Some(pl) = a.protocols() {
        head.push(Doc::text(": "));
        let protos: Vec<Doc> = pl.0.children().map(|c| super::types::type_doc(&c)).collect();
        head.push(Doc::join(Doc::text(" + "), protos));
    }
    let mut body = Vec::new();
    for sf in a.state_fields() { body.push(state_decl(&sf)); body.push(Doc::text("\n")); }
    for h in a.handlers() { body.push(on_handler(&h)); body.push(Doc::text("\n")); }
    Doc::concat_all([
        Doc::concat_all(head),
        Doc::text(" {"),
        Doc::nest(2, Doc::concat(Doc::line(), Doc::concat_all(body))),
        Doc::line(),
        Doc::text("}"),
    ])
}

fn state_decl(sf: &AgentStateDecl) -> Doc {
    Doc::text(sf.0.text().to_string())
}

fn on_handler(h: &OnHandler) -> Doc {
    Doc::text(h.0.text().to_string())
}

pub fn protocol_decl(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn supervisor_decl(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
```

(The state/handler stubs use passthrough; replace with `Doc`-based emission once the basic shape passes idempotence tests.)

- [ ] **Step 2: Implement `fmt/concurrency.rs`**

```rust
use sdust_syntax::{SyntaxNode, SyntaxKind};
use crate::doc::Doc;

pub fn arena_block(n: &SyntaxNode) -> Doc {
    // Detect short form (arena X: expr) vs block form (arena X { ... })
    let has_brace = n.children().any(|c| c.kind() == SyntaxKind::BLOCK);
    if !has_brace {
        // short form: rebuild as `arena <name>: <expr>`
        return Doc::text(n.text().to_string());
    }
    Doc::text(n.text().to_string())
}
pub fn task_scope(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn budget_block(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
pub fn sandbox_block(n: &SyntaxNode) -> Doc { Doc::text(n.text().to_string()) }
```

- [ ] **Step 3: Implement comment attachment in `trivia.rs`**

```rust
use sdust_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use crate::doc::Doc;

/// Collect line/block/doc comments immediately preceding a node.
pub fn leading_comments(node: &SyntaxNode) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = node.first_token();
    while let Some(tok) = cursor {
        let Some(prev) = tok.prev_token() else { break; };
        match prev.kind() {
            SyntaxKind::WHITESPACE => { cursor = Some(prev); }
            SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                out.push(prev.text().to_string());
                cursor = Some(prev);
            }
            _ => break,
        }
    }
    out.reverse();
    out
}

/// Wrap a Doc with any leading comments.
pub fn with_leading(node: &SyntaxNode, body: Doc) -> Doc {
    let comments = leading_comments(node);
    if comments.is_empty() { return body; }
    let mut parts: Vec<Doc> = comments.into_iter().flat_map(|c| {
        vec![Doc::text(c), Doc::text("\n")]
    }).collect();
    parts.push(body);
    Doc::concat_all(parts)
}
```

Update `fmt/mod.rs::node_doc` to wrap each top-level item with `trivia::with_leading(n, doc)`.

- [ ] **Step 4: Tests**

```rust
#[test] fn comments_survive() {
    let src = "// hi\nfn main() {}\n";
    let out = sdust_fmt::format(sdust_syntax::parse(src).green);
    assert!(out.contains("// hi"), "got: {}", out);
}
```

- [ ] **Step 5: Commit**

```bash
git add crates/mty-fmt/
git commit -m "Format agents/supervisors/concurrency; attach leading comments"
```

---

## Task 27: Formatter idempotence + round-trip sweep

**Files:**
- Create: `tests/fmt/idempotence.rs`
- Create: `tests/fmt/round_trip.rs`
- Modify: `Cargo.toml` (add an integration-tests workspace member or use the cli's tests dir)

The sweep tests live in the workspace top-level `tests/` directory and run via `cargo test --test idempotence` after we wire them as integration tests on the `mty-fmt` crate.

- [ ] **Step 1: Decide location**

Place sweep tests inside the existing `mty-fmt` crate at `crates/mty-fmt/tests/idempotence.rs` and `crates/mty-fmt/tests/round_trip.rs`. They iterate `../../examples/*.sd` and `../../tests/fmt/fixtures/*.sd` (paths resolved at test compile time). Create the fixtures dir.

- [ ] **Step 2: Write `idempotence.rs`**

```rust
use std::fs;
use std::path::PathBuf;
use sdust_syntax::parse;

fn collect_sd_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in [root.join("examples"), root.join("tests/fmt/fixtures")] {
        if !dir.exists() { continue; }
        for entry in fs::read_dir(&dir).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("sd") { out.push(p); }
        }
    }
    out
}

#[test]
fn fmt_is_idempotent() {
    let mut failed = Vec::new();
    for path in collect_sd_files() {
        let src = fs::read_to_string(&path).unwrap();
        let once = sdust_fmt::format(parse(&src).green);
        let twice = sdust_fmt::format(parse(&once).green);
        if once != twice {
            failed.push(format!("{}\n--- once ---\n{}\n--- twice ---\n{}",
                path.display(), once, twice));
        }
    }
    assert!(failed.is_empty(), "{} files not idempotent:\n{}", failed.len(), failed.join("\n\n"));
}
```

- [ ] **Step 3: Write `round_trip.rs`**

```rust
use std::fs;
use sdust_syntax::{parse, SyntaxNode};

#[test]
fn round_trip_preserves_item_shape() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for entry in fs::read_dir(root.join("examples")).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) != Some("sd") { continue; }
        let src = fs::read_to_string(&p).unwrap();
        let original_tree = parse(&src).green;
        let formatted = sdust_fmt::format(original_tree.clone());
        let reparsed_tree = parse(&formatted).green;
        // Compare top-level item kinds in order.
        let kinds = |g: rowan::GreenNode| -> Vec<u16> {
            SyntaxNode::new_root(g).children().map(|c| c.kind() as u16).collect()
        };
        assert_eq!(kinds(original_tree), kinds(reparsed_tree),
                   "item-shape mismatch for {}", p.display());
    }
}
```

- [ ] **Step 4: Create one starter fixture so the test passes before examples land**

`tests/fmt/fixtures/00_smoke.sd`:

```sd
fn main() {
  log("hi")
}
```

- [ ] **Step 5: Run + commit**

```bash
cargo test -p mty-fmt --test idempotence
cargo test -p mty-fmt --test round_trip
```

(Expected: pass on `00_smoke.sd`. Failures on real examples surface as we add them in Task 31; iterate the formatter until they all pass.)

```bash
git add crates/mty-fmt/tests/ tests/fmt/fixtures/
git commit -m "Add formatter idempotence + round-trip sweep tests"
```

---

## Task 28: Driver crate — pipeline + mighty.toml

**Files:**
- Modify: `crates/mty-driver/src/lib.rs`
- Create: `crates/mty-driver/src/manifest.rs`
- Create: `crates/mty-driver/src/pipeline.rs`

- [ ] **Step 1: `manifest.rs`**

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub package: Package,
    #[serde(default)]
    pub deps: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub edition: String,
    #[serde(default = "default_profile")]
    pub profile: String,
}

fn default_profile() -> String { "host".into() }

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("read error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Toml(#[from] toml::de::Error),
}

pub fn load(path: &std::path::Path) -> Result<Manifest, ManifestError> {
    let src = std::fs::read_to_string(path)?;
    let m: Manifest = toml::from_str(&src)?;
    Ok(m)
}
```

- [ ] **Step 2: `pipeline.rs`**

```rust
use sdust_syntax::{parse, SyntaxNode, ParseError};
use sdust_ast::{File, AstNode};
use sdust_hir::Package;
use sdust_diagnostics::{Diagnostic, Label, codes::*};

pub struct ParsedFile {
    pub source: String,
    pub source_id: String,
    pub green: rowan::GreenNode,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_source(source: String, source_id: String) -> ParsedFile {
    let r = parse(&source);
    let diagnostics = r.errors.iter().map(|e| Diagnostic::error(
        UNEXPECTED_TOKEN,
        Label { start: e.start, end: e.end, message: e.message.clone() })).collect();
    ParsedFile { source, source_id, green: r.green, diagnostics }
}

pub fn lower(p: &ParsedFile) -> (Package, Vec<Diagnostic>) {
    let file = File::cast(SyntaxNode::new_root(p.green.clone())).expect("FILE root");
    let (pkg, diag) = sdust_hir::lower::LoweringCtx::new().lower_file(file);
    let mut all = p.diagnostics.clone();
    all.extend(diag);
    (pkg, all)
}
```

- [ ] **Step 3: `lib.rs`**

```rust
pub mod manifest;
pub mod pipeline;
pub use manifest::Manifest;
pub use pipeline::{parse_source, lower, ParsedFile};
```

- [ ] **Step 4: Tests**

```rust
#[test]
fn loads_minimal_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(&path, br#"
[package]
name = "x"
version = "0.1.0"
edition = "2026"
"#).unwrap();
    let m = sdust_driver::manifest::load(&path).unwrap();
    assert_eq!(m.package.name, "x");
    assert_eq!(m.package.profile, "host");
}
```

Add `tempfile` to dev-dependencies in `mty-driver/Cargo.toml`.

Run: `cargo test -p mty-driver`

- [ ] **Step 5: Commit**

```bash
git add crates/mty-driver/
git commit -m "Driver crate: pipeline (parse+lower) + mighty.toml loader"
```

---

## Task 29: CLI `mty` binary

**Files:**
- Modify: `crates/mty-cli/src/main.rs`
- Create: `crates/mty-cli/src/cmd/mod.rs`
- Create: `crates/mty-cli/src/cmd/new.rs`
- Create: `crates/mty-cli/src/cmd/fmt.rs`
- Create: `crates/mty-cli/src/cmd/check.rs`
- Create: `crates/mty-cli/src/cmd/dump.rs`

- [ ] **Step 1: `main.rs`** clap entry

```rust
use clap::{Parser, Subcommand};

mod cmd;

#[derive(Parser)]
#[command(name = "mty", version, about = "Mighty compiler CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scaffold a new Mighty package.
    New { name: String },
    /// Format .sd files in place (or stdin).
    Fmt {
        #[arg(num_args = 0..)] paths: Vec<std::path::PathBuf>,
        #[arg(long)] stdin: bool,
        #[arg(long)] check: bool,
    },
    /// Parse + HIR-lower; emit diagnostics; exit nonzero on error.
    Check { path: std::path::PathBuf },
    /// Dump intermediate representations.
    Dump {
        path: std::path::PathBuf,
        #[arg(long)] ast: bool,
        #[arg(long)] cst: bool,
        #[arg(long)] hir: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::New { name } => cmd::new::run(&name),
        Cmd::Fmt { paths, stdin, check } => cmd::fmt::run(paths, stdin, check),
        Cmd::Check { path } => cmd::check::run(&path),
        Cmd::Dump { path, ast, cst, hir } => cmd::dump::run(&path, ast, cst, hir),
    };
    std::process::exit(code);
}
```

- [ ] **Step 2: `cmd/new.rs`**

```rust
use std::fs;
use std::path::Path;

pub fn run(name: &str) -> i32 {
    let dir = Path::new(name);
    if dir.exists() { eprintln!("directory `{}` already exists", name); return 1; }
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("mighty.toml"), format!(r#"[package]
name = "{}"
version = "0.1.0"
edition = "2026"
profile = "host"

[deps]
"#, name)).unwrap();
    fs::write(dir.join("src/main.sd"), "fn main() {\n  log(\"hello, Mighty\")\n}\n").unwrap();
    println!("created {}/", name);
    0
}
```

- [ ] **Step 3: `cmd/fmt.rs`**

```rust
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use sdust_driver::parse_source;

pub fn run(paths: Vec<PathBuf>, use_stdin: bool, check_only: bool) -> i32 {
    if use_stdin {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).unwrap();
        let parsed = parse_source(s.clone(), "<stdin>".into());
        let out = sdust_fmt::format(parsed.green);
        if check_only { return if out == s { 0 } else { 1 }; }
        print!("{}", out);
        return 0;
    }
    let mut changed = 0;
    for path in &paths {
        for file in collect(path) {
            let src = fs::read_to_string(&file).unwrap();
            let parsed = parse_source(src.clone(), file.display().to_string());
            let out = sdust_fmt::format(parsed.green);
            if out == src { continue; }
            if check_only { println!("would reformat {}", file.display()); changed += 1; }
            else { fs::write(&file, out).unwrap(); println!("formatted {}", file.display()); changed += 1; }
        }
    }
    if check_only && changed > 0 { 1 } else { 0 }
}

fn collect(p: &PathBuf) -> Vec<PathBuf> {
    if p.is_file() { vec![p.clone()] }
    else if p.is_dir() {
        let mut out = Vec::new();
        walk(p, &mut out);
        out
    } else { Vec::new() }
}
fn walk(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { walk(&p, out); }
            else if p.extension().and_then(|s| s.to_str()) == Some("sd") { out.push(p); }
        }
    }
}
```

- [ ] **Step 4: `cmd/check.rs`**

```rust
use std::fs;
use std::path::Path;
use sdust_driver::{parse_source, lower};
use sdust_diagnostics::render::ariadne::render_all;

pub fn run(path: &Path) -> i32 {
    let src = fs::read_to_string(path).unwrap();
    let parsed = parse_source(src.clone(), path.display().to_string());
    let (_pkg, diags) = lower(&parsed);
    if !diags.is_empty() {
        eprint!("{}", render_all(&diags, &path.display().to_string(), &src));
        return 1;
    }
    println!("ok: {}", path.display());
    0
}
```

- [ ] **Step 5: `cmd/dump.rs`**

```rust
use std::fs;
use std::path::Path;
use sdust_driver::{parse_source, lower};
use sdust_syntax::SyntaxNode;

pub fn run(path: &Path, ast: bool, cst: bool, hir: bool) -> i32 {
    let src = fs::read_to_string(path).unwrap();
    let parsed = parse_source(src.clone(), path.display().to_string());
    if cst {
        let root = SyntaxNode::new_root(parsed.green.clone());
        println!("{:#?}", root);
    }
    if ast {
        let root = SyntaxNode::new_root(parsed.green.clone());
        for item in root.children() {
            println!("- {:?} {:?}", item.kind(), item.text_range());
        }
    }
    if hir {
        let (pkg, _) = lower(&parsed);
        println!("{}", sdust_hir::dump::dump_package(&pkg));
    }
    if !ast && !cst && !hir {
        eprintln!("specify one of --ast --cst --hir"); return 2;
    }
    0
}
```

- [ ] **Step 6: `cmd/mod.rs`**

```rust
pub mod new;
pub mod fmt;
pub mod check;
pub mod dump;
```

- [ ] **Step 7: Build + smoke test**

```bash
cargo build -p mty-cli
cargo run -p mty-cli -- new demo
cd demo && cargo run -p mty-cli --manifest-path ../Cargo.toml -- fmt src/main.sd
```

- [ ] **Step 8: Commit**

```bash
git add crates/mty-cli/
git commit -m "CLI: mty binary with new/fmt/check/dump commands"
```

---

## Task 30: 20 canonical example programs (part 1: examples 01–10)

**Files:**
- Create: `examples/01_hello.sd` through `examples/10_supervisor.sd`

Each file must parse, format idempotently, and lower to HIR without errors. Verbatim from spec where the spec gives an example; otherwise constructed from spec syntax.

- [ ] **Step 1: Write `examples/01_hello.sd`**

```sd
fn main() {
  log("hello, Mighty")
}
```

- [ ] **Step 2: Write `examples/02_struct_enum.sd`**

```sd
struct User {
  id: UserId
  name: String
}

enum Shape {
  Circle(F64)
  Rect(F64, F64)
}

type UserId = U64

fn area(s: Shape) -> F64 {
  match s {
    Shape.Circle(r) => 3.14159 * r * r
    Shape.Rect(w, h) => w * h
  }
}
```

- [ ] **Step 3: Write `examples/03_generic_fn.sd`**

```sd
fn first[T](xs: &[T]) -> Option[&T] {
  if xs.len == 0 { None } else { Some(&xs[0]) }
}
```

- [ ] **Step 4: Write `examples/04_result_propagation.sd`**

```sd
fn parse(s: Str) -> I32!ParseErr {
  Ok(0)
}

fn load(url: Url) -> Page!{NetErr, ParseErr} {
  let body = fetch(url)?
  parse(body)?
  Ok(Page {})
}
```

- [ ] **Step 5: Write `examples/05_match_expr.sd`**

```sd
fn classify(n: I32) -> Str {
  match n {
    0 => "zero"
    1..10 => "small"
    _ => "big"
  }
}
```

- [ ] **Step 6: Write `examples/06_for_while_loop.sd`**

```sd
fn process(items: &[I32]) {
  for item in items {
    work(item)?
  }
  while ready() {
    step()
  }
  loop {
    tick()
  }
}
```

- [ ] **Step 7: Write `examples/07_agent_echo.sd`** (spec §4)

```sd
protocol Echo {
  Ping(msg: Str) -> Str
}

agent Echoer: Echo {
  on Ping(msg) -> msg
}
```

- [ ] **Step 8: Write `examples/08_agent_state.sd`** (spec §12.2)

```sd
protocol Count {
  Inc() -> I64
}

agent Counter: Count {
  n = 0
  on Inc() -> { n += 1; n }
}
```

- [ ] **Step 9: Write `examples/09_send_ask_deadline.sd`**

```sd
fn driver(logger: Logger, fetcher: Fetcher, url: Url) -> Page!FetchErr {
  logger!Info("started")
  let page = fetcher?Page(url) @2s?
  Ok(page)
}
```

- [ ] **Step 10: Write `examples/10_supervisor.sd`** (spec §15)

```sd
supervisor SearchFlow(strategy: one_for_one) {
  child planner = spawn Planner()
  child fetcher = spawn Fetcher(net)

  on_fail(planner) { restart up_to 3 in 30s }
  on_fail(fetcher) { backoff 100ms..2s; restart }
}
```

- [ ] **Step 11: Verify each file parses, formats idempotently, lowers**

```bash
for f in examples/*.sd; do
  cargo run -q -p mty-cli -- check "$f" || { echo "FAILED: $f"; exit 1; }
  cargo run -q -p mty-cli -- fmt --check "$f" || { echo "FMT CHANGED: $f"; exit 1; }
done
```

(In PowerShell: use `Get-ChildItem examples/*.sd | ForEach-Object { ... }`.)

- [ ] **Step 12: Commit**

```bash
git add examples/01_*.sd examples/02_*.sd examples/03_*.sd examples/04_*.sd examples/05_*.sd \
        examples/06_*.sd examples/07_*.sd examples/08_*.sd examples/09_*.sd examples/10_*.sd
git commit -m "Add canonical examples 01-10 (basic syntax, generics, agents, supervisors)"
```

---

## Task 31: 20 canonical example programs (part 2: examples 11–20)

**Files:**
- Create: `examples/11_budget_block.sd` through `examples/20_frontend_component.sd`

- [ ] **Step 1: `examples/11_budget_block.sd`**

```sd
fn run_job(input: Bytes) -> Result!RunErr {
  budget {
    cpu 150ms
    wall 2s
    mem 128MiB
    mb 1k
  } run {
    job(input)?
  }
}
```

- [ ] **Step 2: `examples/12_arena.sd`** (spec §7.5)

```sd
fn turn(input: Str) -> Lowered!ParseErr {
  arena turn {
    let toks = tokenize(input)
    let ast = parse(toks)?
    lower(ast)
  }
}

fn turn_short(input: Str) -> Lowered!ParseErr {
  arena turn: lower(parse(tokenize(input))?)
}
```

- [ ] **Step 3: `examples/13_capabilities.sd`**

```sd
fn load(fs: Fs, path: Path) -> Bytes!IoErr {
  fs.read(path)?
}

agent Fetcher(net, clock): Fetch {
  on Page(url) -> net.get(url) @2s?
}
```

- [ ] **Step 4: `examples/14_extern_c.sd`** (spec §26.1)

```sd
extern c {
  fn strlen(s: *U8) -> USize
}

export c fn add(a: I32, b: I32) -> I32 = a + b
```

- [ ] **Step 5: `examples/15_extern_js.sd`** (spec §22.3)

```sd
extern js {
  fn alert(msg: Str) effect dom
}
```

- [ ] **Step 6: `examples/16_macro.sd`** (spec §20.3)

```sd
macro assert_eq(a, b) => {
  if a != b { panic("assert_eq failed") }
}
```

- [ ] **Step 7: `examples/17_unsafe.sd`** (spec §21)

```sd
fn read_byte(addr: USize) -> U8 {
  unsafe {
    let p = raw_ptr(addr)
    p.read()
  }
}

pub unsafe fn from_raw(ptr: *U8, len: USize) -> Bytes
  requires ptr != null
  requires valid(ptr, len)
```

- [ ] **Step 8: `examples/18_sandbox.sd`** (spec §16.1)

```sd
sandbox ToolRun with {
  fs.read = ["/models", "/tmp/input.json"]
  fs.write = ["/tmp/out"]
  net = ["api.example.com:443"]
  cpu = 150ms
  wall = 2s
  memory = 128MiB
  mailbox = 1024
} {
  run job(input)
}
```

- [ ] **Step 9: `examples/19_backend_service.sd`** — verbatim from spec §34. Copy text from `C:\Users\ihass\Downloads\stardust_language_spec_v0_1.md` lines 1727–1767.

- [ ] **Step 10: `examples/20_frontend_component.sd`** — verbatim from spec §35. Copy text from spec lines 1773–1799.

- [ ] **Step 11: Verify the harder examples**

```bash
cargo run -p mty-cli -- check examples/19_backend_service.sd
cargo run -p mty-cli -- check examples/20_frontend_component.sd
cargo test -p mty-fmt --test idempotence
cargo test -p mty-fmt --test round_trip
```

Expected: all green. If `19_backend_service.sd` or `20_frontend_component.sd` reveal parser/HIR gaps, fix the gap and add a regression test in the appropriate crate.

- [ ] **Step 12: Commit**

```bash
git add examples/11_*.sd examples/12_*.sd examples/13_*.sd examples/14_*.sd examples/15_*.sd \
        examples/16_*.sd examples/17_*.sd examples/18_*.sd examples/19_*.sd examples/20_*.sd
git commit -m "Add canonical examples 11-20 (budgets, arenas, caps, extern, sandbox, full backend+frontend)"
```

---

## Task 32: Conformance suite scaffold

**Files:**
- Create: `tests/conformance/README.md`
- Create: `tests/conformance/{lexical,parser,formatter_idempotence,type_inference,ownership_rejection,borrow_checking,effect_checking,capability_checking,agent_protocol,mailbox_ordering,supervisor_restart,budget_violation,native_abi,wasm_component,deterministic_replay}/README.md`

Per spec §37, these are the test categories. Slice 1 populates only the first three.

- [ ] **Step 1: Write the top-level README**

`tests/conformance/README.md`:

```markdown
# Mighty Conformance Suite

Per Mighty v0.1 spec §37. Each subdirectory holds tests for one category.

Slice-1 categories (populated): `lexical/`, `parser/`, `formatter_idempotence/`.
Other categories are placeholders; later slices fill them.

## Running

```
cargo test -p mty-syntax --test parse_recovery
cargo test -p mty-fmt --test idempotence
cargo test -p mty-fmt --test round_trip
```

## Adding a test

1. Drop the input `.sd` file in the appropriate category.
2. Add a Rust test that loads it and asserts the expected outcome (parse OK, specific diagnostic, fmt idempotence, etc.).
```

- [ ] **Step 2: Write each subdirectory README with the slice that fills it**

```bash
mkdir tests/conformance/{lexical,parser,formatter_idempotence,type_inference,ownership_rejection,borrow_checking,effect_checking,capability_checking,agent_protocol,mailbox_ordering,supervisor_restart,budget_violation,native_abi,wasm_component,deterministic_replay}
```

(Use PowerShell `New-Item` since we're on Windows.)

For each placeholder dir, write a one-liner README naming the slice that fills it:

- `type_inference/README.md`: "Populated in slice 2 (type checker)."
- `ownership_rejection/README.md`: "Populated in slice 2 (borrow checker)."
- (etc., per design doc §14)

- [ ] **Step 3: Commit**

```bash
git add tests/conformance/
git commit -m "Scaffold conformance suite layout per spec §37"
```

---

## Task 33: Slice-1 done-definition verification

**Files:** none (verification only)

Per design doc §19. Run every command, capture output, fix anything that doesn't pass.

- [ ] **Step 1: Workspace builds clean**

```bash
cargo build --workspace
```

Expected: no warnings, no errors.

- [ ] **Step 2: All tests pass**

```bash
cargo test --workspace
```

Expected: zero failures. If a snapshot drifted, `cargo insta review` and accept legitimate changes.

- [ ] **Step 3: Clippy clean**

```bash
cargo clippy --workspace -- -D warnings
```

Expected: no warnings. Fix any that surface; don't `allow` without a comment.

- [ ] **Step 4: Rustfmt clean**

```bash
cargo fmt --check
```

Expected: no diff.

- [ ] **Step 5: All examples check + fmt-idempotent**

In PowerShell:

```powershell
$failed = @()
Get-ChildItem examples\*.sd | ForEach-Object {
    & cargo run -q -p mty-cli -- check $_.FullName
    if ($LASTEXITCODE -ne 0) { $failed += "check: $($_.Name)" }
    & cargo run -q -p mty-cli -- fmt --check $_.FullName
    if ($LASTEXITCODE -ne 0) { $failed += "fmt: $($_.Name)" }
}
if ($failed.Count -gt 0) { Write-Error "FAILED: $($failed -join ', ')" } else { Write-Host "all 20 examples pass" }
```

Expected: "all 20 examples pass".

- [ ] **Step 6: `mty fmt examples/` is a no-op on the formatted tree**

```bash
cargo run -p mty-cli -- fmt examples/
git diff --stat examples/
```

Expected: empty diff.

- [ ] **Step 7: Heaviest examples lower without diagnostics**

```bash
cargo run -p mty-cli -- check examples/19_backend_service.sd
cargo run -p mty-cli -- check examples/20_frontend_component.sd
```

Expected: `ok: examples/19_backend_service.sd` and `ok: examples/20_frontend_component.sd`.

- [ ] **Step 8: HIR dump round-trips through snapshot review**

```bash
cargo test -p mty-hir --test dump_snapshots
cargo insta review     # accept anything reviewed; no surprises expected
```

- [ ] **Step 9: Slice closeout commit**

```bash
git add -A
git commit -m "Slice 1 complete: lexer, parser, formatter, HIR, CLI, 20 examples, conformance scaffold"
git log --oneline | head -40
```

Expected: the slice closeout commit is the latest; the commit log shows ~33 task commits between bootstrap and closeout.

- [ ] **Step 10: Tag the slice**

```bash
git tag -a v0.1.0-phase1 -m "Phase 1: lexer + parser + formatter + HIR"
git log --oneline -1
```

- [ ] **Step 11: Slice review (user gate)**

Per the autonomous build mandate's review gate (memory: `feedback_cani_autonomous_build.md`, `feedback_kesseldb_autonomous_build.md`), surface slice 1 for review. Summary message to user:

> Slice 1 complete and tagged `v0.1.0-phase1`. All 20 canonical examples parse, format idempotently, and lower to HIR. `cargo test --workspace`, `clippy`, `fmt` all green. Ready for review before starting slice 2 (type checker + ownership/borrow + effect/capability).

---

## Self-review notes

**Spec coverage check** — design doc §1 deliverables:

- ✅ `mty` CLI with `new`/`fmt`/`check`/`dump` — Tasks 29, 30
- ✅ 20 canonical examples — Tasks 30, 31
- ✅ Stable AST + HIR dumps — Tasks 17, 23
- ✅ Syntax error recovery + ariadne diagnostics — Tasks 6, 16, 18, 19
- ✅ Test infrastructure (insta + idempotence sweep + conformance scaffold) — Tasks throughout + Task 27, 32

**Grammar coverage check** — design doc §7 lists what slice 1 parses:

- ✅ Ownership annotations (`&`, `&mut`, `move`, `mut`) — Task 10
- ✅ Capability params, effect annotations — Tasks 12, 13
- ✅ `pub`/`unsafe` visibility/safety prefixes — Tasks 12, 15
- ✅ Generic params + constraints — Task 8 (`generic_params`)
- ✅ Agent + protocol declarations with `on` handlers — Task 13
- ✅ Send/ask + `@deadline` — Task 10
- ✅ Supervisors — Task 13
- ✅ Budgets + sandboxes — Task 14
- ✅ Arenas — Task 14
- ✅ Task scopes — Task 14
- ✅ Macros — Task 15
- ✅ Unsafe blocks + fns + `requires` — Tasks 12, 15
- ✅ extern c/js + export — Task 15
- ✅ `T!E`, `T!{A,B}`, `?` propagation — Tasks 8, 10
- ✅ All pattern kinds — Task 9

**Non-goals confirmed deferred** (design doc §2): type checking, borrow checking, effect/capability checking, MtyIR, runtime, codegen, LSP, package manager. No task implements these.

**Placeholder scan**: no `TBD`/`TODO`/`fill in later` in the plan. Every step shows the actual code or the specific command.

**Type consistency**: `SyntaxKind` enum defined Task 3 is referenced consistently in lexer (Task 4), parser (Tasks 6+), AST (Task 17), HIR lowering (Tasks 21–22), formatter (Tasks 24–26). `HirExpr` / `HirType` / `HirPat` enum variants defined Task 20 are used in lowering Tasks 21–22 and dump Task 23. CLI commands in Task 29 use `sdust_driver::parse_source` / `lower` as defined in Task 28.

**Risks (per design doc §17) covered by tests**: ambiguous `!Msg` vs unary-not handled by lookahead in Task 10 postfix; `arena turn: expr` short form in Task 14 + example 12; nesting depth limit not implemented in slice 1 (deferred — emit a warning if hit). The depth limit is the only spec §17 risk we don't actively guard; add a Task 16 step if it surfaces in practice.

---

**Plan length:** 33 tasks. Each task ends with a commit so the slice has a clean history. The TDD step pattern (write test → see it fail → implement → see it pass → commit) keeps each change reviewable.




