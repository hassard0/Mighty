use mty_driver::{lower, parse_source};
use mty_syntax::SyntaxNode;
use std::fs;
use std::path::Path;

pub fn run(path: &Path, ast: bool, cst: bool, hir: bool, ir: bool) -> i32 {
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
        println!("{}", mty_hir::dump::dump_package(&pkg));
    }
    if ir {
        let (pkg, _) = lower(&parsed);
        let typed = mty_types::check_package_typed(&pkg);
        let prog = mty_ir::lower_package(&pkg, &typed);
        println!("{}", mty_ir::dump_program(&prog));
    }
    if !ast && !cst && !hir && !ir {
        eprintln!("specify one of --ast --cst --hir --ir");
        return 2;
    }
    0
}
