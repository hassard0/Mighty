use clap::{Args, Parser, Subcommand};

// v0.33 T7: the binary used to own `mod cmd;` directly. We now route
// through the crate's library face (`src/lib.rs`) so integration tests
// can reach helpers like `mty_cli::cmd::find::parse_source_for_tests`
// without re-listing every cmd module here. The binary itself just
// re-imports the `cmd` tree under a local alias.
use mty_cli::cmd;

#[derive(Parser)]
#[command(name = "mty", version, about = "Mighty compiler CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Clap-side mirror of [`cmd::serve::ServeArgs`]. Kept in `main.rs`
/// so the public `Cmd::Serve(ServeArgs)` variant stays self-contained.
#[derive(Args, Debug, Clone)]
struct ServeArgs {
    /// Port to bind on `127.0.0.1`. Defaults to 8000.
    #[arg(long, default_value_t = 8000)]
    port: u16,
    /// File-watch `src/` and rebuild + reload on every change.
    #[arg(long)]
    watch: bool,
    /// Package root (default: current working directory).
    #[arg(long)]
    manifest_dir: Option<std::path::PathBuf>,
}

/// v0.34 T4 — `mty hooks <install|uninstall|status>`.
#[derive(Subcommand, Debug, Clone)]
enum HooksCmd {
    /// Install the project's pre-push hook into `.git/hooks/pre-push`.
    Install {
        /// Overwrite any pre-existing hook, even if it's not a Mighty
        /// hook. Without this flag, the install refuses to clobber
        /// hooks we didn't author.
        #[arg(long)]
        force: bool,
    },
    /// Remove the installed pre-push hook (only if it carries our
    /// sentinel).
    Uninstall,
    /// Report whether the hook is installed.
    Status,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scaffold a new Mighty package.
    ///
    /// v0.23 Track C: pass `--template <name>` to pick a scaffold
    /// (`blank` is the default; `web-game` produces a wasm32-web
    /// agent + canvas + dom-shim ready for `mty serve`).
    New {
        name: String,
        /// Template name. Defaults to `blank`. Built-in templates:
        /// `blank`, `web-game`.
        #[arg(long)]
        template: Option<String>,
    },
    /// Built-in dev server for a web-game package.
    ///
    /// Reads `mighty.toml`, builds with `--target wasm32-web`, and
    /// serves `web/` + the freshly-built `main.wasm` on
    /// `127.0.0.1:<port>` (default 8000). With `--watch`,
    /// file-watches `src/` and pushes a reload to the page over a
    /// websocket on every successful rebuild. See
    /// `docs/reference/cli/mty-serve.md`.
    Serve(ServeArgs),
    /// Format .mty files in place (or stdin).
    Fmt {
        #[arg(num_args = 0..)]
        paths: Vec<std::path::PathBuf>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        check: bool,
    },
    /// Parse + HIR-lower; emit diagnostics; exit nonzero on error.
    ///
    /// v0.33 T4 — structured agent-actionable diagnostics. With
    /// `--format json`, emits one NDJSON envelope per diagnostic
    /// (schema: `docs/internals/diagnostic-envelopes.md`). Add
    /// `--include-source` to embed a 3-line source snippet in every
    /// envelope. The default `pretty` output (ariadne-rendered) is
    /// unchanged from previous releases.
    Check {
        path: std::path::PathBuf,
        /// `pretty` (default) or `json`.
        #[arg(long, default_value = "pretty")]
        format: String,
        /// Only meaningful with `--format json`: embed a 3-line source
        /// snippet around each diagnostic's primary span.
        #[arg(long)]
        include_source: bool,
    },
    /// Dump intermediate representations.
    Dump {
        path: std::path::PathBuf,
        #[arg(long)]
        ast: bool,
        #[arg(long)]
        cst: bool,
        #[arg(long)]
        hir: bool,
        #[arg(long, alias = "sir")]
        ir: bool,
    },
    /// Run a Mighty source file. Default: slice-7 runtime (tokio
    /// executor + agents). With `--legacy-interp`, use the slice-6
    /// synchronous interpreter (useful for diagnostic comparison).
    ///
    /// v0.27 Track E (QoL #3): trailing positionals after `--` are
    /// forwarded to Mighty source as `std.env.args()`. Example:
    /// `mty run demo.mty -- alpha "beta gamma"` makes `args` resolve
    /// to `["alpha", "beta gamma"]` inside the program.
    Run {
        path: std::path::PathBuf,
        #[arg(long)]
        legacy_interp: bool,
        /// Positional argv forwarded to the Mighty program as
        /// `std.env.args()`. Everything after `--` lands here.
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// Build a Mighty source file to a runnable artifact (slice 8).
    ///
    /// Default target = `native` (host-architecture executable, via
    /// Cranelift + the platform linker). Use `--target wasm32-wasi`
    /// for a Wasm module runnable under `wasmtime`, or
    /// `--target wasm32-web` for a browser-targeted module.
    Build {
        path: std::path::PathBuf,
        #[arg(long)]
        debug: bool,
        #[arg(long)]
        release: bool,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        out_dir: Option<std::path::PathBuf>,
        /// Wasm targets only: emit a bare core wasm module instead
        /// of a Component Model component. Default = component
        /// output (v0.2 wave-2, closes A47).
        #[arg(long)]
        no_component: bool,
        /// Wasm targets only: which WASI preview to target.
        /// `p2` (default since v0.15) emits a component that
        /// imports the versioned `wasi:*@0.2.3` interfaces with
        /// direct lowerings for `std.random` + `std.time`;
        /// `p1` keeps the legacy v0.2..v0.14 import shape (the
        /// pre-v0.15 default) for back-compat. See
        /// `docs/reference/wasi.md`.
        #[arg(long)]
        wasi: Option<String>,
        /// Wasm targets only: pick the component world by name
        /// when the user's `[wit]` package defines more than one.
        /// Defaults to the world declared in `mighty.toml`'s
        /// `[wit] world = ...`, or the synthesized
        /// `<pkg>-world` if none is declared.
        #[arg(long)]
        world: Option<String>,
    },
    /// Print a human-readable explanation of a diagnostic code.
    Explain {
        /// e.g. MT0001, sd0001, 0001, 1
        code: String,
    },
    /// v0.35 T3 — Bulk-apply fix envelopes to a source file.
    ///
    /// Reads the file, runs `mty check` in-process, and for every
    /// diagnostic that carries a fix envelope: picks the highest-
    /// confidence alternative (≥ `--threshold`, default 0.85),
    /// applies it, and writes back to disk. Pipe `mty check
    /// --format json` into `mty fix --apply --from-stdin` to drive
    /// the loop without re-checking inside `mty fix`.
    ///
    /// See `docs/reference/cli/mty-fix.md` for the full flag matrix.
    Fix {
        /// Required when `--apply` is set: source file to fix.
        path: Option<std::path::PathBuf>,
        /// Bulk-apply fixes. Without this flag, `mty fix` is a no-op
        /// (reserved for future read-only commands).
        #[arg(long)]
        apply: bool,
        /// Apply only fixes whose code matches (e.g. `MT4099`).
        #[arg(long)]
        code: Option<String>,
        /// Always pick this 0-indexed alternative instead of the
        /// highest-confidence one.
        #[arg(long)]
        alternative: Option<usize>,
        /// Confidence floor. Default 0.85.
        #[arg(long, default_value_t = cmd::fix::DEFAULT_THRESHOLD)]
        threshold: f32,
        /// Print the diff to stdout; don't write back.
        #[arg(long)]
        dry_run: bool,
        /// Prompt y/N before each fix. Incompatible with `--from-stdin`.
        #[arg(long)]
        interactive: bool,
        /// Read NDJSON envelopes from stdin (pipe from `mty check --format json`).
        #[arg(long)]
        from_stdin: bool,
    },
    /// v0.33 T7 — Capability-tagged search across the Mighty stdlib.
    ///
    /// Examples:
    ///
    ///   mty find "write files"
    ///   mty find "send http" --format json
    ///   mty find --by-capability fs.write
    ///   mty find "vector store" --explain
    ///
    /// See `docs/reference/find.md` for the query DSL + ranking spec.
    Find {
        /// Free-form query, e.g. `"write files"` or `"vector store"`.
        query: Option<String>,
        /// List every item that requires this capability instead of
        /// running a query. Inverse of "I want to write files".
        #[arg(long)]
        by_capability: Option<String>,
        /// Output format: `pretty` (default), `json` (NDJSON), `short`.
        #[arg(long, default_value = "pretty")]
        format: String,
        /// Append capability + minimal usage example to each result.
        #[arg(long)]
        explain: bool,
        /// Number of top results to surface. Defaults to 5.
        #[arg(long, default_value_t = 5)]
        top: usize,
        /// Force a fresh index rebuild (ignores `~/.mty/find-index.json`).
        #[arg(long)]
        rebuild: bool,
    },
    /// Run the Mighty Language Server (LSP 3.17) over stdio.
    Lsp,
    /// v0.32 Track A — Run the Mighty Debug Adapter Protocol server
    /// over stdio. Speaks DAP per the Microsoft spec; the VS Code +
    /// JetBrains plugins shell out to `mty dap` as their adapter
    /// process. Accepts no flags; the client drives everything via
    /// `launch` / `setBreakpoints` / `continue` / etc. See
    /// `docs/reference/cli/mty-dap.md`.
    Dap,
    /// Package manager: add / remove / update / fetch / list / publish.
    Pkg {
        #[command(subcommand)]
        cmd: cmd::pkg::PkgCmd,
        /// Override the package root (default: current directory).
        #[arg(long, global = true)]
        manifest_dir: Option<std::path::PathBuf>,
    },
    /// Connect to a running Mighty runtime's control socket and
    /// print a snapshot of every live agent. Requires the runtime
    /// to have been started with `MTY_RUNTIME_CONTROL_SOCK=<path>`.
    /// See `docs/reference/cli/mty-inspect.md`.
    ///
    /// v0.30 Track D: pass `--cost` to switch to LLM-cost mode —
    /// reads `~/.mty/observations.sqlite` and prints total $$, per-
    /// {provider,model,agent} breakdown, p50/p95/p99 latency, and
    /// (optionally) top-N most expensive calls. See
    /// `docs/internals/observability.md`.
    Inspect {
        /// Socket path (overrides `MTY_RUNTIME_CONTROL_SOCK`).
        #[arg(long)]
        sock: Option<String>,
        /// Return a single agent's snapshot instead of the whole runtime.
        #[arg(long)]
        agent: Option<u64>,
        /// Emit raw JSON instead of the pretty-printed table.
        #[arg(long)]
        json: bool,
        /// Poll every N milliseconds until interrupted.
        #[arg(long, value_name = "MS")]
        watch: Option<u64>,
        /// v0.30 Track D: switch to LLM-cost mode.
        #[arg(long)]
        cost: bool,
        /// v0.30 Track D: window spec for `--cost`. `7d`, `12h`,
        /// `30m`, `45s`, `500ms`, or `all`. Default `24h`.
        #[arg(long, value_name = "DURATION")]
        since: Option<String>,
        /// v0.30 Track D: group key for `--cost`. One of
        /// `provider`, `model`, `agent`, `none`. Default `provider`.
        #[arg(long, value_name = "KEY")]
        by: Option<String>,
        /// v0.30 Track D: print the top-N most expensive single calls.
        #[arg(long, value_name = "N")]
        top: Option<usize>,
        /// v0.30 Track D: observations DB path (overrides
        /// `MTY_OBSERVE_DB`; default `~/.mty/observations.sqlite`).
        #[arg(long, value_name = "PATH")]
        db: Option<String>,
    },
    /// Load a recorded runtime trace (`mty-trace-*.bin`) and either
    /// summarize it, dump every event as JSON, or step-replay it.
    ///
    /// v0.17 Tier 1.4. Recording is opt-in: set
    /// `MTY_RECORD_TRACE=<path>` when running the program you want to
    /// capture. See `docs/reference/cli/mty-replay.md`.
    Replay {
        /// Path to the `.bin` trace file (produced via `MTY_RECORD_TRACE`).
        trace: std::path::PathBuf,
        /// Dump every event as one JSON line to stdout.
        #[arg(long)]
        dump_json: bool,
        /// Walk the trace through a step handler (in-process replay).
        #[arg(long)]
        step: bool,
        /// Emit the default summary as JSON instead of the table.
        #[arg(long)]
        json: bool,
        /// v0.19: drive a fresh `Runtime` from the trace + assert each
        /// emitted event matches the recorded one. Requires `--program`.
        #[arg(long)]
        byte_identical: bool,
        /// v0.19: IO/Clock/Random reads return recorded bytes instead
        /// of touching the live host. Defaults to true so replay is
        /// deterministic across machines.
        #[arg(long, default_value_t = true)]
        mock_io: bool,
        /// v0.19: path to the `.mty` source file the trace was
        /// recorded against. Required with `--byte-identical`.
        #[arg(long)]
        program: Option<std::path::PathBuf>,
        /// v0.29 Track F: render an LLM-turn diff (one per recorded
        /// `TraceEvent::LlmCall`). Pair with `--turn <id>` to address
        /// a single turn. Used by `std.eval`'s divergence reporter to
        /// surface the exact recorded turn behind a failing case.
        #[arg(long)]
        diff: bool,
        /// v0.29 Track F: focus the diff renderer on one recorded
        /// `LlmCall.turn_id`. Setting this without `--diff` implies
        /// `--diff`.
        #[arg(long, value_name = "ID")]
        turn: Option<u64>,
    },
    /// Hot-reload a running agent: drain its current handler, snapshot
    /// state via `Resumable`, swap the code, restore the state, and
    /// resume — preserving the mailbox end-to-end. v0.20 Tier 1.5.
    /// See `docs/reference/cli/mty-reload.md`.
    Reload {
        /// The agent type (registry name) to reload.
        agent_type: String,
        /// Path to the replacement wasm module.
        #[arg(long)]
        from: std::path::PathBuf,
        /// How long to wait for the agent's current handler to drain
        /// before failing with `MT5062` (default: 5000 ms).
        #[arg(long, value_name = "MS")]
        deadline_ms: Option<u64>,
        /// Control-socket path (overrides `MTY_RUNTIME_CONTROL_SOCK`).
        #[arg(long)]
        sock: Option<String>,
        /// Emit the raw `ReloadReport` JSON instead of the pretty-printed table.
        #[arg(long)]
        json: bool,
        /// Validate inputs without contacting the runtime. Useful in CI.
        #[arg(long)]
        dry_run: bool,
    },
    /// v0.33 T5 — `mty agent`: structured JSON-over-stdio protocol
    /// that lets LLM agents drive every other `mty` subcommand
    /// without scraping human-rendered output. See
    /// `docs/internals/agent-mode-protocol.md` for the wire format
    /// and `docs/reference/cli/mty-agent.md` for human-facing CLI
    /// knobs.
    Agent {
        /// Read exactly one JSON request from stdin, run it, exit.
        #[arg(long)]
        single_shot: bool,
        /// Transport: `stdio` (default), `http`, `unix`.
        #[arg(long, default_value = "stdio")]
        transport: String,
        /// HTTP transport: bind port. Defaults to 8889.
        #[arg(long, default_value_t = 8889)]
        port: u16,
        /// Unix transport: socket path.
        #[arg(long)]
        socket: Option<std::path::PathBuf>,
        /// v0.35 T2 — `host:port` to bind for HTTP, or a path for unix.
        /// Overrides `--port` / `--socket` when set. Accepted shapes:
        /// `0.0.0.0:9090`, `127.0.0.1:9090`, `[::]:9090`, or a unix
        /// socket path.
        #[arg(long)]
        listen: Option<String>,
        /// v0.35 T2 — bearer-token required on the `Authorization:
        /// Bearer <token>` header for every HTTP request. Off by
        /// default. Ignored under stdio / unix transports.
        #[arg(long)]
        auth_token: Option<String>,
        /// v0.35 T2 — append every `(request, response)` pair to this
        /// NDJSON file. Useful for regression tests, debugging client
        /// behaviour, and training/eval traces. Works under any
        /// transport.
        #[arg(long, value_name = "PATH")]
        record: Option<std::path::PathBuf>,
        /// v0.35 T2 — replay a previously recorded session from this
        /// NDJSON file. The session reads the recorded requests, runs
        /// them against the live session, and asserts each response
        /// stream byte-matches the recorded one. Exit 0 on match, 1 on
        /// drift.
        #[arg(long, value_name = "PATH")]
        replay: Option<std::path::PathBuf>,
    },
    /// Mighty test runner. Discovers `tests/*.test.mty` (legacy bare
    /// `tests/*.mty` is still accepted) and dispatches each through
    /// the slice-6 SIR interpreter, the same shape the standalone
    /// `mty-test` binary has served since v0.2.
    ///
    /// v0.30 Track E: pass `--eval` to discover `**/*.eval.mty`
    /// instead — frontmatter-driven LLM-eval suites that pin a panel
    /// of providers and assert per-cell scores against a configurable
    /// threshold. See `docs/internals/eval.md`.
    Test {
        /// Override the discovery root. Defaults to the cwd.
        #[arg(long)]
        manifest_dir: Option<std::path::PathBuf>,
        /// Run eval suites (`**/*.eval.mty`) instead of unit tests.
        #[arg(long)]
        eval: bool,
        /// Eval mode: fail the run if any cell errored (default).
        #[arg(long)]
        strict: bool,
        /// Eval mode: opposite of `--strict` — error cells are logged
        /// but don't fail the run. Useful for offline / no-API-key
        /// dev so a missing `ANTHROPIC_API_KEY` doesn't break the
        /// inner loop.
        #[arg(long, conflicts_with = "strict")]
        no_strict: bool,
        /// Eval mode: skip the live-dispatch path; run only against
        /// previously recorded traces (deterministic-replay
        /// equivalence check — free + fast for CI).
        #[arg(long)]
        replay_only: bool,
        /// Eval mode: read provider-set + threshold from `[eval.ci]`
        /// in `mighty.toml` instead of the per-file frontmatter.
        #[arg(long)]
        ci: bool,
        /// Output format: `pretty` (default) or `json`. JSON emits one
        /// object per suite + a summary object on its own line so CI
        /// dashboards can read incrementally.
        #[arg(long, default_value = "pretty")]
        format: String,
    },
    /// v0.34 T4 — install / uninstall / status of the project's
    /// pre-push git hook (mirrors the cheapest two CI gates,
    /// `cargo fmt --check` + `cargo clippy -D warnings`). See
    /// `docs/contributing.md` for the swarm-agent setup boilerplate.
    Hooks {
        #[command(subcommand)]
        cmd: HooksCmd,
    },
    /// Render package documentation extracted from `///` doc comments.
    ///
    /// With no flags, prints a Go-style summary of the package's public
    /// items to stdout. With `ITEM`, prints the full doc body of one
    /// item. With `--html` or `--markdown`, renders a navigable site
    /// to `target/doc/<package>` (override with `--out`).
    ///
    /// v0.35 T5: `--check` (no path) runs the Strategy B drift gate —
    /// compares the extracted stdlib catalog (per-module docstubs at
    /// `crates/mty-stdlib/docs/*.docstub`) to the curated gold-set
    /// (`crates/mty-doc/src/examples.rs::STDLIB_EXAMPLES`) and exits
    /// non-zero on any divergence. This is the CI gate against
    /// hover-catalog rot. See `docs/internals/stdlib-docs-pipeline.md`.
    Doc {
        /// Path to the `.mty` file to document. Optional when `--check`
        /// is set (the drift gate has no input path).
        path: Option<std::path::PathBuf>,
        /// Print one item's full doc instead of the package summary.
        item: Option<String>,
        /// Render an HTML site (per-module pages + search index).
        #[arg(long)]
        html: bool,
        /// Render a markdown tree (one file per item, plus an index).
        #[arg(long)]
        markdown: bool,
        /// Output directory for --html / --markdown.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Type-check extracted `mty` / `mighty` / `sd` / `stardust` code blocks.
        /// (No-op in v0.2; see DOC_V0_2_NOTES.md.)
        #[arg(long)]
        check_examples: bool,
        /// v0.35 T5: run the stdlib-hover-catalog drift gate. Compares
        /// the extracted docstub catalog to the curated table and
        /// exits non-zero on any divergence. No path required.
        #[arg(long)]
        check: bool,
        /// v0.41 T5: with `--check`, also audit every catalog entry
        /// against the real stdlib surface (prelude registrations +
        /// interp dispatch tables + host dispatcher + `mty-stdlib`
        /// source) and exit non-zero if any entry resolves to nothing.
        /// Catches catalog drift in the opposite direction from
        /// `--check`: this one fires when docs describe a symbol the
        /// runtime has not actually shipped.
        #[arg(long)]
        check_surface: bool,
        /// With `--check`, write the drift report to this file in
        /// addition to stdout. Useful for CI artefact uploads.
        #[arg(long)]
        report: Option<std::path::PathBuf>,
    },
}

/// CLI-level `std.*` dispatcher (see V0_2_CLEANUP_NOTES.md, Task 1).
///
/// Wraps `mty_stdlib::host::dispatch` so paths that lose the `std.`
/// prefix (e.g. the IR lowerer emits `["json"]` after a `use std.json`
/// rewrite, instead of `["std", "json"]`) still route to the real
/// implementation. We try the path verbatim first, then re-prepend
/// `"std"` and try again.
fn cli_std_dispatch(
    path: &[String],
    method: &str,
    args: &[mty_ir::interp::value::Value],
) -> mty_ir::interp::value::Value {
    let v = mty_stdlib::host::dispatch(path, method, args);
    // If the first attempt didn't match a real impl (Value::Unit
    // is the fallback for unmatched (module, method) pairs), retry
    // with `std.` prepended.
    if matches!(v, mty_ir::interp::value::Value::Unit)
        && path.first().map(String::as_str) != Some("std")
    {
        let mut prefixed = Vec::with_capacity(path.len() + 1);
        prefixed.push("std".to_string());
        prefixed.extend_from_slice(path);
        mty_stdlib::host::dispatch(&prefixed, method, args)
    } else {
        v
    }
}

fn main() {
    // v0.3 Task 1 (see V0_2_CLEANUP_NOTES.md): plug a stdlib-bridging
    // dispatcher into the runtime before parsing any CLI args so every
    // command path (Run / Build / Check / …) sees real `std.*`
    // semantics. Idempotent: safe to call once per process.
    mty_runtime::host_std::install_dispatcher(cli_std_dispatch);

    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::New { name, template } => cmd::new::run(&name, template.as_deref()),
        Cmd::Serve(args) => cmd::serve::run(cmd::serve::ServeArgs {
            port: args.port,
            watch: args.watch,
            manifest_dir: args.manifest_dir,
        }),
        Cmd::Fmt {
            paths,
            stdin,
            check,
        } => cmd::fmt::run(paths, stdin, check),
        Cmd::Check {
            path,
            format,
            include_source,
        } => {
            let fmt = cmd::check::CheckFormat::parse(&format);
            cmd::check::run_with(&path, fmt, include_source)
        }
        Cmd::Dump {
            path,
            ast,
            cst,
            hir,
            ir,
        } => cmd::dump::run(&path, ast, cst, hir, ir),
        Cmd::Run {
            path,
            legacy_interp,
            argv,
        } => cmd::run::run(&path, legacy_interp, argv),
        Cmd::Build {
            path,
            debug,
            release,
            target,
            out_dir,
            no_component,
            wasi,
            world,
        } => cmd::build::run(
            &path,
            debug,
            release,
            target,
            out_dir,
            no_component,
            wasi,
            world,
        ),
        Cmd::Inspect {
            sock,
            agent,
            json,
            watch,
            cost,
            since,
            by,
            top,
            db,
        } => cmd::inspect::run(cmd::inspect::InspectArgs {
            sock,
            agent,
            json,
            watch_ms: watch,
            cost,
            since,
            by,
            top,
            db,
        }),
        Cmd::Explain { code } => cmd::explain::run(&code),
        Cmd::Fix {
            path,
            apply,
            code,
            alternative,
            threshold,
            dry_run,
            interactive,
            from_stdin,
        } => {
            if !apply {
                eprintln!("mty fix: pass --apply to bulk-apply fix envelopes (this command is reserved for read-only operations otherwise; see `mty fix --help`)");
                std::process::exit(2);
            }
            cmd::fix::run(cmd::fix::FixApplyArgs {
                path,
                code,
                alternative,
                threshold,
                dry_run,
                interactive,
                from_stdin,
            })
        }
        Cmd::Find {
            query,
            by_capability,
            format,
            explain,
            top,
            rebuild,
        } => cmd::find::run(cmd::find::FindArgs {
            query,
            by_capability,
            format,
            explain,
            top,
            rebuild,
            stdlib_root: None,
            index_path: None,
        }),
        Cmd::Lsp => cmd::lsp::run(),
        Cmd::Dap => cmd::dap::run(),
        Cmd::Pkg {
            cmd: pkg_cmd,
            manifest_dir,
        } => cmd::pkg::run(pkg_cmd, manifest_dir),
        Cmd::Doc {
            path,
            item,
            html,
            markdown,
            out,
            check_examples,
            check,
            check_surface,
            report,
        } => {
            if check {
                cmd::doc::run_check(report.as_deref(), check_surface)
            } else {
                match path {
                    Some(p) => cmd::doc::run(&p, item, html, markdown, out, check_examples),
                    None => {
                        eprintln!(
                            "mty doc: a PATH is required unless --check is set; run `mty doc --help`"
                        );
                        2
                    }
                }
            }
        }
        Cmd::Replay {
            trace,
            dump_json,
            step,
            json,
            byte_identical,
            mock_io,
            program,
            diff,
            turn,
        } => cmd::replay::run(cmd::replay::ReplayArgs {
            trace,
            dump_json,
            step,
            json_summary: json,
            byte_identical,
            mock_io,
            program,
            diff,
            turn,
        }),
        Cmd::Test {
            manifest_dir,
            eval,
            strict,
            no_strict,
            replay_only,
            ci,
            format,
        } => {
            let fmt =
                cmd::test::OutputFormat::parse(&format).unwrap_or(cmd::test::OutputFormat::Pretty);
            // `--strict` is the default; only flip when --no-strict
            // was passed. (Both flags conflict in clap above so we
            // never see both set at once.) `strict` is accepted as a
            // no-op flag for explicit/scripted invocations.
            let _ = strict;
            let strict_flag = !no_strict;
            cmd::test::run(cmd::test::TestArgs {
                manifest_dir,
                eval,
                strict: strict_flag,
                replay_only,
                ci,
                format: fmt,
            })
        }
        Cmd::Agent {
            single_shot,
            transport,
            port,
            socket,
            listen,
            auth_token,
            record,
            replay,
        } => {
            let Some(transport_parsed) = cmd::agent::Transport::parse(&transport) else {
                eprintln!(
                    "mty agent: unknown --transport `{}` (expected stdio, http, unix)",
                    transport
                );
                std::process::exit(2);
            };
            cmd::agent::run(cmd::agent::AgentArgs {
                single_shot,
                transport: transport_parsed,
                http_port: port,
                unix_socket: socket,
                listen,
                auth_token,
                record,
                replay,
            })
        }
        Cmd::Reload {
            agent_type,
            from,
            deadline_ms,
            sock,
            json,
            dry_run,
        } => cmd::reload::run(cmd::reload::ReloadArgs {
            agent_type,
            from,
            deadline_ms,
            sock,
            json,
            dry_run,
        }),
        Cmd::Hooks { cmd: hooks_cmd } => {
            let action = match hooks_cmd {
                HooksCmd::Install { force } => cmd::hooks::HooksAction::Install { force },
                HooksCmd::Uninstall => cmd::hooks::HooksAction::Uninstall,
                HooksCmd::Status => cmd::hooks::HooksAction::Status,
            };
            cmd::hooks::run(action)
        }
    };
    std::process::exit(code);
}
