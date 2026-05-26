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
    Check { path: std::path::PathBuf },
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
    Run {
        path: std::path::PathBuf,
        #[arg(long)]
        legacy_interp: bool,
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
    /// Run the Mighty Language Server (LSP 3.17) over stdio.
    Lsp,
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
    },
    /// Render package documentation extracted from `///` doc comments.
    ///
    /// With no flags, prints a Go-style summary of the package's public
    /// items to stdout. With `ITEM`, prints the full doc body of one
    /// item. With `--html` or `--markdown`, renders a navigable site
    /// to `target/doc/<package>` (override with `--out`).
    Doc {
        path: std::path::PathBuf,
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
        Cmd::New { name } => cmd::new::run(&name),
        Cmd::Fmt {
            paths,
            stdin,
            check,
        } => cmd::fmt::run(paths, stdin, check),
        Cmd::Check { path } => cmd::check::run(&path),
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
        } => cmd::run::run(&path, legacy_interp),
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
        } => cmd::inspect::run(cmd::inspect::InspectArgs {
            sock,
            agent,
            json,
            watch_ms: watch,
        }),
        Cmd::Explain { code } => cmd::explain::run(&code),
        Cmd::Lsp => cmd::lsp::run(),
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
        } => cmd::doc::run(&path, item, html, markdown, out, check_examples),
        Cmd::Replay {
            trace,
            dump_json,
            step,
            json,
        } => cmd::replay::run(cmd::replay::ReplayArgs {
            trace,
            dump_json,
            step,
            json_summary: json,
        }),
    };
    std::process::exit(code);
}
