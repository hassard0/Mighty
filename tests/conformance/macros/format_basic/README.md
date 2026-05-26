# macros/format_basic

v0.24 Track B positive: `format!()` builtin macro expands a template
with a positional `{}` placeholder, the runtime materializes the
concatenated `String`, and `log(...)` prints it.

Exercises the full chain:
- mty-macros: `format!` builtin recognition + template parsing
- mty-hir: preprocessor splices the expanded `("" + "score: " + (score).to_str())`
- mty-types: `to_str` accepted on `I32` receiver via permissive method table
- mty-ir interp: `+` on strings concatenates; `to_str` on int renders
