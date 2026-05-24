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
        } => cmd::dump::run(&path, ast, cst, hir),
    };
    std::process::exit(code);
}
