//! Integration tests for the DWARF v5 builder (`Dwarf5Builder`).
//!
//! Companion to `tests/dwarf_roundtrip.rs` which exercises the v4
//! path. These tests assert v5-specific shape: the `.debug_line`
//! header carries the `0x0005` version word, the `.debug_line_str`
//! indirect string table is emitted, per-instruction line rows match
//! the `line_table` length, and a gimli round-trip can re-parse the
//! v5 output back into walkable units.

use mty_debuginfo::{Dwarf5Builder, FunctionDebugInfo, SourcePos, VarDebugInfo};

fn sample_fn_with_per_instr_lines() -> FunctionDebugInfo {
    // 8 per-instruction entries inside a single fn, with monotonic
    // address offsets. A v4 basic-block-level emitter would typically
    // produce 2–3 rows for this; v5 records all 8.
    let mut line_table = Vec::new();
    for i in 0..8u64 {
        line_table.push((
            i * 4,
            SourcePos::new((i * 5) as u32, 1 + i as u32, 1 + (i as u32 % 4) * 2),
        ));
    }
    FunctionDebugInfo {
        name: "fib".into(),
        mangled_name: None,
        return_type: "i64".into(),
        decl_pos: SourcePos::new(0, 1, 1),
        code_range: (0, 32),
        line_table,
        locals: vec![
            VarDebugInfo {
                name: "n".into(),
                type_name: "i64".into(),
                frame_offset: Some(-8),
            },
            VarDebugInfo {
                name: "a".into(),
                type_name: "i64".into(),
                frame_offset: Some(-16),
            },
        ],
    }
}

/// Pull a section's bytes from an `EncodedDwarf` (by exact name).
fn section_bytes(enc: &mty_debuginfo::EncodedDwarf, name: &str) -> Option<Vec<u8>> {
    enc.sections
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.bytes.clone())
}

#[test]
fn emit_v5_line_program_shape() {
    let mut b = Dwarf5Builder::new("examples/01_hello.mty", "/work");
    b.init_compile_unit().unwrap();
    b.add_function(&sample_fn_with_per_instr_lines()).unwrap();
    b.set_total_code_size(32);
    let enc = b.finish().unwrap();

    let line = section_bytes(&enc, ".debug_line").expect(".debug_line emitted");
    // DWARF32 line header layout:
    //   bytes 0..4  : unit_length (little-endian u32, < 0xfffffff0)
    //   bytes 4..6  : version (u16)
    // Assert the version field is 5.
    assert!(
        line.len() >= 6,
        "line section too short to inspect version: {} bytes",
        line.len()
    );
    let version = u16::from_le_bytes([line[4], line[5]]);
    assert_eq!(version, 5, "expected DWARF v5 line-program magic");
}

#[test]
fn emit_v5_indirect_string_table() {
    let mut b = Dwarf5Builder::new("/repo/examples/02_loop.mty", "/repo");
    b.init_compile_unit().unwrap();
    b.add_function(&sample_fn_with_per_instr_lines()).unwrap();
    b.set_total_code_size(32);
    let enc = b.finish().unwrap();

    let line_str = section_bytes(&enc, ".debug_line_str")
        .expect("v5 emits .debug_line_str for indirect dir/file names");
    assert!(!line_str.is_empty(), ".debug_line_str must be non-empty");
    // The comp_dir and basename should appear inside the line-str
    // table as raw null-terminated strings (the format `gimli` writes).
    let blob = String::from_utf8_lossy(&line_str);
    assert!(
        blob.contains("/repo"),
        ".debug_line_str should contain comp_dir; got: {blob:?}"
    );
    assert!(
        blob.contains("02_loop.mty"),
        ".debug_line_str should contain source basename; got: {blob:?}"
    );
}

#[test]
fn per_instruction_line_records() {
    // Two functions, each with 8 per-instruction rows = 16 rows
    // total across 2 sequences. A per-basic-block emitter for the
    // same fns would yield closer to 2 rows total (one per fn entry).
    let mut b = Dwarf5Builder::new("multi.mty", "/work");
    b.init_compile_unit().unwrap();
    let mut f1 = sample_fn_with_per_instr_lines();
    f1.name = "alpha".into();
    f1.code_range = (0, 32);
    let mut f2 = sample_fn_with_per_instr_lines();
    f2.name = "beta".into();
    f2.code_range = (32, 64);
    b.add_function(&f1).unwrap();
    b.add_function(&f2).unwrap();
    b.set_total_code_size(64);
    let rows = b.rows_emitted();
    let seqs = b.sequences_emitted();
    let _ = b.finish().unwrap();

    assert_eq!(seqs, 2, "one sequence per fn");
    assert_eq!(rows, 16, "8 rows per fn × 2 fns");
    // Per-instruction granularity sanity: rows must exceed sequences.
    assert!(
        rows > seqs,
        "per-instruction line program: rows ({rows}) > sequences ({seqs})"
    );
}

#[test]
fn v5_output_reparses_with_gimli_read() {
    let mut b = Dwarf5Builder::new("examples/01_hello.mty", "/work");
    b.init_compile_unit().unwrap();
    b.add_function(&sample_fn_with_per_instr_lines()).unwrap();
    b.set_total_code_size(32);
    let enc = b.finish().unwrap();

    let info = section_bytes(&enc, ".debug_info").expect(".debug_info");
    let abbrev = section_bytes(&enc, ".debug_abbrev").expect(".debug_abbrev");
    let strs = section_bytes(&enc, ".debug_str").unwrap_or_default();

    let endian = gimli::LittleEndian;
    let info = gimli::DebugInfo::new(&info, endian);
    let abbrev_sec = gimli::DebugAbbrev::new(&abbrev, endian);
    let str_sec = gimli::DebugStr::new(&strs, endian);

    let mut iter = info.units();
    let unit_header = iter.next().expect("one unit").expect("unit ok");
    // The CU header carries the DWARF version too — assert v5.
    assert_eq!(
        unit_header.version(),
        5,
        "compile-unit version should be DWARF v5"
    );
    let abbrevs = unit_header.abbreviations(&abbrev_sec).expect("abbrevs");
    let mut cursor = unit_header.entries(&abbrevs);

    let mut found_subprogram = false;
    let mut found_compile_unit = false;
    while let Some((_d, entry)) = cursor.next_dfs().expect("dfs") {
        match entry.tag() {
            gimli::DW_TAG_compile_unit => {
                found_compile_unit = true;
            }
            gimli::DW_TAG_subprogram => {
                if let Ok(Some(attr)) = entry.attr(gimli::DW_AT_name) {
                    if let gimli::AttributeValue::DebugStrRef(off) = attr.value() {
                        let bytes = str_sec.get_str(off).expect("fn name");
                        if bytes.to_string_lossy() == "fib" {
                            found_subprogram = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    assert!(found_compile_unit, "DW_TAG_compile_unit present");
    assert!(found_subprogram, "DW_TAG_subprogram for fib present");
}

#[test]
fn v5_total_section_count_includes_line_str() {
    // Sanity check: v5 emits strictly more section *kinds* than v4
    // when comp_dir/file go through line_strings. v4 produces
    // .debug_info/.debug_abbrev/.debug_line/.debug_str (4); v5 adds
    // .debug_line_str on top.
    let mut b = Dwarf5Builder::new("x.mty", "/tmp");
    b.init_compile_unit().unwrap();
    b.add_function(&sample_fn_with_per_instr_lines()).unwrap();
    b.set_total_code_size(32);
    let enc = b.finish().unwrap();

    let names: Vec<_> = enc.sections.iter().map(|s| s.name.clone()).collect();
    for expected in [
        ".debug_info",
        ".debug_abbrev",
        ".debug_line",
        ".debug_str",
        ".debug_line_str",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "expected {expected} in v5 output, got {names:?}"
        );
    }
}
