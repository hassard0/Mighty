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
        | "CostSummary" => "observe",
        "HtmlEscape" | "ShellEscape" | "SqlEscape" | "PathBoundary" | "sanitize_with"
        | "matches_regex" | "in_allowlist" => "taint",
        "FsCap" | "StatResult" => "fs",
        "Json" => "json",
        "String" | "format" => "string",
        "Vec" => "vec",
        "Index" | "Doc" | "ChunkStrategy" | "Chunker" | "Retriever" | "Reranker" | "Rag" => "rag",
        "ComputerCap" | "Dispatcher" | "Mouse" | "Keyboard" | "Screen" | "MockScreen"
        | "ComputerAction" | "SandboxViolation" => "computer",
        "Canvas" | "Input" | "Key" => "web",
        "log" | "panic" | "spawn" => "builtin",
        _ => "builtin",
    }
}
