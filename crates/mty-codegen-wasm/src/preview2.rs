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
//! `std.fs.read` still lower to the P1 import shape; under v0.14 the
//! component now embeds the official `wasi_snapshot_preview1` adapter
//! (vendored from wasmtime v32.0.0 — see `crates/mty-codegen-wasm/adapter/`)
//! so the P1-shaped calls are translated to versioned P2 imports at
//! instantiation time. The boundary is documented in
//! `docs/reference/wasi.md` and `WASI_P2_LOWERINGS_V0_14_NOTES.md`.
//!
//! ### v0.14 direct-import lowering helpers
//!
//! For `std.random.*` and `std.time.*` we additionally expose direct
//! P2 import-emission helpers ([`emit_p2_random_bytes_import`],
//! [`emit_p2_monotonic_clock_now_import`], …) that callers can splice
//! into a core module under construction. These helpers mint the
//! versioned import shape `wasi:random/random@0.2.3#get-random-bytes`
//! (et al.) so the core module imports the canonical P2 interface
//! directly, bypassing the adapter for those cases where the
//! `wasi-libc`-generated calls would otherwise route through it.
//! `std.fs` and `std.http` continue to use the adapter — their
//! resource-typed surfaces require richer canonical-ABI plumbing
//! that's tracked for v0.15.

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

/// Upstream `wasmtime` release tag the vendored P1→P2 adapter modules
/// were built from. Surfaced for tests + docs; bump in lockstep with
/// the bytes in `crates/mty-codegen-wasm/adapter/`. v32.0.0 is the
/// first wasmtime release whose adapter targets WASI 0.2.3, matching
/// Mighty's [`WASI_P2_VERSION`].
pub const WASI_P1_ADAPTER_VERSION: &str = "wasmtime-v32.0.0";

/// Official `wasi_snapshot_preview1` → P2 adapter for **command**
/// components (programs with a `main` export — what `mty build`
/// produces by default). Vendored from wasmtime
/// [`WASI_P1_ADAPTER_VERSION`].
///
/// Passed to [`wit_component::ComponentEncoder::adapter`] when a P2
/// build wraps a core module so the core module's P1-shaped imports
/// (`wasi_snapshot_preview1#fd_write`, `clock_time_get`, …) are
/// translated into versioned P2 interface calls at instantiation.
pub const WASI_P1_ADAPTER_COMMAND: &[u8] =
    include_bytes!("../adapter/wasi_snapshot_preview1.command.wasm");

/// Official `wasi_snapshot_preview1` → P2 adapter for **reactor**
/// components (libraries that don't ship a `main`). Vendored from
/// wasmtime [`WASI_P1_ADAPTER_VERSION`]. Not yet wired into Mighty's
/// build path (`mty build` only emits command components) but
/// vendored alongside the command adapter so future slices can opt in
/// without a second vendoring pass.
pub const WASI_P1_ADAPTER_REACTOR: &[u8] =
    include_bytes!("../adapter/wasi_snapshot_preview1.reactor.wasm");

/// Official `wasi_snapshot_preview1` → P2 adapter for **proxy**
/// (wasi-http) components. Vendored from wasmtime
/// [`WASI_P1_ADAPTER_VERSION`]. Not yet wired into Mighty's build
/// path; see [`WASI_P1_ADAPTER_REACTOR`] for the same rationale.
pub const WASI_P1_ADAPTER_PROXY: &[u8] =
    include_bytes!("../adapter/wasi_snapshot_preview1.proxy.wasm");

/// Which P1→P2 adapter shape to embed when wrapping a core module
/// into a P2 component. Driven by the component "kind" the build
/// produces (command/reactor/proxy). `mty build` only emits
/// commands today so [`AdapterKind::Command`] is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    /// `wasi_snapshot_preview1.command.wasm` — programs with `main`.
    Command,
    /// `wasi_snapshot_preview1.reactor.wasm` — exported-function
    /// libraries (no `main`).
    Reactor,
    /// `wasi_snapshot_preview1.proxy.wasm` — wasi-http proxy shape.
    Proxy,
}

impl AdapterKind {
    /// Borrow the vendored adapter bytes for this kind.
    pub fn bytes(self) -> &'static [u8] {
        match self {
            AdapterKind::Command => WASI_P1_ADAPTER_COMMAND,
            AdapterKind::Reactor => WASI_P1_ADAPTER_REACTOR,
            AdapterKind::Proxy => WASI_P1_ADAPTER_PROXY,
        }
    }

    /// Human-readable name (matches the upstream file stem). Used by
    /// the `wit-component` adapter API — the same string the core
    /// module's imports name (`wasi_snapshot_preview1`).
    pub fn import_module_name(self) -> &'static str {
        // All three adapter shapes export the same legacy module
        // name; the difference is in what they import and how they
        // instantiate.
        "wasi_snapshot_preview1"
    }
}

/// v0.14 stdlib-direct lowering descriptors. Each variant names a
/// versioned P2 import the codegen layer can splice into a core
/// module under construction, in place of an equivalent P1 syscall.
///
/// Kept as a flat enum rather than free functions so callers can
/// pattern-match for tests + diagnostics without coupling to the
/// specific `wasm-encoder` types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P2DirectImport {
    /// `wasi:random/random@0.2.3#get-random-bytes(len: u64) -> list<u8>`
    /// — the canonical "secure random bytes" call. Mighty's
    /// `std.random.bytes(n)` lowers to this on `--wasi=p2`.
    RandomBytes,
    /// `wasi:clocks/monotonic-clock@0.2.3#now() -> instant`
    /// — monotonic instant in nanoseconds since an unspecified epoch.
    /// Mighty's `std.time.monotonic_now()` lowers to this on
    /// `--wasi=p2`.
    MonotonicNow,
    /// `wasi:clocks/wall-clock@0.2.3#now() -> datetime`
    /// — wall-clock instant (seconds + nanos since UNIX epoch).
    /// Mighty's `std.time.now()` lowers to this on `--wasi=p2`.
    WallClockNow,
    /// `wasi:clocks/monotonic-clock@0.2.3#resolution() -> instant`
    /// — resolution of the monotonic clock in nanoseconds. Mighty's
    /// `std.time.resolution()` lowers to this on `--wasi=p2`.
    MonotonicResolution,
}

impl P2DirectImport {
    /// The `(module_name, fn_name)` pair as it appears in the core
    /// Wasm module's import section. Module names match the WIT
    /// "namespace/interface@version" form that
    /// `wit-component::ComponentEncoder` lifts into the P2 component
    /// without an adapter hop.
    pub fn import_pair(self) -> (&'static str, &'static str) {
        match self {
            P2DirectImport::RandomBytes => ("wasi:random/random@0.2.3", "get-random-bytes"),
            P2DirectImport::MonotonicNow => ("wasi:clocks/monotonic-clock@0.2.3", "now"),
            P2DirectImport::WallClockNow => ("wasi:clocks/wall-clock@0.2.3", "now"),
            P2DirectImport::MonotonicResolution => {
                ("wasi:clocks/monotonic-clock@0.2.3", "resolution")
            }
        }
    }

    /// Stable name used in diagnostics, test assertions and the
    /// `Display` impl. Matches the variant ident in snake-case.
    pub fn label(self) -> &'static str {
        match self {
            P2DirectImport::RandomBytes => "random_bytes",
            P2DirectImport::MonotonicNow => "monotonic_now",
            P2DirectImport::WallClockNow => "wall_clock_now",
            P2DirectImport::MonotonicResolution => "monotonic_resolution",
        }
    }
}

impl std::fmt::Display for P2DirectImport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (m, n) = self.import_pair();
        write!(f, "{m}#{n}")
    }
}

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
    /// Embed the vendored P1→P2 adapter when wrapping. v0.14 default:
    /// `Some(AdapterKind::Command)` — every Mighty build today emits a
    /// command-shaped component, and the adapter lets the core
    /// module's existing `wasi_snapshot_preview1`-shaped imports
    /// translate into versioned P2 interface calls at instantiation.
    ///
    /// Set to `None` for builds that already speak pure P2 (when the
    /// v0.15 direct-lowering work lands). Set to a different
    /// [`AdapterKind`] for reactor/proxy components.
    pub embed_adapter: Option<AdapterKind>,
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
            embed_adapter: Some(AdapterKind::Command),
        }
    }

    pub fn with_user_wit(mut self, uw: UserWit) -> Self {
        self.user_wit = Some(uw);
        self
    }

    /// Override the embedded adapter kind. Pass `None` to skip
    /// adapter embedding entirely (only safe when the core module
    /// already exclusively imports versioned P2 interfaces — the
    /// v0.15 direct-lowering work).
    pub fn with_adapter(mut self, adapter: Option<AdapterKind>) -> Self {
        self.embed_adapter = adapter;
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

    // We assemble the WIT document as a single text blob (returned in
    // [`WitDocument::text`] for callers that want to display it), but
    // we feed `wit_parser::Resolve` *separate* package files so
    // each top-level `package X { ... }` can cross-reference the
    // others (the parser only allows cross-package references between
    // distinct top-level files, not between nested packages in a
    // single file).
    let mut user_body = String::new();
    user_body.push_str("// AUTO-GENERATED by mty-codegen-wasm (WASI Preview 2 path).\n");
    user_body.push_str(&format!(
        "// Target: wasm32-wasi, WASI version: {}.\n",
        WASI_P2_VERSION
    ));
    user_body.push_str(&format!(
        "// Adapter: {} (wasi_snapshot_preview1 → P2).\n\n",
        WASI_P1_ADAPTER_VERSION
    ));

    // -- 1. The synthesized Mighty package + its primary world. This
    //       must be a *top-level* `package X:Y;` so the resolver
    //       returns it as the document's primary package.
    let mighty_pkg_text = format!(
        "package mighty:{pkg_id};\n\n\
         world {synth_world_name} {{\n\
         {imports}\
           export main: func();\n\
         }}\n",
        imports = synth_world_imports(),
    );

    // -- 2. The unversioned `wasi:cli` shim. Carries only the `log`
    //       interface so `wit-component` can resolve the core
    //       module's literal `wasi:cli/log#log` import (which has no
    //       `@0.2.3` annotation). Co-exists with the versioned
    //       wasi:cli@0.2.3 package because they have different
    //       package versions.
    let cli_shim_text = "package wasi:cli;\n\
         interface log {\n\
           log: func(msg: string);\n\
         }\n"
    .to_string();

    // For display + assertions: the public `WitDocument::text` field
    // concatenates everything so test/console pretty-printing still
    // sees one blob.
    user_body.push_str(&mighty_pkg_text);
    user_body.push('\n');
    user_body.push_str(&cli_shim_text);
    user_body.push('\n');
    user_body.push_str("// ---- Vendored WASI Preview 2 surface (0.2.3) ----\n");
    user_body.push_str(VENDORED_WASI_P2_WIT);
    user_body.push('\n');

    // Phase 2: merge the user's WIT, if any. The user's text may use
    // top-level `package X:Y;` form (canonical when authored as a
    // standalone .wit file).
    if let Some(uw) = &opts.user_wit {
        user_body.push_str("\n// ---- USER-SUPPLIED WIT BELOW ----\n");
        user_body.push_str(&uw.text);
        user_body.push('\n');
    }

    // Round-trip validation — push each top-level package as its own
    // file so they can cross-reference. The vendored P2 slice already
    // contains *nested* packages (one file with multiple
    // `package wasi:X@0.2.3 { ... }` blocks); we have to split it on
    // package-block boundaries first.
    let mut resolve = wit_parser::Resolve::default();
    for (label, pkg_text) in split_nested_into_packages(VENDORED_WASI_P2_WIT, "vendored-p2") {
        let _ = resolve
            .push_str(&label, &pkg_text)
            .map_err(|e| WasmError::Invalid(format!("wit p2 round-trip vendored: {e:#}")))?;
    }
    let _ = resolve
        .push_str("mighty-cli-shim.wit", &cli_shim_text)
        .map_err(|e| WasmError::Invalid(format!("wit p2 round-trip cli shim: {e:#}")))?;
    let _ = resolve
        .push_str("mighty-main.wit", &mighty_pkg_text)
        .map_err(|e| WasmError::Invalid(format!("wit p2 round-trip mighty: {e:#}")))?;
    if let Some(uw) = &opts.user_wit {
        let _ = resolve
            .push_str(&uw.source_label, &uw.text)
            .map_err(|e| WasmError::Invalid(format!("wit p2 round-trip user: {e:#}")))?;
    }

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
    wrap_p2(&core, &doc, opts.embed_adapter, opts.user_wit.as_ref())
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
    let bytes = wrap_p2(&core, &doc, opts.embed_adapter, opts.user_wit.as_ref())?;
    std::fs::write(out, &bytes)
        .map_err(|e| WasmError::Io(format!("write {}: {}", out.display(), e)))?;
    Ok((bytes, doc))
}

/// Wrap a core module as a P2 component, doing the world lookup
/// across *every* package in the resolve (not just the synthesized
/// `mighty:<pkg>` one). This is the difference from
/// [`crate::component::wrap_as_component`], which assumes the world
/// lives in the document's primary package.
///
/// When `embed_adapter` is `Some`, the vendored
/// `wasi_snapshot_preview1` adapter is passed to
/// [`wit_component::ComponentEncoder::adapter`] so the core module's
/// P1-shaped imports are translated into versioned P2 calls at
/// instantiation. The adapter add ~80 KB to the component (mostly
/// constant; wasmtime strips unused adapter exports during
/// `ComponentEncoder::encode`).
fn wrap_p2(
    core_module: &[u8],
    doc: &WitDocument,
    embed_adapter: Option<AdapterKind>,
    user_wit: Option<&UserWit>,
) -> CompileResult<Vec<u8>> {
    // Re-derive the per-package text the way `emit_wit_p2` did so the
    // resolver sees each top-level `package` as a separate file (the
    // only way wit-parser permits cross-package references). The
    // canonical text we display in `doc.text` is *not* round-trip
    // parse-able as a single blob — that's a documented quirk of
    // the multi-package serialization.
    let mut resolve = wit_parser::Resolve::default();
    // Vendored P2 packages, one push_str per package.
    for (label, pkg_text) in split_nested_into_packages(VENDORED_WASI_P2_WIT, "vendored-p2") {
        let _ = resolve
            .push_str(&label, &pkg_text)
            .map_err(|e| WasmError::Invalid(format!("p2 wrap vendored: {e:#}")))?;
    }
    // The wasi:cli (unversioned) shim for the slice-8 log import.
    let cli_shim_text = "package wasi:cli;\n\
         interface log {\n\
           log: func(msg: string);\n\
         }\n";
    let _ = resolve
        .push_str("mighty-cli-shim.wit", cli_shim_text)
        .map_err(|e| WasmError::Invalid(format!("p2 wrap cli shim: {e:#}")))?;
    // Re-synthesize the mighty package so we know the package id for
    // its `select_world` call.
    let mighty_pkg_text = format!(
        "package {pkg_id};\n\n\
         world {world_name} {{\n\
         {imports}\
           export main: func();\n\
         }}\n",
        pkg_id = doc.package_id,
        world_name = if doc.world_name.is_empty() {
            "mighty-world".to_string()
        } else {
            doc.world_name.clone()
        },
        imports = synth_world_imports(),
    );
    let mighty_pkg_id = resolve
        .push_str("mighty-main.wit", &mighty_pkg_text)
        .map_err(|e| WasmError::Invalid(format!("p2 wrap mighty: {e:#}")))?;
    if let Some(uw) = user_wit {
        let _ = resolve
            .push_str(&uw.source_label, &uw.text)
            .map_err(|e| WasmError::Invalid(format!("p2 wrap user wit: {e:#}")))?;
    }

    // Find which package owns the world named `doc.world_name`. The
    // mighty package is the most likely candidate; if a user-WIT
    // overrode the world name, fall back to searching every package
    // in the resolve.
    let mut world_id = resolve
        .select_world(mighty_pkg_id, Some(&doc.world_name))
        .ok();
    if world_id.is_none() {
        // Lookup across every package — handles the user-supplied
        // world case (e.g. `[wit] world = "custom-world"` from a
        // demo:user-pkg package).
        let pkg_ids: Vec<_> = resolve.packages.iter().map(|(id, _)| id).collect();
        for pkg_id in pkg_ids {
            if let Ok(w) = resolve.select_world(pkg_id, Some(&doc.world_name)) {
                world_id = Some(w);
                break;
            }
        }
    }
    let world_id = world_id.ok_or_else(|| {
        WasmError::Invalid(format!(
            "p2 wrap: world '{}' not found in any package",
            doc.world_name
        ))
    })?;

    let mut module_bytes = core_module.to_vec();

    // The wasmtime adapter expects a *command*-shape core module
    // when [`AdapterKind::Command`] is in use, which means an
    // exported `_start: func()`. Mighty's slice-8 core module
    // exports `main` (not `_start`). Synthesize a `_start` export
    // that aliases `main` so the adapter's `wasi:cli/run.run`
    // re-export is satisfied.
    //
    // Only run this when the core module is missing `_start` —
    // if a future emitter path provides it directly we leave the
    // module untouched.
    if matches!(embed_adapter, Some(AdapterKind::Command))
        && !module_exports_func(&module_bytes, "_start")
    {
        module_bytes = alias_main_as_start(&module_bytes)?;
    }

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
    if let Some(kind) = embed_adapter {
        enc = enc
            .adapter(kind.import_module_name(), kind.bytes())
            .map_err(|e| {
                WasmError::Invalid(format!(
                    "p2 adapter embed ({}): {e:#}",
                    kind.import_module_name()
                ))
            })?;
    }
    enc.encode()
        .map_err(|e| WasmError::Invalid(format!("p2 component encode: {e:#}")))
}

/// Return true iff `module_bytes` declares an exported function
/// named `name`.
fn module_exports_func(module_bytes: &[u8], name: &str) -> bool {
    use wasmparser::{ExternalKind, Parser, Payload};
    for payload in Parser::new(0).parse_all(module_bytes) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for ex in reader.into_iter().flatten() {
                if matches!(ex.kind, ExternalKind::Func) && ex.name == name {
                    return true;
                }
            }
        }
    }
    false
}

/// Find the func-index that's currently exported as `main`. Returns
/// `None` if no such export exists (the slice-8 emitter doesn't
/// always synthesize `main`).
fn find_main_export(module_bytes: &[u8]) -> Option<u32> {
    use wasmparser::{ExternalKind, Parser, Payload};
    for payload in Parser::new(0).parse_all(module_bytes) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for ex in reader.into_iter().flatten() {
                if matches!(ex.kind, ExternalKind::Func) && ex.name == "main" {
                    return Some(ex.index);
                }
            }
        }
    }
    None
}

/// Add an export `_start: func()` to `module_bytes`. The export
/// aliases the existing `main` export (the wasmtime command-adapter
/// invokes `_start`, which is what wasi-libc-built programs emit;
/// Mighty's slice-8 emitter still uses `main`).
///
/// Implementation: parse the existing module, copy every section
/// verbatim, and rewrite the export section to add `_start`. We
/// don't add a *new* function — the export simply points at the
/// same func index as `main`.
fn alias_main_as_start(module_bytes: &[u8]) -> CompileResult<Vec<u8>> {
    use wasm_encoder::{ExportKind as WExportKind, ExportSection, Module, RawSection};
    use wasmparser::{ExternalKind, Parser, Payload};

    let Some(main_idx) = find_main_export(module_bytes) else {
        // No `main` to alias from — return unchanged so the existing
        // diagnostics (from wit-component) surface.
        return Ok(module_bytes.to_vec());
    };

    // Walk payloads and rewrite the export section. wasm-encoder's
    // `RawSection` lets us splice unchanged sections back in
    // byte-for-byte.
    let mut new_module = Module::new();
    let mut handled_export = false;
    for payload in Parser::new(0).parse_all(module_bytes) {
        let payload =
            payload.map_err(|e| WasmError::Invalid(format!("alias_main_as_start parse: {e:#}")))?;
        match payload {
            Payload::Version { .. } => {}
            Payload::ExportSection(reader) => {
                let mut new_exports = ExportSection::new();
                for ex in reader.into_iter() {
                    let ex = ex.map_err(|e| WasmError::Invalid(format!("export iter: {e:#}")))?;
                    let kind = match ex.kind {
                        ExternalKind::Func | ExternalKind::FuncExact => WExportKind::Func,
                        ExternalKind::Table => WExportKind::Table,
                        ExternalKind::Memory => WExportKind::Memory,
                        ExternalKind::Global => WExportKind::Global,
                        ExternalKind::Tag => WExportKind::Tag,
                    };
                    new_exports.export(ex.name, kind, ex.index);
                }
                new_exports.export("_start", WExportKind::Func, main_idx);
                new_module.section(&new_exports);
                handled_export = true;
            }
            // Re-emit any other section verbatim. wasmparser's
            // `Payload::*::range()` gives us the original byte range.
            other => {
                if let Some((id, range)) = section_passthrough(&other) {
                    new_module.section(&RawSection {
                        id,
                        data: &module_bytes[range],
                    });
                }
            }
        }
    }
    // If the source module had no export section, append one with
    // just our synthetic `_start`. (Shouldn't happen for a Mighty-
    // compiled core module but the helper is defensive.)
    if !handled_export {
        let mut new_exports = ExportSection::new();
        new_exports.export("_start", WExportKind::Func, main_idx);
        new_module.section(&new_exports);
    }
    Ok(new_module.finish())
}

/// Return `Some((section_id, byte_range))` for the source-byte range
/// of any wasm payload we want to copy verbatim into the rewritten
/// module. Returns `None` for payloads that don't correspond to a
/// section we should pass through (e.g. `Payload::End`,
/// `Payload::Version`, the export section we're rewriting, etc.).
fn section_passthrough(payload: &wasmparser::Payload<'_>) -> Option<(u8, std::ops::Range<usize>)> {
    use wasmparser::Payload::*;
    match payload {
        TypeSection(s) => Some((1, s.range())),
        ImportSection(s) => Some((2, s.range())),
        FunctionSection(s) => Some((3, s.range())),
        TableSection(s) => Some((4, s.range())),
        MemorySection(s) => Some((5, s.range())),
        GlobalSection(s) => Some((6, s.range())),
        // ExportSection is rewritten above; do not pass through.
        ExportSection(_) => None,
        StartSection { range, .. } => Some((8, range.clone())),
        ElementSection(s) => Some((9, s.range())),
        CodeSectionStart { range, .. } => Some((10, range.clone())),
        DataSection(s) => Some((11, s.range())),
        DataCountSection { range, .. } => Some((12, range.clone())),
        CustomSection(s) => Some((0, s.range())),
        TagSection(s) => Some((13, s.range())),
        // Payloads with no on-the-wire bytes:
        Version { .. } | End(_) => None,
        // Component-model + GC payloads — we don't expect to see
        // these in a Mighty-emitted core module, but be defensive.
        _ => None,
    }
}

/// Build a *standalone* core Wasm module that imports a single P2
/// interface call described by `which` and re-exports it under
/// `_start`. Used by the v0.14 direct-lowering tests to demonstrate
/// the helper produces a wrapping-component whose imports reference
/// the versioned P2 interface verbatim (no `wasi_snapshot_preview1`
/// hop).
///
/// The module is intentionally trivial — one import, one `_start`
/// function that calls it and traps on the return — so the
/// `wit-component` machinery has nothing else to do besides wire the
/// versioned import through. Callers in real codegen splice the
/// import into the existing module-under-construction via the same
/// `(module_name, fn_name)` pair returned by
/// [`P2DirectImport::import_pair`].
///
/// This function only exists so that the test suite (and any future
/// integration test) can exercise the P2-direct path without having
/// to hand-roll a `wasm-encoder` module each time. **It is not on
/// the build path** — `compile_program_to_bytes_p2` still produces
/// the full Mighty core module from `Program`.
pub fn build_direct_p2_probe_module(which: P2DirectImport) -> Vec<u8> {
    use wasm_encoder::{
        CodeSection, EntityType, ExportKind, ExportSection, Function as WFn, FunctionSection,
        ImportSection, Instruction as I, MemoryType, Module, TypeSection, ValType,
    };

    let mut module = Module::new();

    // Pick a signature that's loose enough to satisfy *any* of the
    // direct imports we currently emit. For the v0.14 probe the
    // signature is only used by the import declaration — we don't
    // actually call into the host, the probe's `_start` just returns.
    //
    //   get-random-bytes: (param i32) (param i32) (result)
    //     → P2-canonical-ABI lifted shape
    //   clock now / resolution / wall-clock now: (param i32) (result)
    //     → return-via-pointer
    //
    // We pick a uniform `(i32, i32) -> ()` so a single type entry
    // covers all three. The wit-component encoder is happy as long
    // as the import's module name matches the WIT interface; the
    // *signature* gets re-lifted under canonical-ABI translation
    // anyway.
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32, ValType::I32], []);
    types.ty().function([], []);
    module.section(&types);

    let (mod_name, fn_name) = which.import_pair();
    let mut imports = ImportSection::new();
    imports.import(mod_name, fn_name, EntityType::Function(0));
    module.section(&imports);

    let mut funcs = FunctionSection::new();
    funcs.function(1);
    module.section(&funcs);

    // Minimal `(memory 1)` so wit-component's canonical-ABI lifting
    // has somewhere to land its returned-list payload.
    let mut memory = wasm_encoder::MemorySection::new();
    memory.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memory);

    let mut exports = ExportSection::new();
    exports.export("memory", ExportKind::Memory, 0);
    // `_start` is function index 1 (after the 1 imported fn).
    exports.export("_start", ExportKind::Func, 1);
    module.section(&exports);

    let mut code = CodeSection::new();
    let mut start = WFn::new([]);
    start.instruction(&I::End);
    code.function(&start);
    module.section(&code);

    module.finish()
}

/// Body lines for the synthesized world's import section. Lifted out
/// of `emit_wit_p2` so both that function and `wrap_p2` can share the
/// same import list when assembling per-package WIT files.
fn synth_world_imports() -> String {
    let lines = [
        "  import wasi:cli/environment@0.2.3;",
        "  import wasi:cli/exit@0.2.3;",
        "  import wasi:cli/stdin@0.2.3;",
        "  import wasi:cli/stdout@0.2.3;",
        "  import wasi:cli/stderr@0.2.3;",
        "  import wasi:io/error@0.2.3;",
        "  import wasi:io/poll@0.2.3;",
        "  import wasi:io/streams@0.2.3;",
        "  import wasi:clocks/monotonic-clock@0.2.3;",
        "  import wasi:clocks/wall-clock@0.2.3;",
        "  import wasi:random/random@0.2.3;",
        "  import wasi:filesystem/preopens@0.2.3;",
        "  import wasi:filesystem/types@0.2.3;",
        "  import wasi:http/types@0.2.3;",
        "  import wasi:http/outgoing-handler@0.2.3;",
        // v0.14 boundary — `wasi:cli/log` is an unversioned shim
        // (declared in `mighty-cli-shim.wit`) used by the slice-8
        // emitter's `log()` lowering. A future slice replaces it
        // with a real `wasi:cli/stdout#print` lowering.
        "  import wasi:cli/log;",
    ];
    let mut s = String::new();
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    s
}

/// Topological order for the vendored WASI Preview 2 packages.
/// `Resolve::push_str` rejects forward references between top-level
/// files — every `use wasi:io/...` requires that `wasi:io` already
/// be in the resolve. The ordering below matches the WASI 0.2.3
/// package dependency DAG:
///
/// ```text
///   wasi:io      ── no deps
///   wasi:clocks  → wasi:io
///   wasi:random  → no deps
///   wasi:sockets → wasi:io
///   wasi:filesystem → wasi:io, wasi:clocks
///   wasi:cli     → all of the above
///   wasi:http    → wasi:io, wasi:clocks, wasi:cli
/// ```
const VENDORED_P2_PKG_ORDER: &[&str] = &[
    "wasi:io@0.2.3",
    "wasi:clocks@0.2.3",
    "wasi:random@0.2.3",
    "wasi:sockets@0.2.3",
    "wasi:filesystem@0.2.3",
    "wasi:cli@0.2.3",
    "wasi:http@0.2.3",
];

/// Split a multi-package text (nested `package X:Y { ... }` blocks
/// at the top level) into individual top-level files, one per
/// package. The returned strings carry a `package X:Y;` declaration
/// (no braces) followed by the original block body, ready to feed
/// directly to `wit_parser::Resolve::push_str`.
///
/// The order of returned chunks matches the
/// [`VENDORED_P2_PKG_ORDER`] DAG so callers can push them straight
/// into a resolve without worrying about forward-reference errors.
///
/// `label_prefix` is used to disambiguate the filename labels we
/// hand to `push_str` (these only appear in parser diagnostics).
fn split_nested_into_packages(text: &str, label_prefix: &str) -> Vec<(String, String)> {
    // Triple is (filename label, package name, chunk text). The package
    // name is used only to topologically order the chunks before they're
    // handed back to the caller as `(label, chunk)`.
    let mut out: Vec<(String, String, String)> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        // Skip whitespace + comment lines until we see a `package`
        // keyword followed by an open brace.
        // A simple approach: scan for the substring `package ` at
        // start of a logical line, then look for the matching `{`.
        let rest = &text[i..];
        // Skip leading whitespace + comments
        let start = match find_next_package_block(rest) {
            Some(p) => p + i,
            None => break,
        };
        // Parse `package <name> {` -- extract the name and the brace
        // position.
        let pkg_start = &text[start..];
        let (pkg_name, brace_open) = match parse_package_header(pkg_start) {
            Some(v) => v,
            None => {
                i = start + 1;
                continue;
            }
        };
        // Find matching close brace.
        let body_start = start + brace_open + 1;
        let mut depth: u32 = 1;
        let mut j = body_start;
        while j < n && depth > 0 {
            match bytes[j] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        if depth != 0 {
            // unbalanced — bail. The error will surface when we feed
            // the truncated chunk to `push_str` below.
            break;
        }
        let body = &text[body_start..(j - 1)];
        let label = format!(
            "{label_prefix}-{}.wit",
            pkg_name.replace(':', "-").replace('@', "_")
        );
        let chunk = format!("package {pkg_name};\n{body}\n");
        out.push((label, pkg_name, chunk));
        i = j;
    }
    // Topologically reorder using `VENDORED_P2_PKG_ORDER`. Any chunk
    // not in the ordering list is appended at the end (preserves the
    // text-order for new packages someone forgot to add to the
    // ordering const).
    let mut ordered: Vec<(String, String)> = Vec::with_capacity(out.len());
    for target in VENDORED_P2_PKG_ORDER {
        if let Some(pos) = out.iter().position(|(_, name, _)| name == target) {
            let (label, _name, chunk) = out.swap_remove(pos);
            ordered.push((label, chunk));
        }
    }
    for (label, _, chunk) in out {
        ordered.push((label, chunk));
    }
    ordered
}

/// Return the byte offset of the next `package` keyword that begins a
/// top-level nested block (look for `package <name> {`). Returns `None`
/// when no such block is found in `text`.
fn find_next_package_block(text: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(pos) = text[search_from..].find("package ") {
        let absolute = search_from + pos;
        // Only accept matches that look like the start of a top-level
        // decl: preceded by start-of-string or a newline (and only
        // whitespace between the newline and the keyword).
        let prefix_ok = if absolute == 0 {
            true
        } else {
            let prev = &text[..absolute];
            // Walk backward over the same line — accept only spaces/tabs.
            let line_start = prev.rfind('\n').map(|p| p + 1).unwrap_or(0);
            text[line_start..absolute]
                .chars()
                .all(|c| c == ' ' || c == '\t')
        };
        // Also require that this is a *block* form, not a `package X;`
        // top-level decl. Look ahead for the next non-comment token to
        // see if it's `{`. Cheap proxy: scan forward to the next `{`
        // or `;` and check which comes first.
        let after = &text[absolute..];
        let semi = after.find(';');
        let brace = after.find('{');
        let is_block = match (semi, brace) {
            (Some(s), Some(b)) => b < s,
            (None, Some(_)) => true,
            _ => false,
        };
        if prefix_ok && is_block {
            return Some(absolute);
        }
        search_from = absolute + "package ".len();
    }
    None
}

/// Given text starting with `package <name> {`, return the package
/// name (without braces) and the byte offset of the opening `{`.
fn parse_package_header(text: &str) -> Option<(String, usize)> {
    let after = text.strip_prefix("package ")?;
    let brace = after.find('{')?;
    let name = after[..brace].trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some((name, "package ".len() + brace))
}

// `wrap_user_wit_as_nested` (v0.13) used to fold user `.wit` text into
// nested-package form so it could be concatenated into one big blob
// passed to `Resolve::push_str`. The v0.14 architecture pushes each
// top-level package separately, so the helper is no longer needed —
// user WIT is fed verbatim to the resolver.

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
        // Note: `doc.text` is a multi-package display blob — under
        // the v0.14 architecture it does NOT round-trip through a
        // single `Resolve::push_str` call (each top-level package
        // must be pushed separately). The end-to-end validation that
        // the merge succeeded is `emit_wit_p2`'s internal round-trip
        // (run before `Ok(doc)` returns above), which has already
        // succeeded by the time we get here.
    }

    #[test]
    fn p2_component_wraps() {
        let opts = Preview2Options::new("hello");
        let bytes = compile_program_to_bytes_p2(&empty_main(), &opts).expect("compile p2");
        assert!(crate::component::is_component(&bytes));
    }
}
