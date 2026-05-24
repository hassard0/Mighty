//! `mty lsp` — run the Mighty Language Server over stdio.

pub fn run() -> i32 {
    mty_lsp::run_stdio()
}
