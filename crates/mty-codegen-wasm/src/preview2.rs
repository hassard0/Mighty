//! WASI Preview 2 (0.2.x) host bindings + component wrapping.
//!
//! v0.13 introduces an opt-in P2 path for the Wasm Component Model
//! backend. The existing P1 pipeline (`emit_wit` + `wrap_as_component`)
//! is untouched and remains the default; callers ask for P2 via
//! [`Preview2Options`] (driven by the `--wasi=p2` CLI flag).
//!
//! ### What the P2 path produces
//!
//! Given the same core Wasm module the slice-8 emitter produces, this
//! module builds a Component Model component whose imports are the
//! versioned WASI P2 interface set:
//!
//! - `wasi:cli@0.2.3` (stdout/stderr/stdin/exit/environment)
//! - `wasi:io@0.2.3`  (streams + poll + error)
//! - `wasi:clocks@0.2.3` (monotonic + wall-clock)
//! - `wasi:filesystem@0.2.3` (preopens + descriptor types)
//! - `wasi:http@0.2.3` (outgoing-handler + request/response types)
//! - `wasi:random@0.2.3`
//!
//! The core module's existing `wasi:cli/log#log` import is preserved as
//! a Mighty-internal *adapter import* — the P2 world declares the same
//! interface alongside the P2 ones so the existing core module
//! continues to validate. A v0.14 follow-up will replace the adapter
//! with a real `wasi:cli/stdout#print` lowering.
//!
//! ### User-WIT integration
//!
//! When the caller supplies a user package world (loaded via
//! `mty_pkg::wit_resolve::UserWit`), it is merged into the generated
//! WIT document so the user's exports / additional imports show up in
//! the produced component. The user world's *name* takes precedence
//! over the synthesized `<pkg>-world` name.
//!
//! ### What's stubbed
//!
//! `wasi:filesystem@0.2.3` resource methods (descriptor `read-via-stream`
//! etc.) are declared but not wired through from `std.fs`. Calls to
//! `std.fs.read` still lower to the P1 import shape; the produced
//! component will trap at instantiation under a strict P2 host until
//! the v0.14 lowering work lands. The boundary is documented in
//! `docs/reference/wasi.md` and `WASI_P2_V0_13_NOTES.md`.

use crate::emit::compile_program_to_bytes;
use crate::error::{CompileResult, WasmError};
use crate::target::WasmTarget;
use crate::wit::WitDocument;
use mty_ir::ir::Program;

/// Vendored, in-tree slice of WASI Preview 2 (0.2.3). See
/// `crates/mty-codegen-wasm/wit/wasi-p2/wasi-p2.wit` for the source.
///
/// This is loaded into every P2 build's `wit_parser::Resolve` so user
/// worlds can `use` from the P2 namespaces without a vendored `wit/deps`
/// tree on disk.
pub const VENDORED_WASI_P2_WIT: &str = include_str!("../wit/wasi-p2/wasi-p2.wit");

/// The WASI P2 version Mighty v0.13 targets.
pub const WASI_P2_VERSION: &str = "0.2.3";

/// Per-build options for the WASI Preview 2 backend.
#[derive(Debug, Clone)]
pub struct Preview2Options {
    /// Package name (kebab-case stem). Used for the synthesized
    /// `<pkg>-world` when no user-world is supplied.
    pub pkg_name: String,
    /// Optional user-supplied WIT (parsed + resolved by
    /// `mty_pkg::wit_resolve`). When `Some`, the user's package is
    /// merged into the component's exported world.
    pub user_wit: Option<UserWit>,
}

/// A user-authored WIT package, pre-loaded by the caller (typically
/// `mty_pkg::wit_resolve::load_user_wit`).
///
/// We accept the *raw text* of the user's package so that
/// `preview2.rs` is the single source of truth for `wit_parser::Resolve`
/// composition. This keeps the user-WIT loader (`mty-pkg`) decoupled
/// from `wit-parser`'s API at the type level.
#[derive(Debug, Clone)]
pub struct UserWit {
    /// Concatenated text of every user `.wit` file (with package
    /// declarations preserved). Loaded by `mty_pkg::wit_resolve`.
    pub text: String,
    /// Optional explicit world name (`--world <name>`). When `None`
    /// we pick the *only* world in the user's package; if there are
    /// multiple, we surface a [`WasmError::Invalid`].
    pub world: Option<String>,
    /// Source label used in `wit_parser` diagnostics.
    pub source_label: String,
}

impl Preview2Options {
    pub fn new(pkg_name: impl Into<String>) -> Self {
        Self {
            pkg_name: pkg_name.into(),
            user_wit: None,
        }
    }

    pub fn with_user_wit(mut self, uw: UserWit) -> Self {
        self.user_wit = Some(uw);
        self
    }
}

/// Build the P2 WIT document. This is exposed for testing; the normal
/// build path goes through [`compile_program_to_bytes_p2`] /
/// [`compile_program_to_file_p2`].
pub fn emit_wit_p2(_prog: &Program, opts: &Preview2Options) -> CompileResult<WitDocument> {
    // `_prog` is reserved for v0.14 — once the lowering pass starts
    // synthesizing per-fn exports inside the P2 world (rather than
    // the hard-coded `main`), we'll consume the program's signature
    // here. Keeping the param in the public signature now avoids a
    // breaking change later.
    let pkg_id = sanitize_pkg_id(&opts.pkg_name);

    // Phase 1: synthesize the Mighty package world. We emit it as a
    // self-contained package text so `wit_parser::Resolve` can re-parse
    // it for round-trip validation (matching the v0.2 contract).
    let synth_world_name = format!("{}-world", pkg_id);
    let user_world_name = opts.user_wit.as_ref().and_then(|u| u.world.clone());

    let world_name = user_world_name
        .clone()
        .unwrap_or_else(|| synth_world_name.clone());

    let mut user_body = String::new();
    user_body.push_str("// AUTO-GENERATED by mty-codegen-wasm (WASI Preview 2 path).\n");
    user_body.push_str(&format!(
        "// Target: wasm32-wasi, WASI version: {}.\n",
        WASI_P2_VERSION
    ));
    user_body.push_str(&format!("package mighty:{};\n\n", pkg_id));

    user_body.push_str(&format!("world {} {{\n", synth_world_name));
    // P2 imports — every P2 build pulls the full standard surface.
    // Components that don't use a given interface pay only the import
    // declaration cost (the host doesn't have to satisfy unused imports
    // at instantiation under wasmtime-wasi).
    user_body.push_str("  import wasi:cli/environment@0.2.3;\n");
    user_body.push_str("  import wasi:cli/exit@0.2.3;\n");
    user_body.push_str("  import wasi:cli/stdin@0.2.3;\n");
    user_body.push_str("  import wasi:cli/stdout@0.2.3;\n");
    user_body.push_str("  import wasi:cli/stderr@0.2.3;\n");
    user_body.push_str("  import wasi:io/error@0.2.3;\n");
    user_body.push_str("  import wasi:io/poll@0.2.3;\n");
    user_body.push_str("  import wasi:io/streams@0.2.3;\n");
    user_body.push_str("  import wasi:clocks/monotonic-clock@0.2.3;\n");
    user_body.push_str("  import wasi:clocks/wall-clock@0.2.3;\n");
    user_body.push_str("  import wasi:random/random@0.2.3;\n");
    user_body.push_str("  import wasi:filesystem/preopens@0.2.3;\n");
    user_body.push_str("  import wasi:filesystem/types@0.2.3;\n");
    user_body.push_str("  import wasi:http/types@0.2.3;\n");
    user_body.push_str("  import wasi:http/outgoing-handler@0.2.3;\n");
    // v0.13 boundary: the slice-8 core module's `wasi:cli/log#log`
    // import is *not* an upstream P2 interface. We declare an
    // **unversioned** `wasi:cli/log` stub in the same document so
    // `wit-component::ComponentEncoder` matches the core module's
    // literal import name (which carries no `@0.2.3` annotation).
    // A v0.14 follow-up replaces this with a real
    // `wasi:cli/stdout#print` lowering pass.
    user_body.push_str("  import wasi:cli/log;\n");
    // Re-export the symbol from the user's program (matches v0.2
    // behavior so existing `mty build` flows still find a `main`).
    user_body.push_str("  export main: func();\n");
    user_body.push_str("}\n\n");

    // Unversioned `wasi:cli` stub — sole purpose is to declare the
    // `log` interface so wit-component's encoder matches the slice-8
    // core module's literal import name `wasi:cli/log#log`. The
    // versioned `wasi:cli@0.2.3` package (with stdin/stdout/stderr/
    // exit/environment) sits alongside in the vendored P2 slice; both
    // can coexist because they have distinct package versions.
    user_body.push_str("package wasi:cli {\n");
    user_body.push_str("  interface log {\n");
    user_body.push_str("    log: func(msg: string);\n");
    user_body.push_str("  }\n");
    user_body.push_str("}\n\n");

    // Vendored P2 packages (full text included so `wit_parser::Resolve`
    // is satisfied for round-trip parsing).
    user_body.push_str(VENDORED_WASI_P2_WIT);
    user_body.push('\n');

    // Phase 2: merge the user's WIT, if any. The user's text may use
    // top-level `package X:Y;` form (canonical when authored as a
    // standalone .wit file), but we can only have **one** top-level
    // package per resolve. Convert any user top-level `package X:Y;`
    // declarations into nested `package X:Y { ... }` form so they
    // sit alongside our synthesized packages.
    if let Some(uw) = &opts.user_wit {
        user_body.push_str("\n// ---- USER-SUPPLIED WIT BELOW ----\n");
        user_body.push_str(&wrap_user_wit_as_nested(&uw.text));
        user_body.push('\n');
    }

    // Round-trip validation.
    let mut resolve = wit_parser::Resolve::default();
    let _ = resolve
        .push_str("mighty-p2.wit", &user_body)
        .map_err(|e| WasmError::Invalid(format!("wit p2 round-trip: {e:#}")))?;

    // Surface a useful error early if the user-world name doesn't exist.
    // `select_world` only searches one package, so we iterate over
    // every package in the resolve and look for the world there.
    if let Some(name) = &user_world_name {
        let mut found = false;
        let pkg_ids: Vec<_> = resolve.packages.iter().map(|(id, _)| id).collect();
        for pkg_id in pkg_ids {
            if resolve.select_world(pkg_id, Some(name)).is_ok() {
                found = true;
                break;
            }
        }
        if !found {
            return Err(WasmError::Invalid(format!(
                "user world '{}' not found in any package",
                name
            )));
        }
    }

    Ok(WitDocument {
        text: user_body,
        package_id: format!("mighty:{}", pkg_id),
        world_name,
        target: WasmTarget::Wasi,
    })
}

/// Compile + wrap a program as a P2 Component Model component.
pub fn compile_program_to_bytes_p2(
    prog: &Program,
    opts: &Preview2Options,
) -> CompileResult<Vec<u8>> {
    let core = compile_program_to_bytes(prog, WasmTarget::Wasi)?;
    let doc = emit_wit_p2(prog, opts)?;
    wrap_p2(&core, &doc)
}

/// Compile a program to `out` as a P2 component. Returns the WIT
/// document used (for callers that want to display it).
pub fn compile_program_to_file_p2(
    prog: &Program,
    opts: &Preview2Options,
    out: &std::path::Path,
) -> CompileResult<(Vec<u8>, WitDocument)> {
    let core = compile_program_to_bytes(prog, WasmTarget::Wasi)?;
    let doc = emit_wit_p2(prog, opts)?;
    let bytes = wrap_p2(&core, &doc)?;
    std::fs::write(out, &bytes)
        .map_err(|e| WasmError::Io(format!("write {}: {}", out.display(), e)))?;
    Ok((bytes, doc))
}

/// Wrap a core module as a P2 component, doing the world lookup
/// across *every* package in the resolve (not just the synthesized
/// `mighty:<pkg>` one). This is the difference from
/// [`crate::component::wrap_as_component`], which assumes the world
/// lives in the document's primary package.
fn wrap_p2(core_module: &[u8], doc: &WitDocument) -> CompileResult<Vec<u8>> {
    let mut resolve = wit_parser::Resolve::default();
    let _ = resolve
        .push_str("mighty-p2.wit", &doc.text)
        .map_err(|e| WasmError::Invalid(format!("p2 wrap re-parse: {e:#}")))?;

    // Find which package owns the world named `doc.world_name`.
    let pkg_ids: Vec<_> = resolve.packages.iter().map(|(id, _)| id).collect();
    let mut world_id = None;
    for pkg_id in pkg_ids {
        if let Ok(w) = resolve.select_world(pkg_id, Some(&doc.world_name)) {
            world_id = Some(w);
            break;
        }
    }
    let world_id = world_id.ok_or_else(|| {
        WasmError::Invalid(format!(
            "p2 wrap: world '{}' not found in any package",
            doc.world_name
        ))
    })?;

    let mut module_bytes = core_module.to_vec();
    wit_component::embed_component_metadata(
        &mut module_bytes,
        &resolve,
        world_id,
        wit_component::StringEncoding::UTF8,
    )
    .map_err(|e| WasmError::Invalid(format!("p2 embed wit metadata: {e:#}")))?;

    let mut enc = wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module_bytes)
        .map_err(|e| WasmError::Invalid(format!("p2 component encoder module: {e:#}")))?;
    enc.encode()
        .map_err(|e| WasmError::Invalid(format!("p2 component encode: {e:#}")))
}

/// Rewrite a user `.wit` text so that top-level `package X:Y;`
/// declarations become nested `package X:Y { ... }` blocks, suitable
/// for concatenation into a parent resolve that already has its own
/// top-level package.
///
/// The transform is intentionally simple — line-based. Each
/// occurrence of a line whose trimmed form is `package X:Y;`
/// (or `package X:Y@version;`) becomes a `package X:Y {` opener;
/// the matching `}` is appended at end-of-file. We assume the user
/// supplies one top-level package per `.wit` file (matching the
/// upstream WIT convention). Multi-package files should already use
/// nested form and pass through unchanged.
///
/// This won't handle pathological inputs (e.g. `package X:Y;`
/// inside a comment), but the round-trip parse via `wit_parser`
/// surfaces any breakage with a useful diagnostic.
fn wrap_user_wit_as_nested(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut opened_packages: u32 = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("package ") {
            if let Some(pkg_decl) = rest.strip_suffix(';') {
                // Convert `package X:Y;` → `package X:Y {` and
                // remember to close it at EOF.
                out.push_str("package ");
                out.push_str(pkg_decl.trim());
                out.push_str(" {\n");
                opened_packages += 1;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    for _ in 0..opened_packages {
        out.push_str("}\n");
    }
    out
}

/// Sanitize a free-form package name into a kebab-case WIT id.
///
/// Mirrors `wit::sanitize_pkg_id` but is duplicated here to avoid
/// reaching into a private helper. Cheap enough that we don't bother
/// hoisting it.
fn sanitize_pkg_id(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if c == '-' || c == '_' {
            s.push('-');
        }
    }
    if s.is_empty() || !s.chars().next().unwrap().is_ascii_alphabetic() {
        s.insert(0, 'p');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program,
        Term,
    };

    fn empty_main() -> Program {
        let mut p = Program::default();
        p.fns.push(Function {
            id: IrFnId(0),
            name: "main".into(),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Unit,
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        });
        p
    }

    #[test]
    fn p2_wit_round_trips() {
        let opts = Preview2Options::new("hello");
        let doc = emit_wit_p2(&empty_main(), &opts).expect("emit p2 wit");
        assert!(doc.text.contains("wasi:io/streams@0.2.3"));
        assert!(doc.text.contains("wasi:cli/stdout@0.2.3"));
        assert!(doc.text.contains("wasi:http/outgoing-handler@0.2.3"));
        // Re-parseable via resolve.
        let _ = doc.resolve().expect("p2 doc resolves");
    }

    #[test]
    fn p2_component_wraps() {
        let opts = Preview2Options::new("hello");
        let bytes = compile_program_to_bytes_p2(&empty_main(), &opts).expect("compile p2");
        assert!(crate::component::is_component(&bytes));
    }
}
