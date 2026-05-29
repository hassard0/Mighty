//! v0.36 T1 — Hex / binary / octal integer literal lowering.
//!
//! Verifies the new `0xFF` / `0xFF_u8` / `0b1010` / `0o777` literal
//! shapes:
//! * Lex as their dedicated `*_INT_LITERAL` token (covered in the
//!   `mty-syntax/tests/lexer.rs` suite).
//! * Lower through `mty-hir`'s `lower_literal_token` into
//!   `HirLiteral::Int(value, suffix)` with the correct numeric value
//!   and the canonical suffix string (`"u8"` / `"i32"` / `"usize"`).

use mty_ast::{AstNode, File};
use mty_hir::{HirExpr, HirLiteral, Package};
use mty_syntax::{parse, SyntaxNode};

fn lower(src: &str) -> Package {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).unwrap();
    let (pkg, _) = mty_hir::lower::LoweringCtx::new().lower_file(f);
    pkg
}

/// Find the first int literal anywhere in the lowered package and
/// return its (value, suffix) pair. The test sources use a single
/// constant-style fn body containing the literal as the trailing expr.
fn first_int_literal(pkg: &Package) -> (i128, Option<String>) {
    for expr in pkg.exprs.iter() {
        if let HirExpr::Literal(HirLiteral::Int(v, suf)) = &expr.1 {
            return (*v, suf.clone());
        }
    }
    panic!("no HirLiteral::Int in lowered package");
}

#[test]
fn hex_bare_lowers_to_int() {
    let pkg = lower("fn answer() -> I32 { 0xFF }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0xFF);
    assert_eq!(s, None);
}

#[test]
fn hex_with_u8_suffix() {
    let pkg = lower("fn answer() -> U8 { 0xFF_u8 }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0xFF);
    assert_eq!(s.as_deref(), Some("u8"));
}

#[test]
fn hex_with_u8_suffix_no_underscore() {
    let pkg = lower("fn answer() -> U8 { 0xFFu8 }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0xFF);
    assert_eq!(s.as_deref(), Some("u8"));
}

#[test]
fn hex_u16_value() {
    let pkg = lower("fn answer() -> U16 { 0xCAFE_u16 }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0xCAFE);
    assert_eq!(s.as_deref(), Some("u16"));
}

#[test]
fn hex_u32_value() {
    let pkg = lower("fn answer() -> U32 { 0xDEAD_BEEF_u32 }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0xDEAD_BEEF);
    assert_eq!(s.as_deref(), Some("u32"));
}

#[test]
fn hex_u64_value() {
    let pkg = lower("fn answer() -> U64 { 0xCAFE_BABE_DEAD_BEEF_u64 }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v as u64, 0xCAFE_BABE_DEAD_BEEF_u64);
    assert_eq!(s.as_deref(), Some("u64"));
}

#[test]
fn hex_i8_negative_via_two_complement() {
    // 0xFF as i8 is -1 — but the parser stores the raw u128 value,
    // typeck does the wrap. We just check the suffix here.
    let pkg = lower("fn answer() -> I8 { 0xFF_i8 }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0xFF);
    assert_eq!(s.as_deref(), Some("i8"));
}

#[test]
fn hex_i16_value() {
    let pkg = lower("fn answer() -> I16 { 0xFF_i16 }");
    let (_, s) = first_int_literal(&pkg);
    assert_eq!(s.as_deref(), Some("i16"));
}

#[test]
fn hex_i32_value() {
    let pkg = lower("fn answer() -> I32 { 0x7FFF_FFFF_i32 }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0x7FFF_FFFF);
    assert_eq!(s.as_deref(), Some("i32"));
}

#[test]
fn hex_i64_value() {
    let pkg = lower("fn answer() -> I64 { 0x1_i64 }");
    let (_, s) = first_int_literal(&pkg);
    assert_eq!(s.as_deref(), Some("i64"));
}

#[test]
fn hex_usize_suffix() {
    let pkg = lower("fn answer() -> USize { 0x10_usize }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0x10);
    assert_eq!(s.as_deref(), Some("usize"));
}

#[test]
fn hex_isize_suffix() {
    let pkg = lower("fn answer() -> ISize { 0x10_isize }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0x10);
    assert_eq!(s.as_deref(), Some("isize"));
}

#[test]
fn binary_literal_value() {
    let pkg = lower("fn answer() -> U8 { 0b1010_u8 }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0b1010);
    assert_eq!(s.as_deref(), Some("u8"));
}

#[test]
fn octal_literal_value() {
    let pkg = lower("fn answer() -> U32 { 0o777_u32 }");
    let (v, s) = first_int_literal(&pkg);
    assert_eq!(v, 0o777);
    assert_eq!(s.as_deref(), Some("u32"));
}

#[test]
fn hex_max_u64() {
    // 0xFFFF_FFFF_FFFF_FFFF should not panic on i128 parse — we go
    // through u128 first to preserve the unsigned value.
    let pkg = lower("fn answer() -> U64 { 0xFFFF_FFFF_FFFF_FFFF_u64 }");
    let (v, _) = first_int_literal(&pkg);
    assert_eq!(v as u64, u64::MAX);
}
