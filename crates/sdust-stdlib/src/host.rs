//! `std.*` dispatcher invoked by `sdust-runtime::host_std`.
//!
//! The runtime keeps a process-wide `DispatcherFn` slot; calling
//! [`install`] from this crate (or from a downstream test) plugs
//! [`dispatch`] in so `EffectOp::GenericCall` paths reach the real
//! impls. We pattern-match on `(module path, method)` and run the real
//! implementation, returning a Stardust-shaped `Value`.
//!
//! Calls we don't yet handle return `Value::Unit` so existing tests that
//! exercise other effects keep passing.

use sdust_sir::interp::value::Value;

/// Register this crate as the runtime's `std.*` dispatcher. Idempotent
/// — safe to call from every test or once at driver start. Once the
/// driver wires this in v0.3, user code that does `use std.json` and
/// calls `json.parse(...)` through `sdust run` will hit the real
/// parser instead of the slice-7 no-op.
pub fn install() {
    sdust_runtime::host_std::install_dispatcher(dispatch);
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
        ("std.fs", "read") => fs_read(args),
        ("std.fs", "write") => fs_write(args),
        ("std.fs", "exists") => fs_exists(args),
        ("std.fs", "list_dir") => fs_list_dir(args),
        // -------- http (sync wrapper around tokio runtime) --------
        ("std.http", "get") => http_get(args),
        ("std.http", "post") => http_post(args),
        _ => Value::Unit,
    }
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
    Value::Int(secs, sdust_types::IntKind::I64)
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
    let cap = crate::fs::FsCap::unrestricted();
    match crate::fs::read(&cap, std::path::Path::new(&path)) {
        Ok(bytes) => Value::Str(String::from_utf8_lossy(&bytes).into_owned()),
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
    let cap = crate::fs::FsCap::unrestricted();
    let _ = crate::fs::write(&cap, std::path::Path::new(&path), data.as_bytes());
    Value::Unit
}

fn fs_exists(args: &[Value]) -> Value {
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => return Value::Bool(false),
        },
    };
    let cap = crate::fs::FsCap::unrestricted();
    Value::Bool(crate::fs::exists(&cap, std::path::Path::new(&path)))
}

fn fs_list_dir(args: &[Value]) -> Value {
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => return Value::Array(vec![]),
        },
    };
    let cap = crate::fs::FsCap::unrestricted();
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

// --- http ---

fn http_get(args: &[Value]) -> Value {
    let url = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        _ => return Value::Unit,
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let rt = match rt {
        Ok(rt) => rt,
        Err(_) => return Value::Unit,
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
    let rt = match rt {
        Ok(rt) => rt,
        Err(_) => return Value::Unit,
    };
    match rt.block_on(crate::http::post(&url, body)) {
        Ok(resp) => Value::Str(resp.body_str().to_string()),
        Err(_) => Value::Unit,
    }
}
