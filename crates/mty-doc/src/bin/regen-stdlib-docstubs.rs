//! One-shot generator for the per-module `_doc.mty` stub catalog
//! consumed by the v0.35 T5 "Strategy B" hover pipeline.
//!
//! Reads [`mty_doc::STDLIB_EXAMPLES`] (the v0.33 / v0.34 hand-curated
//! gold-set) and emits one `crates/mty-stdlib/docs/<module>.docstub`
//! file per stdlib surface. The file format is the docstub mini-grammar
//! parsed by [`mty_doc::stdlib_walker`] — see
//! `docs/internals/stdlib-docs-pipeline.md` for the full spec.
//!
//! Invoked as:
//!
//! ```bash
//! cargo run -p mty-doc --bin regen-stdlib-docstubs
//! ```
//!
//! The output files are committed verbatim and are the runtime
//! source-of-truth thereafter. `mty doc check` enforces zero drift
//! between the curated table and the extracted catalog.

use mty_doc::{StdlibExample, STDLIB_EXAMPLES};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    // Bucket by stdlib module. The module is decided by [`module_of`]
    // — a stable function the walker also uses, so a curated/extracted
    // round-trip stays consistent.
    let mut buckets: BTreeMap<&'static str, Vec<&'static StdlibExample>> = BTreeMap::new();
    for e in STDLIB_EXAMPLES {
        let m = module_of(e.symbol);
        buckets.entry(m).or_default().push(e);
    }

    // The output dir lives inside `mty-stdlib` so the docstubs ship with
    // the stdlib crate (philosophically — they describe stdlib symbols).
    // The walker is in `mty-doc` and pulls them in via `include_str!`.
    let out_dir = PathBuf::from("crates/mty-stdlib/docs");
    std::fs::create_dir_all(&out_dir)?;

    let mut total = 0usize;
    for (module, entries) in &buckets {
        let path = out_dir.join(format!("{module}.docstub"));
        let mut body = String::new();
        body.push_str("##module ");
        body.push_str(module);
        body.push('\n');
        body.push_str("##generated_from STDLIB_EXAMPLES (run `cargo run -p mty-doc --bin regen-stdlib-docstubs` to re-emit)\n");
        body.push('\n');
        for e in entries {
            body.push_str(&render_entry(e));
            body.push('\n');
            total += 1;
        }
        std::fs::write(&path, body)?;
        println!("wrote {} ({} entries)", path.display(), entries.len());
    }
    println!("total: {total} entries across {} modules", buckets.len());
    Ok(())
}

/// Render one entry in the docstub mini-grammar.
fn render_entry(e: &StdlibExample) -> String {
    let mut s = String::new();
    s.push_str("##sym ");
    s.push_str(e.symbol);
    s.push('\n');
    s.push_str("##sig ");
    s.push_str(e.signature);
    s.push('\n');
    if !e.capability.is_empty() {
        s.push_str("##cap ");
        s.push_str(e.capability);
        s.push('\n');
    }
    s.push_str("##desc ");
    s.push_str(e.description);
    s.push('\n');
    s.push_str("##see ");
    s.push_str(e.see_also);
    s.push('\n');
    s.push_str("##example\n");
    // Preserve trailing newline semantics by trimming exactly one
    // trailing `\n` if present; the parser re-appends it for parity
    // with the curated table.
    let ex = e.example.strip_suffix('\n').unwrap_or(e.example);
    s.push_str(ex);
    s.push('\n');
    s.push_str("##end\n");
    s
}

/// Classify a symbol into the docstub module bucket. Kept in sync with
/// [`mty_doc::stdlib_walker::module_of`] — the two are the same logic.
fn module_of(symbol: &str) -> &'static str {
    if let Some(rest) = symbol.strip_prefix("std.") {
        let head = rest.split('.').next().unwrap_or("");
        return match head {
            "http" => "http",
            "fs" => "fs",
            "time" => "time",
            "json" => "json",
            "env" => "env",
            "observe" => "observe",
            "process" => "process",
            "io" => "io",
            "path" => "path",
            _ => "builtin",
        };
    }
    let prefix = symbol.split('.').next().unwrap_or(symbol);
    match prefix {
        "Member" | "MemberReply" => "llm",
        "swarm" | "ConsensusStrategy" | "DollarBudget" | "Consensus" | "SharedDollarBudget"
        | "SimilarityMode" => "swarm",
        "McpServer" | "McpClient" | "ToolRegistry" => "mcp",
        "VectorStore" | "EpisodicMemory" | "WorkingMemory" => "memory",
        "Suite" | "Compare" | "Verdict" | "Case" => "eval",
        "observe" | "Window" | "GroupBy" | "summarize" | "percentiles" | "aggregate_by"
        | "CostSummary" | "top_by_cost" => "observe",
        "HtmlEscape" | "ShellEscape" | "SqlEscape" | "PathBoundary" | "sanitize_with"
        | "matches_regex" | "in_allowlist" | "sanitize_compose" | "named_regex" | "Allowlist" => {
            "taint"
        }
        "FsCap" | "StatResult" => "fs",
        "Json" => "json",
        "String" | "format" => "string",
        "Vec" => "vec",
        "Index" | "Doc" | "ChunkStrategy" | "Chunker" | "Retriever" | "Reranker" | "Rag" => "rag",
        "ComputerCap" | "Dispatcher" | "Mouse" | "Keyboard" | "Screen" | "MockScreen"
        | "ComputerAction" | "SandboxViolation" => "computer",
        "Canvas" | "Input" | "Key" => "web",
        "log" | "panic" | "spawn" | "eprintln" => "builtin",
        // v0.38 T4: extern c / FFI surfaces (v0.37 T3 + T6)
        "extern_block" | "extern_c_fn" | "extern_c_variadic" | "extern_lib"
        | "coerce_str_to_u8" | "addr_of_local" | "addr_of_mut" | "returned_struct" => "extern",
        // v0.38 T4: cast expressions (v0.37 T2 — MT2027 INVALID_CAST)
        "cast_as" | "cast_u8_to_i64" | "cast_i64_to_u8" | "cast_f32_to_f64"
        | "cast_f64_to_f32" | "cast_i32_to_f32" | "cast_f32_to_i32" | "cast_usize_to_u64"
        | "cast_bool_to_u8" | "cast_char_to_u32" | "cast_ptr_to_usize" | "cast_invalid_mt2027" => {
            "cast"
        }
        // v0.38 T4: runtime / build env vars
        "MTY_LINKER" | "MTY_OTLP_ENDPOINT" | "MTY_TRACE" | "MTY_RUNTIME_THREADS"
        | "MTY_RUNTIME_CONTROL_SOCK" => "env",
        // v0.38 T4: std.process builder + helpers (rooted at non-`std.` prefix)
        "Command" => "process",
        "ProcessExit" => "process",
        // v0.38 T4: std.io readers / writers (rooted at non-`std.` prefix)
        "BufReader" | "BufWriter" | "read_line" | "write_line" => "io",
        // v0.38 T4: std.path
        "Path" | "PathBuf" => "path",
        // v0.38 T4: std.collections
        "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet" => "collections",
        // v0.38 T4: std.iter combinators
        "Iterator" => "iter",
        // v0.38 T4: std.result / std.option
        "Result" => "result",
        "Option" => "option",
        // v0.38 T4: std.error trait + anyhow macro
        "Error" | "anyhow_error" => "error",
        _ => "builtin",
    }
}
