//! DWARF v4 builder for native object files.
//!
//! `DwarfBuilder` wraps gimli's write API and produces, for a single
//! compilation unit, the byte-buffers for the standard debug sections:
//!
//! - `.debug_abbrev`
//! - `.debug_info`
//! - `.debug_line`
//! - `.debug_str`
//! - `.debug_ranges`
//!
//! v0.2 emits:
//!
//! - One `DW_TAG_compile_unit` per program with `DW_AT_producer`,
//!   `DW_AT_language` = `DW_LANG_Rust` (closest available for now),
//!   `DW_AT_name` = source path, `DW_AT_comp_dir` = cwd,
//!   `DW_AT_low_pc` = 0, `DW_AT_high_pc` = total code size.
//! - One `DW_TAG_subprogram` per function with `DW_AT_name`,
//!   `DW_AT_low_pc`, `DW_AT_high_pc`, `DW_AT_decl_line`,
//!   `DW_AT_decl_column`, `DW_AT_external` = true.
//! - One `DW_TAG_variable` per local with `DW_AT_name`, `DW_AT_type`
//!   (pointing to a synthetic `DW_TAG_base_type`).
//! - A line program covering every entry in `FunctionDebugInfo::line_table`.
//!
//! Deferred (v0.3):
//! - Proper `Address::Symbol` so the linker patches `low_pc`/`high_pc`
//!   with real virtual addresses. We use `Address::Constant` with
//!   section-relative offsets, which gdb/lldb can inspect but won't
//!   align to runtime addresses.
//! - `.debug_loc` per-local location lists from cranelift slot-offsets.
//! - Inlining info.
//! - Generic monomorph type info.

use crate::{DebugInfoError, DebugInfoResult, FunctionDebugInfo};
use gimli::write::{
    Address, AttributeValue, DwarfUnit, EndianVec, LineProgram, LineString, Sections, UnitEntryId,
};
use gimli::{Encoding, Format, LineEncoding, LittleEndian};
use std::collections::HashMap;

/// Byte-buffers for the emitted DWARF sections. Keys are section names
/// without the leading "." (so the object writer can prefix as needed).
#[derive(Debug, Default, Clone)]
pub struct EncodedDwarf {
    pub sections: Vec<DwarfSection>,
}

#[derive(Debug, Clone)]
pub struct DwarfSection {
    /// ELF/Mach-O/COFF section name including the leading dot
    /// (e.g. `.debug_info`).
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Convenience alias.
pub type DwarfSections = EncodedDwarf;

/// Builder for a single compilation unit's DWARF.
pub struct DwarfBuilder {
    dwarf: DwarfUnit,
    /// Synthetic primitive base-type DIEs we've created, keyed by
    /// display type name (e.g. "i32"). These are children of the
    /// compile-unit DIE.
    base_types: HashMap<String, UnitEntryId>,
    /// Producer string ("mighty-0.8"). Saved on the compile-unit DIE.
    producer: String,
    /// Source file path used for `DW_AT_name` and the line program's
    /// primary file entry.
    source_path: String,
    /// Working directory used for `DW_AT_comp_dir`.
    comp_dir: String,
    /// Whether the line program has been started.
    line_program_seeded: bool,
    /// The primary file id in the line program.
    primary_file: Option<gimli::write::FileId>,
    /// 64-bit native-pointer encoding.
    encoding: Encoding,
}

impl DwarfBuilder {
    /// Create a new builder. `source_path` is the path of the .mty
    /// source file (used for `DW_AT_name` on the compile unit).
    /// `comp_dir` is typically the current working directory.
    pub fn new(source_path: impl Into<String>, comp_dir: impl Into<String>) -> Self {
        let encoding = Encoding {
            format: Format::Dwarf32,
            version: 4,
            address_size: 8,
        };
        let dwarf = DwarfUnit::new(encoding);
        Self {
            dwarf,
            base_types: HashMap::new(),
            producer: "mighty-0.8".to_string(),
            source_path: source_path.into(),
            comp_dir: comp_dir.into(),
            line_program_seeded: false,
            primary_file: None,
            encoding,
        }
    }

    /// Initialize the compile-unit DIE with producer + language + name.
    /// Must be called once before adding subprograms.
    pub fn init_compile_unit(&mut self) -> DebugInfoResult<()> {
        // Set up the line program. v4 doesn't require a comp_file entry
        // at index 0 (the compile-unit name covers it) but we still add
        // one so we can point subprogram rows at it via FileIndex.
        let mut line_program = LineProgram::new(
            self.encoding,
            LineEncoding::default(),
            LineString::String(self.comp_dir.as_bytes().to_vec()),
            LineString::String(file_basename(&self.source_path).into_bytes()),
            None,
        );
        let dir_id = line_program.default_directory();
        let file_id = line_program.add_file(
            LineString::String(file_basename(&self.source_path).into_bytes()),
            dir_id,
            None,
        );
        self.primary_file = Some(file_id);
        self.dwarf.unit.line_program = line_program;
        self.line_program_seeded = true;

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
        // DW_LANG_Rust = 0x001c (DWARF 5; older debuggers happily
        // accept it under DWARF 4 too). We use it as the closest
        // semantic match for Mighty.
        root_die.set(
            gimli::DW_AT_language,
            AttributeValue::Language(gimli::DW_LANG_Rust),
        );
        root_die.set(gimli::DW_AT_name, AttributeValue::StringRef(name_id));
        root_die.set(gimli::DW_AT_comp_dir, AttributeValue::StringRef(dir_id_str));
        // Default low_pc 0; high_pc set below once we know total size.
        root_die.set(
            gimli::DW_AT_low_pc,
            AttributeValue::Address(Address::Constant(0)),
        );
        Ok(())
    }

    /// Set the compile-unit's total code-range high_pc (as size in
    /// bytes from low_pc=0). Call after all subprograms are added.
    pub fn set_total_code_size(&mut self, size: u64) {
        let root = self.dwarf.unit.root();
        let root_die = self.dwarf.unit.get_mut(root);
        root_die.set(gimli::DW_AT_high_pc, AttributeValue::Udata(size));
    }

    /// Synthesize (or fetch) a `DW_TAG_base_type` for the given
    /// display type name. Returns its UnitEntryId so subprogram /
    /// variable DIEs can reference it.
    fn intern_base_type(&mut self, name: &str) -> UnitEntryId {
        if let Some(&id) = self.base_types.get(name) {
            return id;
        }
        let root = self.dwarf.unit.root();
        let id = self.dwarf.unit.add(root, gimli::DW_TAG_base_type);
        let name_str = self.dwarf.strings.add(name.as_bytes().to_vec());
        // Best-effort: encoding + size are conservative defaults.
        let (encoding, size) = guess_base_type(name);
        let die = self.dwarf.unit.get_mut(id);
        die.set(gimli::DW_AT_name, AttributeValue::StringRef(name_str));
        die.set(gimli::DW_AT_encoding, AttributeValue::Encoding(encoding));
        die.set(gimli::DW_AT_byte_size, AttributeValue::Data1(size));
        self.base_types.insert(name.to_string(), id);
        id
    }

    /// Add a `DW_TAG_subprogram` DIE for `fn_info` with name, ranges,
    /// decl line/column, and child `DW_TAG_variable` DIEs per local.
    /// Also emits line-program rows for every entry in
    /// `fn_info.line_table`.
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
        // DWARF 4 uses offset-from-low_pc for high_pc when expressed
        // as a Udata. gdb/lldb both accept this.
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

        // Variables.
        for var in &fn_info.locals {
            let ty_id = self.intern_base_type(&var.type_name);
            let v = self.dwarf.unit.add(sub, gimli::DW_TAG_variable);
            let n = self.dwarf.strings.add(var.name.clone().into_bytes());
            let v_die = self.dwarf.unit.get_mut(v);
            v_die.set(gimli::DW_AT_name, AttributeValue::StringRef(n));
            v_die.set(gimli::DW_AT_type, AttributeValue::UnitRef(ty_id));
            // Optional frame offset → simple DW_OP_fbreg expression.
            // v0.2 doesn't actually patch frame_base from cranelift, so
            // emit the offset as a Data8 attribute on a synthetic
            // DW_AT_data_member_location for inspection only.
            if let Some(off) = var.frame_offset {
                v_die.set(
                    gimli::DW_AT_data_member_location,
                    AttributeValue::Sdata(off as i64),
                );
            }
        }

        // Line program: emit a sequence per fn.
        let file = self.primary_file.expect("primary file initialized");
        let lp = &mut self.dwarf.unit.line_program;
        lp.begin_sequence(Some(Address::Constant(low)));
        for (addr_off, pos) in &fn_info.line_table {
            let row = lp.row();
            row.address_offset = *addr_off;
            row.file = file;
            row.line = pos.line as u64;
            row.column = pos.column as u64;
            row.is_statement = true;
            lp.generate_row();
        }
        lp.end_sequence(size);
        Ok(())
    }

    /// Consume the builder and produce encoded section buffers.
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

/// File basename, working across forward/back slashes (Windows / POSIX).
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

/// Best-effort mapping from display type names → (DW_ATE_*, size_bytes).
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
        // Aggregate or unknown — treat as opaque pointer.
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
            line_table: vec![
                (0, SourcePos::new(0, 1, 1)),
                (4, SourcePos::new(10, 2, 3)),
                (16, SourcePos::new(20, 3, 1)),
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
    fn build_produces_nonempty_sections() {
        let mut b = DwarfBuilder::new("test.mty", "/tmp");
        b.init_compile_unit().unwrap();
        b.add_function(&sample_fn()).unwrap();
        b.set_total_code_size(32);
        let enc = b.finish().unwrap();
        let names: Vec<_> = enc.sections.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&".debug_info"));
        assert!(names.contains(&".debug_abbrev"));
        assert!(names.contains(&".debug_line"));
        assert!(names.contains(&".debug_str"));
    }

    #[test]
    fn empty_program_still_emits_compile_unit() {
        let mut b = DwarfBuilder::new("empty.mty", "/tmp");
        b.init_compile_unit().unwrap();
        b.set_total_code_size(0);
        let enc = b.finish().unwrap();
        // .debug_info must contain the compile-unit DIE.
        let info = enc
            .sections
            .iter()
            .find(|s| s.name == ".debug_info")
            .expect("debug_info present");
        assert!(!info.bytes.is_empty());
    }

    #[test]
    fn file_basename_handles_windows_paths() {
        assert_eq!(file_basename("C:\\foo\\bar.mty"), "bar.mty");
        assert_eq!(file_basename("/usr/local/x.mty"), "x.mty");
        assert_eq!(file_basename("plain.mty"), "plain.mty");
    }

    #[test]
    fn rejects_inverted_range() {
        let mut b = DwarfBuilder::new("t.mty", "/tmp");
        b.init_compile_unit().unwrap();
        let mut f = sample_fn();
        f.code_range = (10, 5);
        assert!(b.add_function(&f).is_err());
    }
}
