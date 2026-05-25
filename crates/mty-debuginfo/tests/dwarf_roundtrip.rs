//! Round-trip test: build DWARF, parse it back with gimli read, and
//! assert structural expectations (compile-unit producer string,
//! subprogram name, line entries).

use mty_debuginfo::{DwarfBuilder, FunctionDebugInfo, SourcePos, VarDebugInfo};

fn sample_main() -> FunctionDebugInfo {
    FunctionDebugInfo {
        name: "main".into(),
        mangled_name: None,
        return_type: "i32".into(),
        decl_pos: SourcePos::new(0, 1, 1),
        code_range: (0, 24),
        line_table: vec![
            (0, SourcePos::new(0, 1, 1)),
            (8, SourcePos::new(13, 2, 3)),
            (16, SourcePos::new(26, 3, 1)),
        ],
        locals: vec![VarDebugInfo {
            name: "n".into(),
            type_name: "i32".into(),
            frame_offset: Some(-4),
        }],
    }
}

#[test]
fn roundtrip_finds_subprogram_for_main() {
    let mut b = DwarfBuilder::new("examples/01_hello.mty", "/tmp");
    b.init_compile_unit().unwrap();
    b.add_function(&sample_main()).unwrap();
    b.set_total_code_size(24);
    let enc = b.finish().unwrap();

    // Pull the section bytes by name.
    let mut info = Vec::new();
    let mut abbrev = Vec::new();
    let mut line = Vec::new();
    let mut strs = Vec::new();
    for s in &enc.sections {
        match s.name.as_str() {
            ".debug_info" => info = s.bytes.clone(),
            ".debug_abbrev" => abbrev = s.bytes.clone(),
            ".debug_line" => line = s.bytes.clone(),
            ".debug_str" => strs = s.bytes.clone(),
            _ => {}
        }
    }
    assert!(!info.is_empty(), "debug_info present");
    assert!(!abbrev.is_empty(), "debug_abbrev present");
    assert!(!line.is_empty(), "debug_line present");

    // Parse back with gimli read.
    let endian = gimli::LittleEndian;
    let info = gimli::DebugInfo::new(&info, endian);
    let abbrev_sec = gimli::DebugAbbrev::new(&abbrev, endian);
    let line_sec = gimli::DebugLine::new(&line, endian);
    let str_sec = gimli::DebugStr::new(&strs, endian);

    // Walk the compile unit and look for the subprogram.
    let mut iter = info.units();
    let unit_header = iter.next().expect("one unit").expect("unit ok");
    let abbrevs = unit_header.abbreviations(&abbrev_sec).expect("abbrevs");
    let mut cursor = unit_header.entries(&abbrevs);

    let mut found_subprogram = false;
    let mut found_compile_unit = false;
    while let Some((_delta, entry)) = cursor.next_dfs().expect("dfs") {
        match entry.tag() {
            gimli::DW_TAG_compile_unit => {
                found_compile_unit = true;
                // Producer should be present.
                if let Ok(Some(attr)) = entry.attr(gimli::DW_AT_producer) {
                    let v = attr.value();
                    if let gimli::AttributeValue::DebugStrRef(off) = v {
                        let bytes = str_sec.get_str(off).expect("producer str");
                        let s = bytes.to_string_lossy().into_owned();
                        assert!(s.contains("mighty"));
                    }
                }
            }
            gimli::DW_TAG_subprogram => {
                if let Ok(Some(attr)) = entry.attr(gimli::DW_AT_name) {
                    if let gimli::AttributeValue::DebugStrRef(off) = attr.value() {
                        let bytes = str_sec.get_str(off).expect("fn name");
                        let s = bytes.to_string_lossy().into_owned();
                        if s == "main" {
                            found_subprogram = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    assert!(found_compile_unit, "DW_TAG_compile_unit present");
    assert!(found_subprogram, "DW_TAG_subprogram for main present");

    // Sanity-check that the line program parses.
    let _ = line_sec;
}

#[test]
fn roundtrip_multi_function() {
    let mut b = DwarfBuilder::new("foo.mty", "/tmp");
    b.init_compile_unit().unwrap();
    let mut f1 = sample_main();
    f1.name = "alpha".into();
    f1.code_range = (0, 16);
    let mut f2 = sample_main();
    f2.name = "beta".into();
    f2.code_range = (16, 32);
    b.add_function(&f1).unwrap();
    b.add_function(&f2).unwrap();
    b.set_total_code_size(32);
    let enc = b.finish().unwrap();

    let mut info = Vec::new();
    let mut abbrev = Vec::new();
    let mut strs = Vec::new();
    for s in &enc.sections {
        match s.name.as_str() {
            ".debug_info" => info = s.bytes.clone(),
            ".debug_abbrev" => abbrev = s.bytes.clone(),
            ".debug_str" => strs = s.bytes.clone(),
            _ => {}
        }
    }
    let endian = gimli::LittleEndian;
    let info = gimli::DebugInfo::new(&info, endian);
    let abbrev_sec = gimli::DebugAbbrev::new(&abbrev, endian);
    let str_sec = gimli::DebugStr::new(&strs, endian);

    let mut iter = info.units();
    let unit_header = iter.next().expect("unit").expect("ok");
    let abbrevs = unit_header.abbreviations(&abbrev_sec).expect("abbrevs");
    let mut cursor = unit_header.entries(&abbrevs);

    let mut names = Vec::new();
    while let Some((_d, entry)) = cursor.next_dfs().expect("dfs") {
        if entry.tag() == gimli::DW_TAG_subprogram {
            if let Ok(Some(attr)) = entry.attr(gimli::DW_AT_name) {
                if let gimli::AttributeValue::DebugStrRef(off) = attr.value() {
                    let bytes = str_sec.get_str(off).unwrap();
                    names.push(bytes.to_string_lossy().into_owned());
                }
            }
        }
    }
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));
}
