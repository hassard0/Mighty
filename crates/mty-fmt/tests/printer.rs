use mty_fmt::doc::Doc;
use mty_fmt::printer::{pretty, Layout};

#[test]
fn renders_text() {
    assert_eq!(pretty(&Doc::text("hello"), &Layout::default()), "hello");
}

#[test]
fn group_fits_on_one_line() {
    let d = Doc::group(Doc::concat_all([
        Doc::text("(a"),
        Doc::line(),
        Doc::text("b)"),
    ]));
    assert_eq!(pretty(&d, &Layout::default()), "(a b)");
}

#[test]
fn group_breaks_when_too_wide() {
    let d = Doc::group(Doc::concat_all([
        Doc::text("(aaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        Doc::line(),
        Doc::text("bbbbbbbbbbbbbbbbbbbbbbbbbbbb)"),
    ]));
    let out = pretty(&d, &Layout { width: 20 });
    assert!(out.contains('\n'), "expected line break, got: {}", out);
}
