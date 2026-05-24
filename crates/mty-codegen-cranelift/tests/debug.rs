//! End-to-end: `compile_object_with_debug` produces an object that, when
//! re-read with `object::read` + `gimli`, exposes a `DW_TAG_subprogram`
//! for the user's `main` fn plus the compile-unit's producer string.

use mty_codegen_cranelift::{compile_object_with_debug, Monomorphizer};
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program, Term,
};
use mty_types::IntKind;
use object::read::{Object as _, ObjectSection as _};
use std::io::Read;

fn empty_main_prog() -> Program {
    let mut p = Program::default();
    p.fns.push(Function {
        id: IrFnId(0),
        name: "main".into(),
        params: vec![],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "n".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: false,
                source: LocalSource::UserLet,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 12 },
    });
    p
}

#[test]
fn emits_debug_subprogram_for_main() {
    let dir = tempfile::tempdir().expect("tempdir");
    let obj_path = dir.path().join("hello.o");
    let prog = Monomorphizer::new(&empty_main_prog()).run();
    let src = "fn main() {\n  let n = 0\n}\n";
    let res = compile_object_with_debug(&prog, &obj_path, src, "hello.mty");
    assert!(
        res.is_ok(),
        "compile_object_with_debug failed: {:?}",
        res.err()
    );

    // Read it back and inspect debug sections.
    let mut bytes = Vec::new();
    std::fs::File::open(&obj_path)
        .expect("open obj")
        .read_to_end(&mut bytes)
        .expect("read obj");
    let parsed = object::read::File::parse(&*bytes).expect("parse object");

    // Find a DWARF section. Naming differs per platform; check for any
    // of the common spellings.
    let mut found_dwarf_section = false;
    for section in parsed.sections() {
        let name = section.name().unwrap_or("");
        if name.starts_with(".debug_") || name.starts_with("__debug_") {
            found_dwarf_section = true;
        }
    }
    assert!(
        found_dwarf_section,
        "expected at least one .debug_* / __debug_* section in the emitted object",
    );

    // Locate .debug_info and the DW_TAG_subprogram for main.
    let endian = if parsed.is_little_endian() {
        gimli::RunTimeEndian::Little
    } else {
        gimli::RunTimeEndian::Big
    };
    let load = |id: gimli::SectionId| -> Result<Vec<u8>, ()> {
        for section in parsed.sections() {
            let name = section.name().unwrap_or("");
            if name == id.name() || name == format!("__{}", id.name().trim_start_matches('.')) {
                return Ok(section.data().unwrap_or_default().to_vec());
            }
        }
        Ok(Vec::new())
    };
    let info_bytes = load(gimli::SectionId::DebugInfo).unwrap();
    let abbrev_bytes = load(gimli::SectionId::DebugAbbrev).unwrap();
    let str_bytes = load(gimli::SectionId::DebugStr).unwrap();
    assert!(!info_bytes.is_empty(), ".debug_info has content");
    assert!(!abbrev_bytes.is_empty(), ".debug_abbrev has content");

    let debug_info = gimli::DebugInfo::new(&info_bytes, endian);
    let debug_abbrev = gimli::DebugAbbrev::new(&abbrev_bytes, endian);
    let debug_str = gimli::DebugStr::new(&str_bytes, endian);

    let mut units = debug_info.units();
    let unit = units.next().expect("at least one unit").expect("ok");
    let abbrevs = unit.abbreviations(&debug_abbrev).expect("abbrevs ok");
    let mut cursor = unit.entries(&abbrevs);
    let mut found_main = false;
    while let Some((_d, entry)) = cursor.next_dfs().expect("dfs") {
        if entry.tag() == gimli::DW_TAG_subprogram {
            if let Ok(Some(attr)) = entry.attr(gimli::DW_AT_name) {
                if let gimli::AttributeValue::DebugStrRef(off) = attr.value() {
                    let bytes = debug_str.get_str(off).expect("name");
                    let s = bytes.to_string_lossy().into_owned();
                    if s == "main" {
                        found_main = true;
                    }
                }
            }
        }
    }
    assert!(found_main, "DW_TAG_subprogram name=main present");
}

#[test]
fn release_build_can_skip_debug_via_plain_compile_object() {
    use mty_codegen_cranelift::compile_object;
    let dir = tempfile::tempdir().expect("tempdir");
    let obj_path = dir.path().join("strip.o");
    let prog = Monomorphizer::new(&empty_main_prog()).run();
    compile_object(&prog, &obj_path).expect("compile");

    // Confirm no .debug_* sections are present.
    let mut bytes = Vec::new();
    std::fs::File::open(&obj_path)
        .expect("open obj")
        .read_to_end(&mut bytes)
        .expect("read");
    let parsed = object::read::File::parse(&*bytes).expect("parse");
    for s in parsed.sections() {
        let name = s.name().unwrap_or("");
        assert!(
            !name.starts_with(".debug_") && !name.starts_with("__debug_"),
            "release build should not emit DWARF section, found: {name}",
        );
    }
}
