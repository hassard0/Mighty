//! `extern { fn ... }` symbol resolution via libloading (slice 8, A53).
//!
//! Slice-8 supports a minimal model: libc-resident C fns can be called
//! by name from Mighty. Per-extern overrides specified in
//! `star.toml`'s `[extern]` table can target other shared libraries.
//!
//! The registry is built once, lazily loads libraries as needed, and
//! caches resolved fn-pointers. Unresolved names trap with SD8005 at
//! the call site.

use libloading::{Library, Symbol};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ExternRegistry {
    libs: Vec<Arc<Library>>,
    /// name → (lib index, raw fn-ptr). The fn-ptr is opaque until the
    /// caller transmutes it to the right signature.
    cache: HashMap<String, *const ()>,
    overrides: HashMap<String, String>,
}

// Library handles aren't Send by default; we wrap in Arc and protect
// concurrent access via Mutex at the call site, so this is fine.
unsafe impl Send for ExternRegistry {}
unsafe impl Sync for ExternRegistry {}

impl ExternRegistry {
    pub fn new() -> Self {
        Self {
            libs: Vec::new(),
            cache: HashMap::new(),
            overrides: HashMap::new(),
        }
    }

    /// Build a registry with libc preloaded. Default for slice 8.
    pub fn with_libc() -> Self {
        let mut r = Self::new();
        if let Some(lib) = open_libc() {
            r.libs.push(Arc::new(lib));
        }
        r
    }

    /// Insert a `name = lib` override before any lookups happen.
    pub fn add_override(&mut self, name: impl Into<String>, lib: impl Into<String>) {
        self.overrides.insert(name.into(), lib.into());
    }

    /// Look up a fn by name. Returns a raw fn-ptr or `None`.
    pub fn resolve(&mut self, name: &str) -> Option<*const ()> {
        if let Some(&p) = self.cache.get(name) {
            return Some(p);
        }
        // Per-name override?
        if let Some(libname) = self.overrides.get(name).cloned() {
            if let Some(lib) = unsafe { Library::new(&libname).ok() } {
                let arc = Arc::new(lib);
                if let Some(p) = sym_in(&arc, name) {
                    self.libs.push(arc);
                    self.cache.insert(name.to_string(), p);
                    return Some(p);
                }
            }
        }
        // Otherwise scan loaded libs.
        for lib in &self.libs {
            if let Some(p) = sym_in(lib, name) {
                self.cache.insert(name.to_string(), p);
                return Some(p);
            }
        }
        None
    }

    /// Convenience: call a no-arg, i64-returning extern by name. Used
    /// by the slice-8 simplified codegen bridge.
    pub fn call_i64(&self, name: &str) -> Option<i64> {
        let p = self.cache.get(name).copied()?;
        let f: extern "C" fn() -> i64 = unsafe { std::mem::transmute(p) };
        Some(f())
    }
}

impl Default for ExternRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn sym_in(lib: &Library, name: &str) -> Option<*const ()> {
    let cstr = std::ffi::CString::new(name).ok()?;
    let sym: Result<Symbol<unsafe extern "C" fn()>, _> =
        unsafe { lib.get(cstr.as_bytes_with_nul()) };
    sym.ok()
        .map(|s| unsafe { std::mem::transmute(s.into_raw().into_raw()) })
}

#[cfg(target_os = "linux")]
fn open_libc() -> Option<Library> {
    unsafe {
        Library::new("libc.so.6")
            .or_else(|_| Library::new("libc.so"))
            .ok()
    }
}

#[cfg(target_os = "macos")]
fn open_libc() -> Option<Library> {
    unsafe { Library::new("libSystem.dylib").ok() }
}

#[cfg(target_os = "windows")]
fn open_libc() -> Option<Library> {
    unsafe {
        Library::new("msvcrt.dll")
            .or_else(|_| Library::new("ucrtbase.dll"))
            .ok()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn open_libc() -> Option<Library> {
    None
}

pub type SharedRegistry = Arc<Mutex<ExternRegistry>>;

pub fn shared() -> SharedRegistry {
    Arc::new(Mutex::new(ExternRegistry::with_libc()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_resolves_nothing() {
        let mut r = ExternRegistry::new();
        assert!(r.resolve("nonexistent_fn_xyz").is_none());
    }

    #[test]
    fn libc_open_does_not_panic() {
        // Some CI hosts may not have libc as expected; this just
        // exercises the path.
        let _ = ExternRegistry::with_libc();
    }

    #[test]
    fn override_stored() {
        let mut r = ExternRegistry::new();
        r.add_override("foo", "libfoo.so");
        assert_eq!(
            r.overrides.get("foo").map(|s| s.as_str()),
            Some("libfoo.so")
        );
    }
}
