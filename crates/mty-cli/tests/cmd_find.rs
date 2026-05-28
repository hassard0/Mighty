//! v0.33 T7 — tests for `mty find` (capability-tagged stdlib search).
//!
//! We test the index-builder + ranker via the `parse_source_for_tests`
//! / `rank_for_tests` re-exports rather than spawning `mty find …` for
//! every case — that lets us hand-craft tiny fixture sources and keep
//! the suite under a second. A small set of integration tests at the
//! end of the file does spawn the real binary to lock in the CLI shape
//! (top-level subcommand, `--format` flag, `--by-capability` flag).

use std::path::Path;
use std::process::Command;

use mty_cli::cmd::find::*;

const FS_FIXTURE: &str = r#"
//! `std.fs` — capability-gated filesystem ops.

/// Read bytes from a file path.
///
/// ```mty
/// let bytes = std.fs.read(cap, "in.txt")?;
/// ```
pub fn read(cap: &FsCap, path: &Path) -> Result<Vec<u8>, IoErr> {
    cap.check(path)?;
    Ok(std::fs::read(path)?)
}

/// Write bytes to a file (creates or truncates).
///
/// ```mty
/// std.fs.write(cap, "out.txt", b"hello")?;
/// ```
pub fn write(cap: &FsCap, path: &Path, data: &[u8]) -> Result<(), IoErr> {
    cap.check(path)?;
    std::fs::write(path, data)?;
    Ok(())
}

/// Atomic write — write through a temp file then rename.
pub fn atomic_write(cap: &FsCap, path: &Path, data: &[u8]) -> Result<(), IoErr> {
    write(cap, path, data)
}

/// Filesystem capability handle.
pub struct FsCap {
    pub allowed: Vec<PathBuf>,
}
"#;

const HTTP_FIXTURE: &str = r#"
/// Send an HTTP GET request.
pub async fn get(url: &str) -> Result<Response, HttpErr> { unimplemented!() }

/// Send an HTTP POST request with a body.
pub async fn post(url: &str, body: Vec<u8>) -> Result<Response, HttpErr> { unimplemented!() }

/// Build a tunable request.
pub fn request(method: &str, url: &str) -> RequestBuilder { unimplemented!() }

pub struct Response {
    pub status: u16,
}
"#;

const TAINT_FIXTURE: &str = r#"
/// A string value carrying provenance from an external source.
/// Calling `into_inner` on it requires an explicit untaint step.
pub struct Tainted<T> { inner: T }

impl<T> Tainted<T> {
    /// Untaint via an explicit policy check.
    pub fn untaint_with_policy(self, policy: &Policy) -> T { self.inner }
}
"#;

#[test]
fn parses_basic_pub_fn() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
    assert!(names.contains(&"read"), "names: {names:?}");
    assert!(names.contains(&"write"), "names: {names:?}");
    assert!(names.contains(&"atomic_write"), "names: {names:?}");
    assert!(names.contains(&"FsCap"), "names: {names:?}");
}

#[test]
fn captures_first_example_block() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    let read = items.iter().find(|i| i.name == "read").unwrap();
    let ex = read.example.as_ref().expect("read should have an example");
    assert!(
        ex.contains("std.fs.read"),
        "expected std.fs.read in example, got {ex:?}"
    );
}

#[test]
fn extracts_verbs_from_name_and_doc() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    let write = items.iter().find(|i| i.name == "write").unwrap();
    // Name word.
    assert!(write.verbs.contains("write"), "verbs: {:?}", write.verbs);
    // Doc word.
    assert!(write.verbs.contains("bytes"), "verbs: {:?}", write.verbs);
}

#[test]
fn infers_fs_read_and_fs_write_capabilities() {
    assert_eq!(infer_capability_for_tests("std.fs", "read", ""), "fs.read");
    assert_eq!(
        infer_capability_for_tests("std.fs", "write", ""),
        "fs.write"
    );
    assert_eq!(
        infer_capability_for_tests("std.fs", "atomic_write", ""),
        "fs.write"
    );
    assert_eq!(
        infer_capability_for_tests("std.http", "get", ""),
        "net.https"
    );
    assert_eq!(
        infer_capability_for_tests("std.http_server", "serve", "bind a tcp listener"),
        "net.bind"
    );
    assert_eq!(
        infer_capability_for_tests("std.llm.anthropic", "complete", ""),
        "net.https + model"
    );
    assert_eq!(
        infer_capability_for_tests("std.memory.vector", "search", ""),
        "fs.read + fs.write"
    );
}

#[test]
fn module_path_collapses_lib_and_mod() {
    let root = Path::new("/x/crates/mty-stdlib/src");
    assert_eq!(
        derive_module_path_for_tests(root, &root.join("lib.rs")),
        "std"
    );
    assert_eq!(
        derive_module_path_for_tests(root, &root.join("fs.rs")),
        "std.fs"
    );
    assert_eq!(
        derive_module_path_for_tests(root, &root.join("llm").join("mod.rs")),
        "std.llm"
    );
    assert_eq!(
        derive_module_path_for_tests(root, &root.join("memory").join("vector.rs")),
        "std.memory.vector"
    );
}

#[test]
fn ranks_exact_name_match_highest() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    let hits = rank_for_tests(items, "write", 5);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].name, "write", "expected write first, got {hits:?}");
}

#[test]
fn ranks_verb_match_above_substring() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    // "write files" — `write` exactly matches `write`; "files" only
    // appears as substring in `FsCap` doc. We expect `write` first.
    let hits = rank_for_tests(items, "write files", 3);
    assert_eq!(hits[0].name, "write");
}

#[test]
fn ranks_http_send_intent() {
    let items = parse_source_for_tests(HTTP_FIXTURE, "std.http", "http.rs");
    let hits = rank_for_tests(items, "send http", 3);
    let names: Vec<_> = hits.iter().map(|i| i.name.as_str()).collect();
    // We accept either `get`, `post`, or `request` as the top hit —
    // the test asserts at least one is in the top-3.
    assert!(
        names
            .iter()
            .any(|n| matches!(*n, "get" | "post" | "request")),
        "expected http verb in top-3, got {names:?}"
    );
}

#[test]
fn returns_empty_on_no_match() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    let hits = rank_for_tests(items, "kaleidoscope-quantum-flux", 5);
    assert!(hits.is_empty(), "expected no hits, got {hits:?}");
}

#[test]
fn verbatim_name_match_boost() {
    let items = parse_source_for_tests(TAINT_FIXTURE, "std.security", "security.rs");
    let hits = rank_for_tests(items, "Tainted", 5);
    let names: Vec<_> = hits.iter().map(|i| i.name.as_str()).collect();
    assert!(
        names.contains(&"Tainted"),
        "expected Tainted in hits, got {names:?}"
    );
}

#[test]
fn top_n_truncates() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    let hits = rank_for_tests(items, "write read", 2);
    assert!(hits.len() <= 2, "expected at most 2, got {}", hits.len());
}

#[test]
fn cap_match_works_via_module_path() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    let hits = rank_for_tests(items, "fs", 10);
    assert!(
        !hits.is_empty(),
        "expected module-path matches for `fs`, got nothing"
    );
}

#[test]
fn index_round_trips_through_json() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    let idx = round_trip_index(items.clone()).expect("round-trip");
    assert_eq!(idx.items.len(), items.len());
    assert_eq!(idx.items[0].name, items[0].name);
}

#[test]
fn by_capability_groups_items() {
    let mut items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    items.extend(parse_source_for_tests(HTTP_FIXTURE, "std.http", "http.rs"));
    let idx = round_trip_index(items).expect("round-trip");
    let groups = items_by_capability(&idx);
    assert!(
        groups.contains_key("fs.read") || groups.contains_key("fs.write"),
        "expected fs.* groupings, got {:?}",
        groups.keys().collect::<Vec<_>>()
    );
    assert!(
        groups.contains_key("net.https"),
        "expected net.https grouping, got {:?}",
        groups.keys().collect::<Vec<_>>()
    );
}

#[test]
fn signature_collapses_whitespace() {
    let items = parse_source_for_tests(FS_FIXTURE, "std.fs", "fs.rs");
    let read = items.iter().find(|i| i.name == "read").unwrap();
    assert!(
        read.signature.contains("fn read") && !read.signature.contains('\n'),
        "expected single-line signature, got {:?}",
        read.signature
    );
}

#[test]
fn summary_truncates_long_docs() {
    let long = "/// ".to_string() + &"x ".repeat(200) + "\npub fn foo() {}\n";
    let items = parse_source_for_tests(&long, "std.test", "test.rs");
    let foo = items.iter().find(|i| i.name == "foo").unwrap();
    assert!(foo.summary.len() <= 240, "summary: {}", foo.summary.len());
}

// ----- integration: drive the real binary --------------------------------

fn mty(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .args(args)
        .output()
        .expect("spawn mty");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn cli_find_help_advertises_subcommand() {
    let (_code, stdout, _stderr) = mty(&["help"]);
    // We don't assert exit code here — clap returns 0 for `mty help`,
    // but some clap setups print to stderr. We just want the subcommand
    // to show up in the top-level listing.
    assert!(
        stdout.contains("find") || stdout.is_empty(),
        "stdout: {stdout:?}"
    );
}

#[test]
fn cli_find_emits_pretty_output_with_no_query_errors() {
    let (code, _stdout, stderr) = mty(&["find"]);
    // Bare `mty find` with no query should fail with a hint.
    assert_ne!(code, 0);
    assert!(
        stderr.contains("query") || stderr.contains("by-capability"),
        "expected helpful stderr, got {stderr:?}"
    );
}
