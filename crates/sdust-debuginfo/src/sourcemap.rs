//! Wasm source-map v3 + `name` custom section emitters.
//!
//! The wasm name custom section is a wasm-spec convention listing
//! function names so DevTools and `wasm-objdump` show readable
//! identifiers instead of `wasm-function[N]`. We emit only the function
//! subsection (subsection id 1) per the wasm spec.
//!
//! The source map is a sidecar JSON file in source-map v3 format. Per
//! the [Web Assembly source map proposal][1] the wasm module declares a
//! `sourceMappingURL` custom section pointing at the sidecar URL, and
//! DevTools / Chrome load it transparently.
//!
//! [1]: https://github.com/WebAssembly/tool-conventions/blob/main/Debugging.md
//!
//! v0.2 scope:
//! - One mapping per wasm byte offset for which we have a source pos.
//! - Function names only (no local names — that's deferred to v0.3).
//! - VLQ encoded mappings; column always 0 (wasm has no source column).

use crate::{DebugInfoError, DebugInfoResult, SourcePos};

/// One source-map mapping entry: wasm byte offset → source position.
#[derive(Debug, Clone, Copy)]
pub struct SourceMapMapping {
    pub generated_offset: u32,
    pub source_index: u32,
    pub source_line: u32,
    pub source_column: u32,
}

impl SourceMapMapping {
    pub fn from_pos(generated_offset: u32, source_index: u32, pos: SourcePos) -> Self {
        Self {
            generated_offset,
            source_index,
            // Source-map v3 lines are 0-based; our SourcePos is 1-based.
            source_line: pos.line.saturating_sub(1),
            source_column: pos.column.saturating_sub(1),
        }
    }
}

/// A source-map v3 builder.
///
/// "Sources" are the .sd files that contributed to the wasm output;
/// for slice-8 there's always exactly one. "Names" are optional symbol
/// names that appear in mappings (function names, local names); we
/// don't use them in v0.2 mappings, so the names array is empty.
#[derive(Debug, Default)]
pub struct SourceMap {
    pub file: Option<String>,
    pub source_root: Option<String>,
    pub sources: Vec<String>,
    pub sources_content: Vec<Option<String>>,
    pub names: Vec<String>,
    pub mappings: Vec<SourceMapMapping>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a source file. Returns its index for use in mappings.
    pub fn add_source(&mut self, path: impl Into<String>, content: Option<String>) -> u32 {
        let i = self.sources.len() as u32;
        self.sources.push(path.into());
        self.sources_content.push(content);
        i
    }

    pub fn add_mapping(&mut self, m: SourceMapMapping) {
        self.mappings.push(m);
    }

    /// Encode the source-map as JSON bytes (suitable for writing
    /// directly to a `<file>.wasm.map` sidecar).
    pub fn to_json(&self) -> DebugInfoResult<Vec<u8>> {
        let mappings_str = encode_mappings(&self.mappings);
        let mut obj = serde_json::Map::new();
        obj.insert("version".into(), serde_json::Value::from(3));
        if let Some(f) = &self.file {
            obj.insert("file".into(), serde_json::Value::from(f.clone()));
        }
        if let Some(r) = &self.source_root {
            obj.insert("sourceRoot".into(), serde_json::Value::from(r.clone()));
        }
        obj.insert(
            "sources".into(),
            serde_json::Value::from(self.sources.clone()),
        );
        // Only emit sourcesContent if any are populated.
        if self.sources_content.iter().any(|c| c.is_some()) {
            let arr: Vec<serde_json::Value> = self
                .sources_content
                .iter()
                .map(|c| match c {
                    Some(s) => serde_json::Value::from(s.clone()),
                    None => serde_json::Value::Null,
                })
                .collect();
            obj.insert("sourcesContent".into(), serde_json::Value::from(arr));
        }
        obj.insert("names".into(), serde_json::Value::from(self.names.clone()));
        obj.insert("mappings".into(), serde_json::Value::from(mappings_str));
        serde_json::to_vec(&serde_json::Value::Object(obj))
            .map_err(|e| DebugInfoError::SourceMap(format!("json: {e}")))
    }
}

/// Encode mappings into the source-map v3 mappings string.
///
/// Source map v3 groups mappings by generated line (semicolons
/// separate lines, commas separate segments within a line). For a
/// wasm "file" there is conceptually one generated line: byte offsets
/// are columns within that line. So we emit one ;-separated section
/// followed by all comma-separated segments inside it.
fn encode_mappings(mappings: &[SourceMapMapping]) -> String {
    // Each segment is a 4-tuple (or 5 if we used names):
    //   [generated_column, source_index, source_line, source_column]
    // All values are *relative* to the previous segment's value (VLQ
    // base64).
    let mut out = String::new();
    let mut prev = (0i64, 0i64, 0i64, 0i64);
    let mut sorted = mappings.to_vec();
    sorted.sort_by_key(|m| m.generated_offset);
    for (i, m) in sorted.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let gcol = m.generated_offset as i64;
        let sidx = m.source_index as i64;
        let sline = m.source_line as i64;
        let scol = m.source_column as i64;
        encode_vlq(&mut out, gcol - prev.0);
        encode_vlq(&mut out, sidx - prev.1);
        encode_vlq(&mut out, sline - prev.2);
        encode_vlq(&mut out, scol - prev.3);
        prev = (gcol, sidx, sline, scol);
    }
    out
}

/// VLQ base64 encoding per the source-map v3 spec.
fn encode_vlq(out: &mut String, value: i64) {
    // Convert to unsigned with sign in LSB.
    let mut vlq: u64 = if value < 0 {
        (((-value) as u64) << 1) | 1
    } else {
        (value as u64) << 1
    };
    loop {
        let mut digit = (vlq & 0b11111) as u8;
        vlq >>= 5;
        if vlq != 0 {
            digit |= 0b100000;
        }
        out.push(BASE64_TABLE[digit as usize] as char);
        if vlq == 0 {
            break;
        }
    }
}

const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A function-name entry for the wasm `name` custom section.
#[derive(Debug, Clone)]
pub struct WasmFnName {
    pub index: u32,
    pub name: String,
}

/// Builder for the wasm `name` custom section (function subsection only
/// in v0.2). Per the wasm spec, the name section is a custom section
/// named "name" containing length-prefixed subsections; the function
/// subsection (id 1) is a vector of (fn_idx, name) pairs.
#[derive(Debug, Default)]
pub struct NameSection {
    pub module_name: Option<String>,
    pub functions: Vec<WasmFnName>,
}

impl NameSection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Encode the section payload bytes (without the surrounding
    /// custom-section header). Callers prepend the section-id byte (0)
    /// + section length + name length + "name".
    pub fn encode_payload(&self) -> Vec<u8> {
        let mut out = Vec::new();
        // Subsection 0: module name (optional).
        if let Some(m) = &self.module_name {
            let mut sub = Vec::new();
            write_name(&mut sub, m);
            out.push(0u8);
            write_uleb128(&mut out, sub.len() as u64);
            out.extend_from_slice(&sub);
        }
        // Subsection 1: function names.
        if !self.functions.is_empty() {
            let mut sub = Vec::new();
            // Sort by index to match wasm spec (vec is in fn-index order).
            let mut fns = self.functions.clone();
            fns.sort_by_key(|f| f.index);
            write_uleb128(&mut sub, fns.len() as u64);
            for f in &fns {
                write_uleb128(&mut sub, f.index as u64);
                write_name(&mut sub, &f.name);
            }
            out.push(1u8);
            write_uleb128(&mut out, sub.len() as u64);
            out.extend_from_slice(&sub);
        }
        out
    }

    /// Encode the full custom-section bytes (id + length + name + payload).
    /// Suitable for appending to a wasm module.
    pub fn encode_full_section(&self) -> Vec<u8> {
        let payload = self.encode_payload();
        let mut section_body = Vec::new();
        write_name(&mut section_body, "name");
        section_body.extend_from_slice(&payload);
        let mut full = Vec::new();
        full.push(0u8); // custom section id
        write_uleb128(&mut full, section_body.len() as u64);
        full.extend_from_slice(&section_body);
        full
    }
}

/// Encode a sourceMappingURL custom section pointing at the sidecar.
/// Per the wasm-tool-conventions debugging spec, the URL points at the
/// sidecar file. Appended to a wasm module so DevTools can discover it.
pub fn source_mapping_url_section(url: &str) -> Vec<u8> {
    let mut body = Vec::new();
    write_name(&mut body, "sourceMappingURL");
    write_name(&mut body, url);
    let mut full = Vec::new();
    full.push(0u8); // custom section id
    write_uleb128(&mut full, body.len() as u64);
    full.extend_from_slice(&body);
    full
}

fn write_uleb128(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn write_name(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    write_uleb128(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_roundtrip_examples() {
        // From the source-map v3 spec examples.
        let mut s = String::new();
        encode_vlq(&mut s, 0);
        assert_eq!(s, "A");
        s.clear();
        encode_vlq(&mut s, 1);
        assert_eq!(s, "C");
        s.clear();
        encode_vlq(&mut s, -1);
        assert_eq!(s, "D");
        s.clear();
        encode_vlq(&mut s, 16);
        assert_eq!(s, "gB");
    }

    #[test]
    fn sourcemap_roundtrips_via_serde() {
        let mut sm = SourceMap::new();
        sm.file = Some("hello.wasm".into());
        let src = sm.add_source("hello.sd", Some("fn main() {}\n".into()));
        sm.add_mapping(SourceMapMapping::from_pos(0, src, SourcePos::new(0, 1, 1)));
        sm.add_mapping(SourceMapMapping::from_pos(5, src, SourcePos::new(13, 2, 3)));
        let bytes = sm.to_json().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["version"], 3);
        assert_eq!(v["sources"][0], "hello.sd");
        assert!(!v["mappings"].as_str().unwrap().is_empty());
        assert_eq!(v["file"], "hello.wasm");
    }

    #[test]
    fn empty_sourcemap_still_valid_json() {
        let sm = SourceMap::new();
        let bytes = sm.to_json().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["version"], 3);
    }

    #[test]
    fn name_section_encodes_functions() {
        let mut ns = NameSection::new();
        ns.functions.push(WasmFnName {
            index: 0,
            name: "log".into(),
        });
        ns.functions.push(WasmFnName {
            index: 1,
            name: "main".into(),
        });
        let payload = ns.encode_payload();
        // Subsection id 1 (functions) should appear.
        assert!(payload.contains(&1u8));
        let full = ns.encode_full_section();
        // Custom section id is 0.
        assert_eq!(full[0], 0);
        // The "name" string should appear in the bytes.
        let has_name = full.windows(4).any(|w| w == b"name");
        assert!(has_name);
        // Function names should appear.
        let has_main = full.windows(4).any(|w| w == b"main");
        assert!(has_main);
    }

    #[test]
    fn name_section_empty_yields_zero_size_payload() {
        let ns = NameSection::new();
        let p = ns.encode_payload();
        assert!(p.is_empty());
    }

    #[test]
    fn source_mapping_url_section_includes_url() {
        let bytes = source_mapping_url_section("hello.wasm.map");
        assert_eq!(bytes[0], 0); // custom section
        let has = bytes
            .windows(b"hello.wasm.map".len())
            .any(|w| w == b"hello.wasm.map");
        assert!(has);
    }
}
