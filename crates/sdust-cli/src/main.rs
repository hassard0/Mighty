use clap::{Parser, Subcommand};

mod cmd;

#[derive(Parser)]
#[command(name = "sdust", version, about = "Stardust compiler CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scaffold a new Stardust package.
    New { name: String },
    /// Format .sd files in place (or stdin).
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
        #[arg(long)]
        sir: bool,
    },
    /// Run a Stardust source file. Default: slice-7 runtime (tokio
    /// executor + agents). With `--legacy-interp`, use the slice-6
    /// synchronous interpreter (useful for diagnostic comparison).
    Run {
        path: std::path::PathBuf,
        #[arg(long)]
        legacy_interp: bool,
    },
    /// Print a human-readable explanation of a diagnostic code.
    Explain {
        /// e.g. SD0001, sd0001, 0001, 1
        code: String,
    },
}

fn main() {
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
            sir,
        } => cmd::dump::run(&path, ast, cst, hir, sir),
        Cmd::Run {
            path,
            legacy_interp,
        } => cmd::run::run(&path, legacy_interp),
        Cmd::Explain { code } => cmd::explain::run(&code),
    };
    std::process::exit(code);
}
