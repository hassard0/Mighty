use sdust_driver::{lower, parse_source};
use sdust_syntax::SyntaxNode;
use std::fs;
use std::path::Path;

pub fn run(path: &Path, ast: bool, cst: bool, hir: bool) -> i32 {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path.display(), e);
            return 1;
        }
    };
    let parsed = parse_source(src.clone(), path.display().to_string());
    if cst {
        let root = SyntaxNode::new_root(parsed.green.clone());
        println!("{:#?}", root);
    }
    if ast {
        let root = SyntaxNode::new_root(parsed.green.clone());
        for item in root.children() {
            println!("- {:?} {:?}", item.kind(), item.text_range());
        }
    }
    if hir {
        let (pkg, _) = lower(&parsed);
        println!("{}", sdust_hir::dump::dump_package(&pkg));
    }
    if !ast && !cst && !hir {
        eprintln!("specify one of --ast --cst --hir");
        return 2;
    }
    0
}
