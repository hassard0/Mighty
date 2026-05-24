//! `sdust lsp` — run the Stardust Language Server over stdio.

pub fn run() -> i32 {
    sdust_lsp::run_stdio()
}
