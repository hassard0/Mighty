#[test]
fn package_default() {
    let p = mty_hir::Package::default();
    assert!(p.top_level.is_empty());
}
