use mty_syntax::{parse, SyntaxNode};
use std::fs;
use std::path::PathBuf;

#[test]
fn round_trip_preserves_item_shape() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    for dir in [root.join("examples"), root.join("tests/fmt/fixtures")] {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("mty") {
                paths.push(p);
            }
        }
    }
    assert!(!paths.is_empty(), "no .mty files found");
    for p in paths {
        let src = fs::read_to_string(&p).unwrap();
        let original_tree = parse(&src).green;
        let formatted = mty_fmt::format(original_tree.clone());
        let reparsed_tree = parse(&formatted).green;
        let kinds_orig: Vec<u16> = SyntaxNode::new_root(original_tree)
            .children()
            .map(|c| c.kind() as u16)
            .collect();
        let kinds_new: Vec<u16> = SyntaxNode::new_root(reparsed_tree)
            .children()
            .map(|c| c.kind() as u16)
            .collect();
        assert_eq!(
            kinds_orig,
            kinds_new,
            "item-shape mismatch for {}",
            p.display()
        );
    }
}
