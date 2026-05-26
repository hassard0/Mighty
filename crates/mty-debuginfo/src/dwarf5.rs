//! DWARF v5 builder for native object files (post-v1.0 surface).
//!
//! Companion to [`crate::dwarf`], which targets DWARF v4. The v5 builder
//! has the same public shape (`Dwarf5Builder::new` → `init_compile_unit`
//! → `add_function`* → `finish`) so callers can dispatch between v4 and
//! v5 at one site.
//!
//! What DWARF v5 brings, and what this builder uses:
//!
//! - **`.debug_line_str`** — a separate string table used by the line
//!   program for directory and file names. v4 inlined those as
//!   null-terminated strings in `.debug_line`; v5's indirect form shares
//!   strings, shrinking the binary when many CUs share paths. We push
//!   all directory + file names through `dwarf.line_strings.add(..)`
//!   and emit `LineString::LineStringRef(..)`.
//! - **`.debug_str_offsets`** — indirect strings via the DWARF unit's
//!   `strings` table; emitted automatically by `gimli` for v5 units.
//! - **`.debug_loclists` / `.debug_rnglists`** — the v5 replacements for
//!   `.debug_loc` / `.debug_ranges`. `gimli`'s writer wires these up
//!   when the encoding version is 5; we don't yet attach loc-lists per
//!   local (same v0.3-era limitation as the v4 path), but the section
//!   bytes are still emitted when the dwarf unit references rnglists
//!   for cross-fn ranges.
//! - **Per-instruction line program** — v4 we typically emit one row
//!   per basic-block (or per fn entry, in the conservative path). v5
//!   we emit one row *per machine-instruction*, mapping each
//!   `(addr_offset, SourcePos)` entry in `FunctionDebugInfo::line_table`
//!   to its own row. Callers that want finer-grained line info just
//!   need to populate `line_table` with one entry per IR instruction;
//!   the v5 builder records every one of them.
//!
//! The v5 encoding produces a `.debug_line` section whose header begins
//! with the standard 4-byte init length (DWARF32) followed by the
//! version word `0x0005` — tooling (gdb ≥ 8.0, lldb ≥ 9, llvm-dwarfdump,
//! `objdump --dwarf=info`) all recognize the v5 magic.

use crate::dwarf::{DwarfSection, EncodedDwarf};
use crate::{DebugInfoError, DebugInfoResult, FunctionDebugInfo};
use gimli::write::{
    Address, AttributeValue, DwarfUnit, EndianVec, Expression, LineProgram, LineString, Location,
    LocationList, Sections, UnitEntryId,
};
use gimli::{Encoding, Format, LineEncoding, LittleEndian, Register};
use std::collections::HashMap;

/// Builder for a single compilation unit's DWARF v5 output.
///
/// Same flow as [`crate::dwarf::DwarfBuilder`] but targets the DWARF v5
/// encoding and emits per-instruction line records plus an indirect
/// (`.debug_line_str`) string table for directory/file names.
pub struct Dwarf5Builder {
    dwarf: DwarfUnit,
    base_types: HashMap<String, UnitEntryId>,
    producer: String,
    source_path: String,
    comp_dir: String,
    line_program_seeded: bool,
    primary_file: Option<gimli::write::FileId>,
    encoding: Encoding,
    /// Running count of line-program rows emitted across all functions.
    /// Used by the tests to assert per-instruction granularity.
    rows_emitted: usize,
    /// Running count of "sequence boundaries" — one per fn. Used to
    /// derive the per-block / per-instruction comparison in tests.
    sequences_emitted: usize,
    /// v0.21: running count of `DW_AT_location` loclist refs attached
    /// to local DIEs. Tests use this to verify per-local
    /// `.debug_loclists` emission scales with the number of locals.
    loclist_locals_emitted: usize,
}

impl Dwarf5Builder {
    /// Create a new v5 builder. `source_path` and `comp_dir` go into
    /// the compile-unit DIE and the line program's primary file entry.
    pub fn new(source_path: impl Into<String>, comp_dir: impl Into<String>) -> Self {
        let encoding = Encoding {
            format: Format::Dwarf32,
            version: 5,
            address_size: 8,
        };
        let dwarf = DwarfUnit::new(encoding);
        Self {
            dwarf,
            base_types: HashMap::new(),
            producer: "mighty-0.20-dwarf5".to_string(),
            source_path: source_path.into(),
            comp_dir: comp_dir.into(),
            line_program_seeded: false,
            primary_file: None,
            encoding,
            rows_emitted: 0,
            sequences_emitted: 0,
            loclist_locals_emitted: 0,
        }
    }

    /// Initialize the compile-unit DIE and line program.
    ///
    /// In v5, file index 0 *must* exist and corresponds to the
    /// compilation unit's primary source file — `LineProgram::new`
    /// inserts it automatically for v5 encodings. We also use the
    /// indirect (`LineString::LineStringRef`) form for the comp_dir /
    /// comp_file values so they live in `.debug_line_str` rather than
    /// being inlined into `.debug_line` itself.
    pub fn init_compile_unit(&mut self) -> DebugInfoResult<()> {
        let basename = file_basename(&self.source_path);
        // Push the dir + filename into the line-str table and use the
        // indirect-form for the primary entry. This is the v5 win:
        // string sharing.
        let comp_dir_id = self
            .dwarf
            .line_strings
            .add(self.comp_dir.clone().into_bytes());
        let comp_file_id = self.dwarf.line_strings.add(basename.clone().into_bytes());

        let mut line_program = LineProgram::new(
            self.encoding,
            LineEncoding::default(),
            LineString::LineStringRef(comp_dir_id),
            LineString::LineStringRef(comp_file_id),
            None,
        );
        // In v5, `LineProgram::new` already populated file index 0
        // with the comp file. `add_file` is idempotent on
        // `(file, directory)`, so calling it again with the same key
        // returns the same FileId — that's the only public way to get
        // a handle to the comp-file id since `FileId::new` is crate-private.
        let dir_id = line_program.default_directory();
        let file_id = line_program.add_file(LineString::LineStringRef(comp_file_id), dir_id, None);
        self.primary_file = Some(file_id);

        self.dwarf.unit.line_program = line_program;
        self.line_program_seeded = true;

        // Compile-unit DIE. Use `.debug_str` (indirect) for producer +
        // name + comp_dir; v5 will encode these via DW_FORM_strx /
        // DW_FORM_strp depending on gimli's chosen form. The DwarfUnit
        // writer drives the str-offsets table automatically.
        let root = self.dwarf.unit.root();
        let producer_id = self.dwarf.strings.add(self.producer.clone().into_bytes());
        let name_id = self
            .dwarf
            .strings
            .add(self.source_path.clone().into_bytes());
        let dir_id_str = self.dwarf.strings.add(self.comp_dir.clone().into_bytes());
        let root_die = self.dwarf.unit.get_mut(root);
        root_die.set(
            gimli::DW_AT_producer,
            AttributeValue::StringRef(producer_id),
        );
        root_die.set(
            gimli::DW_AT_language,
            AttributeValue::Language(gimli::DW_LANG_Rust),
        );
        root_die.set(gimli::DW_AT_name, AttributeValue::StringRef(name_id));
        root_die.set(gimli::DW_AT_comp_dir, AttributeValue::StringRef(dir_id_str));
        root_die.set(
            gimli::DW_AT_low_pc,
            AttributeValue::Address(Address::Constant(0)),
        );
        Ok(())
    }

    /// Set the compile-unit's total code-range high_pc.
    pub fn set_total_code_size(&mut self, size: u64) {
        let root = self.dwarf.unit.root();
        let root_die = self.dwarf.unit.get_mut(root);
        root_die.set(gimli::DW_AT_high_pc, AttributeValue::Udata(size));
    }

    fn intern_base_type(&mut self, name: &str) -> UnitEntryId {
        if let Some(&id) = self.base_types.get(name) {
            return id;
        }
        let root = self.dwarf.unit.root();
        let id = self.dwarf.unit.add(root, gimli::DW_TAG_base_type);
        let name_str = self.dwarf.strings.add(name.as_bytes().to_vec());
        let (encoding, size) = guess_base_type(name);
        let die = self.dwarf.unit.get_mut(id);
        die.set(gimli::DW_AT_name, AttributeValue::StringRef(name_str));
        die.set(gimli::DW_AT_encoding, AttributeValue::Encoding(encoding));
        die.set(gimli::DW_AT_byte_size, AttributeValue::Data1(size));
        self.base_types.insert(name.to_string(), id);
        id
    }

    /// Add a `DW_TAG_subprogram` DIE plus *one line-program row per
    /// entry in `fn_info.line_table`*. Callers should populate
    /// `line_table` with one entry per MtyIR instruction address; the
    /// v5 builder records every one as a separate row.
    pub fn add_function(&mut self, fn_info: &FunctionDebugInfo) -> DebugInfoResult<()> {
        if !self.line_program_seeded {
            self.init_compile_unit()?;
        }
        let root = self.dwarf.unit.root();
        let return_type_id = self.intern_base_type(&fn_info.return_type);
        let sub = self.dwarf.unit.add(root, gimli::DW_TAG_subprogram);
        let name_str = self.dwarf.strings.add(fn_info.name.clone().into_bytes());

        let (low, high) = fn_info.code_range;
        if high < low {
            return Err(DebugInfoError::DwarfWrite(format!(
                "fn {} has high_pc({high}) < low_pc({low})",
                fn_info.name
            )));
        }
        let size = high - low;
        let die = self.dwarf.unit.get_mut(sub);
        die.set(gimli::DW_AT_name, AttributeValue::StringRef(name_str));
        die.set(gimli::DW_AT_external, AttributeValue::Flag(true));
        die.set(
            gimli::DW_AT_low_pc,
            AttributeValue::Address(Address::Constant(low)),
        );
        die.set(gimli::DW_AT_high_pc, AttributeValue::Udata(size));
        die.set(
            gimli::DW_AT_decl_file,
            AttributeValue::FileIndex(self.primary_file),
        );
        die.set(
            gimli::DW_AT_decl_line,
            AttributeValue::Udata(fn_info.decl_pos.line as u64),
        );
        die.set(
            gimli::DW_AT_decl_column,
            AttributeValue::Udata(fn_info.decl_pos.column as u64),
        );
        die.set(gimli::DW_AT_type, AttributeValue::UnitRef(return_type_id));

        // Locals. v0.21 path: if `rich_locals` is populated, emit a
        // real `.debug_loclists` entry per local with
        // `DW_OP_breg7 + slot_offset` (x86_64 RSP-relative).
        // Otherwise fall back to the v0.2-style frame-offset attribute
        // on the legacy `locals` vector.
        if !fn_info.rich_locals.is_empty() {
            for local in &fn_info.rich_locals {
                let ty_id = self.intern_base_type(&local.type_tag);
                let v = self.dwarf.unit.add(sub, gimli::DW_TAG_variable);
                let n = self.dwarf.strings.add(local.name.clone().into_bytes());
                let (lo, hi) = local.address_range;
                if hi > lo {
                    let mut expr = Expression::new();
                    // x86_64 DWARF register numbering: 7 = RSP. v0.21
                    // hardcodes this — the cranelift backend is x86_64
                    // host-only today; v0.22 widens this once aarch64
                    // codegen lands.
                    expr.op_breg(Register(7), local.slot as i64);
                    let loclist = LocationList(vec![Location::OffsetPair {
                        begin: lo,
                        end: hi,
                        data: expr,
                    }]);
                    let loc_id = self.dwarf.unit.locations.add(loclist);
                    let v_die = self.dwarf.unit.get_mut(v);
                    v_die.set(gimli::DW_AT_name, AttributeValue::StringRef(n));
                    v_die.set(gimli::DW_AT_type, AttributeValue::UnitRef(ty_id));
                    v_die.set(
                        gimli::DW_AT_location,
                        AttributeValue::LocationListRef(loc_id),
                    );
                    self.loclist_locals_emitted += 1;
                } else {
                    // Empty range — fall back to a constant slot
                    // attribute. Keeps the DIE structurally valid.
                    let v_die = self.dwarf.unit.get_mut(v);
                    v_die.set(gimli::DW_AT_name, AttributeValue::StringRef(n));
                    v_die.set(gimli::DW_AT_type, AttributeValue::UnitRef(ty_id));
                    v_die.set(
                        gimli::DW_AT_data_member_location,
                        AttributeValue::Sdata(local.slot as i64),
                    );
                }
            }
        } else {
            for var in &fn_info.locals {
                let ty_id = self.intern_base_type(&var.type_name);
                let v = self.dwarf.unit.add(sub, gimli::DW_TAG_variable);
                let n = self.dwarf.strings.add(var.name.clone().into_bytes());
                let v_die = self.dwarf.unit.get_mut(v);
                v_die.set(gimli::DW_AT_name, AttributeValue::StringRef(n));
                v_die.set(gimli::DW_AT_type, AttributeValue::UnitRef(ty_id));
                if let Some(off) = var.frame_offset {
                    v_die.set(
                        gimli::DW_AT_data_member_location,
                        AttributeValue::Sdata(off as i64),
                    );
                }
            }
        }

        // Per-instruction line program. v0.21: prefer
        // `rich_line_table` (with `is_stmt`/`end_sequence` flags)
        // when populated; otherwise fall back to the v0.20 path that
        // marks every row as is_stmt and synthesizes `end_sequence`
        // after the loop.
        let file = self.primary_file.expect("primary file initialized");
        let lp = &mut self.dwarf.unit.line_program;
        lp.begin_sequence(Some(Address::Constant(low)));
        let mut last_off: u64 = 0;
        let mut explicit_end_sequence = false;
        if !fn_info.rich_line_table.is_empty() {
            for r in &fn_info.rich_line_table {
                if r.address_offset < last_off {
                    continue;
                }
                last_off = r.address_offset;
                let row = lp.row();
                row.address_offset = r.address_offset;
                row.file = file;
                row.line = r.line as u64;
                row.column = r.column as u64;
                row.is_statement = r.is_stmt;
                lp.generate_row();
                self.rows_emitted += 1;
                if r.end_sequence {
                    explicit_end_sequence = true;
                    break;
                }
            }
        } else {
            for (addr_off, pos) in &fn_info.line_table {
                if *addr_off < last_off {
                    continue;
                }
                last_off = *addr_off;
                let row = lp.row();
                row.address_offset = *addr_off;
                row.file = file;
                row.line = pos.line as u64;
                row.column = pos.column as u64;
                row.is_statement = true;
                lp.generate_row();
                self.rows_emitted += 1;
            }
        }
        if explicit_end_sequence {
            // The last rich row carried end_sequence — use its address
            // offset as the sequence terminator.
            lp.end_sequence(last_off);
        } else {
            lp.end_sequence(size);
        }
        self.sequences_emitted += 1;
        Ok(())
    }

    /// Number of `.debug_line` rows emitted so far. Useful for callers
    /// (and tests) to verify per-instruction granularity beats
    /// per-basic-block granularity.
    pub fn rows_emitted(&self) -> usize {
        self.rows_emitted
    }

    /// Number of line-program sequences emitted (= number of functions).
    pub fn sequences_emitted(&self) -> usize {
        self.sequences_emitted
    }

    /// v0.21: count of locals for which a real `.debug_loclists`
    /// entry was emitted (i.e. `rich_locals` carried a non-empty
    /// `address_range`). Tests use this to verify the loclist plumbing
    /// fires whenever cranelift slot offsets are available.
    pub fn loclist_locals_emitted(&self) -> usize {
        self.loclist_locals_emitted
    }

    /// Consume the builder and produce encoded section buffers,
    /// including the v5-specific `.debug_line_str` indirect-string
    /// table when non-empty.
    pub fn finish(mut self) -> DebugInfoResult<EncodedDwarf> {
        let mut sections = Sections::new(EndianVec::new(LittleEndian));
        self.dwarf
            .write(&mut sections)
            .map_err(|e| DebugInfoError::DwarfWrite(format!("{e:?}")))?;
        let mut out = EncodedDwarf::default();
        sections
            .for_each(|id, w| -> Result<(), DebugInfoError> {
                let bytes = w.slice().to_vec();
                if bytes.is_empty() {
                    return Ok(());
                }
                out.sections.push(DwarfSection {
                    name: id.name().to_string(),
                    bytes,
                });
                Ok(())
            })
            .map_err(|e| DebugInfoError::DwarfWrite(format!("{e:?}")))?;
        Ok(out)
    }
}

/// File basename (matches `crate::dwarf::file_basename`'s behavior on
/// both Windows and POSIX path forms).
fn file_basename(path: &str) -> String {
    let mut last = path;
    if let Some(idx) = path.rfind(['/', '\\']) {
        last = &path[idx + 1..];
    }
    if last.is_empty() {
        path.to_string()
    } else {
        last.to_string()
    }
}

/// Same conservative base-type mapping as the v4 path.
fn guess_base_type(name: &str) -> (gimli::DwAte, u8) {
    match name {
        "bool" => (gimli::DW_ATE_boolean, 1),
        "char" => (gimli::DW_ATE_UTF, 4),
        "i8" => (gimli::DW_ATE_signed, 1),
        "u8" => (gimli::DW_ATE_unsigned, 1),
        "i16" => (gimli::DW_ATE_signed, 2),
        "u16" => (gimli::DW_ATE_unsigned, 2),
        "i32" => (gimli::DW_ATE_signed, 4),
        "u32" => (gimli::DW_ATE_unsigned, 4),
        "i64" | "isize" | "duration" | "size" => (gimli::DW_ATE_signed, 8),
        "u64" | "usize" => (gimli::DW_ATE_unsigned, 8),
        "f32" => (gimli::DW_ATE_float, 4),
        "f64" => (gimli::DW_ATE_float, 8),
        "()" | "unit" => (gimli::DW_ATE_unsigned, 0),
        _ => (gimli::DW_ATE_address, 8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourcePos, VarDebugInfo};

    fn sample_fn() -> FunctionDebugInfo {
        FunctionDebugInfo {
            name: "main".into(),
            mangled_name: None,
            return_type: "i32".into(),
            decl_pos: SourcePos::new(0, 1, 1),
            code_range: (0, 32),
            // 5 per-instruction entries inside a single fn — more than
            // a v4 per-basic-block emitter would produce (which would
            // typically be 1–2 for a tiny fn).
            line_table: vec![
                (0, SourcePos::new(0, 1, 1)),
                (4, SourcePos::new(10, 2, 3)),
                (8, SourcePos::new(15, 2, 6)),
                (12, SourcePos::new(20, 3, 1)),
                (16, SourcePos::new(25, 3, 4)),
            ],
            locals: vec![VarDebugInfo {
                name: "x".into(),
                type_name: "i32".into(),
                frame_offset: Some(-8),
            }],
            rich_line_table: Vec::new(),
            rich_locals: Vec::new(),
        }
    }

    #[test]
    fn build_produces_v5_sections() {
        let mut b = Dwarf5Builder::new("test.mty", "/tmp");
        b.init_compile_unit().unwrap();
        b.add_function(&sample_fn()).unwrap();
        b.set_total_code_size(32);
        let enc = b.finish().unwrap();
        let names: Vec<_> = enc.sections.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&".debug_info"));
        assert!(names.contains(&".debug_abbrev"));
        assert!(names.contains(&".debug_line"));
        // v5-specific: indirect line-str table must be present because
        // we used LineString::LineStringRef for comp_dir + comp_file.
        assert!(names.contains(&".debug_line_str"));
    }

    #[test]
    fn drops_out_of_order_rows() {
        let mut b = Dwarf5Builder::new("t.mty", "/tmp");
        b.init_compile_unit().unwrap();
        let mut f = sample_fn();
        // Inject an out-of-order entry in the middle. The builder must
        // silently skip it rather than panic (gimli's debug_assert on
        // monotonic address_offsets would otherwise blow up).
        f.line_table.insert(2, (2, SourcePos::new(99, 9, 9)));
        b.add_function(&f).unwrap();
        b.set_total_code_size(32);
        let enc = b.finish().unwrap();
        assert!(enc.sections.iter().any(|s| s.name == ".debug_line"));
    }
}
