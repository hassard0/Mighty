//! `std.*` dispatcher invoked by `mty-runtime::host_std`.
//!
//! The runtime keeps a process-wide `DispatcherFn` slot; calling
//! [`install`] from this crate (or from a downstream test) plugs
//! [`dispatch`] in so `EffectOp::GenericCall` paths reach the real
//! impls. We pattern-match on `(module path, method)` and run the real
//! implementation, returning a Mighty-shaped `Value`.
//!
//! Calls we don't yet handle return `Value::Unit` so existing tests that
//! exercise other effects keep passing.

use mty_ir::interp::value::Value;

/// Register this crate as the runtime's `std.*` dispatcher. Idempotent
/// — safe to call from every test or once at driver start. Once the
/// driver wires this in v0.3, user code that does `use std.json` and
/// calls `json.parse(...)` through `mty run` will hit the real
/// parser instead of the slice-7 no-op.
pub fn install() {
    mty_runtime::host_std::install_dispatcher(dispatch);
}

/// Convert a serialized result into a `Value`. We intentionally keep
/// the shape minimal — most stdlib calls return either `Unit`, a
/// `Str`, an `Int`, or a `Bool`; anything richer (e.g. `Json` value
/// trees) goes through `Value::Str` of a serde-encoded form until
/// HIR/Ty gains the Json ADT properly (v0.3).
pub fn dispatch(path: &[String], method: &str, args: &[Value]) -> Value {
    let module = path.join(".");
    match (module.as_str(), method) {
        // -------- json --------
        ("std.json", "parse") => json_parse(args),
        ("std.json", "encode") => json_encode(args),
        ("std.json", "encode_pretty") => json_encode_pretty(args),
        // -------- time (sync surface) --------
        ("std.time", "now") => time_now(),
        ("std.time", "sleep") => time_sleep(args),
        // -------- fs --------
        ("std.fs", "read" | "read_file" | "read_to_string") => fs_read(args),
        ("std.fs", "write" | "write_file" | "write_string") => fs_write(args),
        ("std.fs", "append") => fs_append(args),
        ("std.fs", "exists") => fs_exists(args),
        ("std.fs", "metadata" | "stat") => fs_metadata(args),
        ("std.fs", "create_dir_all") => fs_create_dir_all(args),
        ("std.fs", "remove_file") => fs_remove_file(args),
        ("std.fs", "remove_dir_all") => fs_remove_dir_all(args),
        ("std.fs", "list_dir" | "read_dir") => fs_list_dir(args),
        // -------- http (sync wrapper around tokio runtime) --------
        ("std.http", "get") => http_get(args),
        ("std.http", "post") => http_post(args),
        // v0.5 dogfood Gap-1: real socket-binding server.
        ("std.http", "serve") => http_serve(args),
        ("std.http", "shutdown") => http_shutdown(args),
        // -------- env (v0.27 Track E QoL #3) --------
        ("std.env", "args") => env_args(),
        _ => Value::Unit,
    }
}

// --- env ---

/// v0.27 Track E (QoL #3): expose the CLI's `mty run <path> -- <argv>`
/// trailing args to Mighty source. Returns a `Value::Array` of
/// `Value::Str`; empty when nothing was installed (library callers,
/// JIT path, wasm32-wasi). Indexing is "Mighty user args" — the leading
/// element is what came right after `--`, not the binary name.
fn env_args() -> Value {
    Value::Array(crate::env::args().into_iter().map(Value::Str).collect())
}

// --- json ---

fn json_parse(args: &[Value]) -> Value {
    let Some(Value::Str(s)) = args.first() else {
        return Value::Unit;
    };
    match crate::json::parse(s) {
        Ok(v) => match crate::json::encode(&v) {
            Ok(s) => Value::Str(s),
            Err(_) => Value::Unit,
        },
        Err(e) => Value::Str(format!("ERR:{e}")),
    }
}

fn json_encode(args: &[Value]) -> Value {
    // Accept a Str already containing JSON and reformat compactly.
    let Some(Value::Str(s)) = args.first() else {
        return Value::Unit;
    };
    match crate::json::parse(s) {
        Ok(v) => Value::Str(crate::json::encode(&v).unwrap_or_default()),
        Err(_) => Value::Str(String::new()),
    }
}

fn json_encode_pretty(args: &[Value]) -> Value {
    let Some(Value::Str(s)) = args.first() else {
        return Value::Unit;
    };
    match crate::json::parse(s) {
        Ok(v) => Value::Str(crate::json::encode_pretty(&v).unwrap_or_default()),
        Err(_) => Value::Str(String::new()),
    }
}

// --- time ---

fn time_now() -> Value {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0);
    Value::Int(secs, mty_types::IntKind::I64)
}

fn time_sleep(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let ms = match v {
            Value::Duration(n) => *n,
            Value::Int(n, _) => *n as u64,
            _ => 0,
        };
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    Value::Unit
}

// --- fs ---

fn fs_read(args: &[Value]) -> Value {
    // arg shape: (cap, path) — cap is opaque (Cap or Unit), path is Str.
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => return Value::Unit,
        },
    };
    // v0.5 Gap-5: consult the process-wide default read cap installed
    // from the sandbox manifest. Tests + the unsandboxed CLI default
    // to unrestricted (matches v0.4 behaviour).
    let cap = crate::fs::current_default_read_cap();
    match crate::fs::read(&cap, std::path::Path::new(&path)) {
        Ok(bytes) => Value::Str(String::from_utf8_lossy(&bytes).into_owned()),
        Err(crate::fs::IoErr::Forbidden(_) | crate::fs::IoErr::Denied(_)) => Value::Enum {
            adt: mty_types::AdtId(0),
            variant: 1,
            payload: vec![Value::Str(format!("forbidden: {}", path))],
        },
        Err(_) => Value::Unit,
    }
}

fn fs_write(args: &[Value]) -> Value {
    let (path, data) = match (args.get(1), args.get(2)) {
        (Some(Value::Str(p)), Some(Value::Str(d))) => (p.clone(), d.clone()),
        _ => match (args.first(), args.get(1)) {
            (Some(Value::Str(p)), Some(Value::Str(d))) => (p.clone(), d.clone()),
            _ => return Value::Unit,
        },
    };
    let cap = crate::fs::current_default_write_cap();
    match crate::fs::write(&cap, std::path::Path::new(&path), data.as_bytes()) {
        Ok(_) => Value::Unit,
        Err(crate::fs::IoErr::Forbidden(_) | crate::fs::IoErr::Denied(_)) => Value::Enum {
            adt: mty_types::AdtId(0),
            variant: 1,
            payload: vec![Value::Str(format!("forbidden: {}", path))],
        },
        Err(_) => Value::Unit,
    }
}

fn fs_exists(args: &[Value]) -> Value {
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => return Value::Bool(false),
        },
    };
    let cap = crate::fs::current_default_read_cap();
    Value::Bool(crate::fs::exists(&cap, std::path::Path::new(&path)))
}

/// v0.45 T1 — `std.fs.append(path, bytes)` dispatcher. Same arg
/// extraction pattern as [`fs_write`] (with or without leading cap).
/// Returns `Value::Unit` on success, an `Err(Str)` enum on capability
/// denial (mirrors the rest of the surface).
fn fs_append(args: &[Value]) -> Value {
    let (path, data) = match (args.get(1), args.get(2)) {
        (Some(Value::Str(p)), Some(Value::Str(d))) => (p.clone(), d.clone()),
        _ => match (args.first(), args.get(1)) {
            (Some(Value::Str(p)), Some(Value::Str(d))) => (p.clone(), d.clone()),
            _ => return Value::Unit,
        },
    };
    let cap = crate::fs::current_default_write_cap();
    match crate::fs::append(&cap, std::path::Path::new(&path), data.as_bytes()) {
        Ok(_) => Value::Unit,
        Err(crate::fs::IoErr::Forbidden(_) | crate::fs::IoErr::Denied(_)) => Value::Enum {
            adt: mty_types::AdtId(0),
            variant: 1,
            payload: vec![Value::Str(format!("forbidden: {}", path))],
        },
        Err(_) => Value::Unit,
    }
}

/// v0.45 T1 — `std.fs.metadata(path)` (also aliased to legacy
/// `std.fs.stat`). Returns a 4-field record-shaped Mighty value so
/// generated apps can pattern-match on `size`/`mtime_ms`/`is_file`/
/// `is_dir`. Under the interpreter we encode it as a tuple-ish
/// `Value::Array([size, mtime_ms, is_file, is_dir])` so the existing
/// place-projection lowering can index into it without a new ADT.
fn fs_metadata(args: &[Value]) -> Value {
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => return Value::Unit,
        },
    };
    let cap = crate::fs::current_default_read_cap();
    match crate::fs::metadata(&cap, std::path::Path::new(&path)) {
        Ok(md) => Value::Array(vec![
            Value::Int(md.size as i128, mty_types::IntKind::U64),
            Value::Int(md.mtime_ms as i128, mty_types::IntKind::I64),
            Value::Bool(md.is_file != 0),
            Value::Bool(md.is_dir != 0),
        ]),
        Err(_) => Value::Unit,
    }
}

/// v0.45 T1 — `std.fs.create_dir_all(path)`. Returns Unit on success
/// and a forbidden `Err(Str)` enum on cap denial; other IO errors
/// surface as Unit (matches the rest of the surface).
fn fs_create_dir_all(args: &[Value]) -> Value {
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => return Value::Unit,
        },
    };
    let cap = crate::fs::current_default_write_cap();
    match crate::fs::create_dir_all(&cap, std::path::Path::new(&path)) {
        Ok(_) => Value::Unit,
        Err(crate::fs::IoErr::Forbidden(_) | crate::fs::IoErr::Denied(_)) => Value::Enum {
            adt: mty_types::AdtId(0),
            variant: 1,
            payload: vec![Value::Str(format!("forbidden: {}", path))],
        },
        Err(_) => Value::Unit,
    }
}

/// v0.45 T1 — `std.fs.remove_file(path)`.
fn fs_remove_file(args: &[Value]) -> Value {
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => return Value::Unit,
        },
    };
    let cap = crate::fs::current_default_write_cap();
    match crate::fs::remove_file(&cap, std::path::Path::new(&path)) {
        Ok(_) => Value::Unit,
        Err(crate::fs::IoErr::Forbidden(_) | crate::fs::IoErr::Denied(_)) => Value::Enum {
            adt: mty_types::AdtId(0),
            variant: 1,
            payload: vec![Value::Str(format!("forbidden: {}", path))],
        },
        Err(_) => Value::Unit,
    }
}

/// v0.45 T1 — `std.fs.remove_dir_all(path)`.
fn fs_remove_dir_all(args: &[Value]) -> Value {
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => return Value::Unit,
        },
    };
    let cap = crate::fs::current_default_write_cap();
    match crate::fs::remove_dir_all(&cap, std::path::Path::new(&path)) {
        Ok(_) => Value::Unit,
        Err(crate::fs::IoErr::Forbidden(_) | crate::fs::IoErr::Denied(_)) => Value::Enum {
            adt: mty_types::AdtId(0),
            variant: 1,
            payload: vec![Value::Str(format!("forbidden: {}", path))],
        },
        Err(_) => Value::Unit,
    }
}

fn fs_list_dir(args: &[Value]) -> Value {
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => return Value::Array(vec![]),
        },
    };
    let cap = crate::fs::current_default_read_cap();
    match crate::fs::list_dir(&cap, std::path::Path::new(&path)) {
        Ok(entries) => Value::Array(
            entries
                .into_iter()
                .map(|p| Value::Str(p.display().to_string()))
                .collect(),
        ),
        Err(_) => Value::Array(vec![]),
    }
}

// ----- v0.5 Gap-1: real-socket HTTP server bridge ----------------------

/// v0.5 dogfood Gap-1 — bind a real HTTP listener on `addr`. Returns
/// a `Str` of the form `"<handle_id>|<bound_addr>"` so the calling
/// agent can keep a stable handle while also seeing the OS-assigned
/// port. Pass the leading `<handle_id>` to [`http_shutdown`] to stop
/// the server.
fn http_serve(args: &[Value]) -> Value {
    let addr = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        _ => return Value::Unit,
    };
    match crate::http_server::start_blocking(&addr) {
        Ok((handle_id, bound)) => Value::Str(format!("{}|{}", handle_id, bound)),
        Err(e) => Value::Str(format!("ERR:{}", e)),
    }
}

fn http_shutdown(args: &[Value]) -> Value {
    let handle_id = match args.first() {
        Some(Value::Str(s)) => s
            .split('|')
            .next()
            .and_then(|p| p.parse::<u64>().ok())
            .unwrap_or(0),
        Some(Value::Int(n, _)) => *n as u64,
        _ => return Value::Bool(false),
    };
    Value::Bool(crate::http_server::shutdown(handle_id))
}

// --- http ---

fn http_get(args: &[Value]) -> Value {
    let url = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        _ => return Value::Unit,
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let Ok(rt) = rt else {
        return Value::Unit;
    };
    match rt.block_on(crate::http::get(&url)) {
        Ok(resp) => Value::Str(resp.body_str().to_string()),
        Err(_) => Value::Unit,
    }
}

fn http_post(args: &[Value]) -> Value {
    let url = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        _ => return Value::Unit,
    };
    let body = match args.get(1) {
        Some(Value::Str(s)) => s.clone().into_bytes(),
        _ => Vec::new(),
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let Ok(rt) = rt else {
        return Value::Unit;
    };
    match rt.block_on(crate::http::post(&url, body)) {
        Ok(resp) => Value::Str(resp.body_str().to_string()),
        Err(_) => Value::Unit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_host_dispatch_accepts_agent_friendly_aliases() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("notes").join("one.txt");
        let path = path.display().to_string();

        let write = dispatch(
            &["std".into(), "fs".into()],
            "write_file",
            &[Value::Str(path.clone()), Value::Str("hello".into())],
        );
        assert!(matches!(write, Value::Unit));

        let read = dispatch(
            &["std".into(), "fs".into()],
            "read_to_string",
            &[Value::Str(path.clone())],
        );
        assert!(matches!(read, Value::Str(ref value) if value == "hello"));

        let exists = dispatch(
            &["std".into(), "fs".into()],
            "exists",
            &[Value::Str(path.clone())],
        );
        assert!(matches!(exists, Value::Bool(true)));

        let listing = dispatch(
            &["std".into(), "fs".into()],
            "read_dir",
            &[Value::Str(dir.path().join("notes").display().to_string())],
        );
        assert!(matches!(listing, Value::Array(entries) if entries.len() == 1));
    }

    /// v0.45 T1 — broader fs surface (append / metadata /
    /// create_dir_all / remove_file / remove_dir_all). Verifies the
    /// interpreter dispatcher accepts each method and the underlying
    /// stdlib impl actually mutates the filesystem. The cranelift
    /// JIT and AOT paths share the same `mty_stdlib::fs::*` helpers,
    /// so cross-backend behavior stays aligned.
    #[test]
    fn fs_host_dispatch_v045_t1_extended_surface() {
        let dir = tempfile::tempdir().expect("tempdir");

        // create_dir_all → mkdir -p
        let nested = dir.path().join("a/b/c");
        let r = dispatch(
            &["std".into(), "fs".into()],
            "create_dir_all",
            &[Value::Str(nested.display().to_string())],
        );
        assert!(matches!(r, Value::Unit));
        assert!(nested.exists());

        // append: creates and extends
        let log = dir.path().join("log.txt");
        let r = dispatch(
            &["std".into(), "fs".into()],
            "append",
            &[
                Value::Str(log.display().to_string()),
                Value::Str("one\n".into()),
            ],
        );
        assert!(matches!(r, Value::Unit));
        let r = dispatch(
            &["std".into(), "fs".into()],
            "append",
            &[
                Value::Str(log.display().to_string()),
                Value::Str("two\n".into()),
            ],
        );
        assert!(matches!(r, Value::Unit));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "one\ntwo\n");

        // metadata: 4-field array
        let md = dispatch(
            &["std".into(), "fs".into()],
            "metadata",
            &[Value::Str(log.display().to_string())],
        );
        match md {
            Value::Array(fields) => {
                assert_eq!(fields.len(), 4, "metadata returns 4 fields");
                // size is field 0; bytes wrote 8.
                if let Some(Value::Int(size, _)) = fields.first() {
                    assert_eq!(*size, 8);
                } else {
                    panic!("size field wrong shape: {:?}", fields.first());
                }
                // is_file is field 2.
                if let Some(Value::Bool(is_file)) = fields.get(2) {
                    assert!(*is_file);
                } else {
                    panic!("is_file field wrong shape");
                }
            }
            other => panic!("metadata should be Array, got {:?}", other),
        }

        // remove_file
        let r = dispatch(
            &["std".into(), "fs".into()],
            "remove_file",
            &[Value::Str(log.display().to_string())],
        );
        assert!(matches!(r, Value::Unit));
        assert!(!log.exists());

        // remove_dir_all (recursive)
        let r = dispatch(
            &["std".into(), "fs".into()],
            "remove_dir_all",
            &[Value::Str(dir.path().join("a").display().to_string())],
        );
        assert!(matches!(r, Value::Unit));
        assert!(!dir.path().join("a").exists());

        // stat is aliased to metadata.
        let nested = dir.path().join("present.txt");
        std::fs::write(&nested, b"xy").unwrap();
        let st = dispatch(
            &["std".into(), "fs".into()],
            "stat",
            &[Value::Str(nested.display().to_string())],
        );
        assert!(matches!(st, Value::Array(_)));
    }
}
