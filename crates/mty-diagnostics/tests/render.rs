use mty_diagnostics::{codes::UNEXPECTED_TOKEN, render::ariadne::render, Diagnostic, Label};

#[test]
fn renders_one_line() {
    let src = "fn main() {\n   bad@@\n}\n";
    let d = Diagnostic::error(
        UNEXPECTED_TOKEN,
        Label {
            start: 18,
            end: 20,
            message: "unexpected `@@`".into(),
        },
    );
    let out = render(&d, "test.mty", src);
    assert!(out.contains("MT0001"), "render output:\n{}", out);
    assert!(
        out.contains("unexpected `@@`") || out.contains("`@@`") || out.contains("@@"),
        "render output:\n{}",
        out
    );
}
