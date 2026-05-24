//! Rust comparator for the parse_throughput benchmark.
//!
//! Lexes a 10 KLOC synthetic source identical in *shape* to the one
//! `sdust_bench::fixtures::stardust_10kloc()` generates. We use logos
//! 0.14 — the same lexer crate Mighty uses — so the comparison is
//! "Mighty's lexer vs an idiomatic hand-written Rust lexer with the
//! same backend." The expected outcome is a small Mighty slowdown
//! due to CST/SyntaxKind metadata; that's documented.
//!
//! Usage: `cargo run --release -- --iters 30`

use logos::Logos;
use std::time::Instant;

#[derive(Logos, Debug, PartialEq)]
#[logos(skip r"[ \t\r\n]+|//.*")]
enum Tok {
    #[token("fn")]
    Fn,
    #[token("struct")]
    Struct,
    #[token("let")]
    Let,
    #[token("->")]
    Arrow,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("=")]
    Eq,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Ident,
    #[regex(r"[0-9]+")]
    Number,
}

fn synth_source(units: usize) -> String {
    let mut out = String::with_capacity(units * 256);
    out.push_str("// rust comparator\n");
    for i in 0..units {
        out.push_str(&format!(
            "struct Rec{i} {{\n  id: I64\n  name: I64\n  flag: I64\n}}\n\
fn bench_f{i}(x: I64, y: I64) -> I64 {{\n  let z = x + y\n  let w = z * 2 - x\n  w\n}}\n"
        ));
    }
    out
}

fn percentiles(mut v: Vec<u128>) -> (u128, u128, u128) {
    v.sort();
    let pick = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    (pick(0.50), pick(0.95), pick(0.99))
}

fn main() {
    let iters: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let src = synth_source(1000);
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let s = src.clone();
        let t0 = Instant::now();
        let mut lex = Tok::lexer(&s);
        let mut count = 0u64;
        while let Some(_) = lex.next() {
            count += 1;
        }
        std::hint::black_box(count);
        samples.push(t0.elapsed().as_nanos());
    }
    let (p50, p95, p99) = percentiles(samples);
    println!(
        "rust_parse_throughput: median={:.3} ms  p95={:.3} ms  p99={:.3} ms  (bytes={})",
        (p50 as f64) / 1.0e6,
        (p95 as f64) / 1.0e6,
        (p99 as f64) / 1.0e6,
        src.len(),
    );
}
