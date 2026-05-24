//! Shared fixture helpers for the integration tests.

use mty_hir::SourceSpan;
use mty_ir::ir::{
    AdtRef, AdtRefKind, Block, BlockId, Const, FieldRef, Function, IrFnId, IrTy, LocalDecl,
    LocalSource, Operand, Program, Term, VariantRef,
};
use mty_types::IntKind;

#[allow(dead_code)]
pub fn empty_main() -> Program {
    let mut p = Program::default();
    p.fns.push(Function {
        id: IrFnId(0),
        name: "main".into(),
        params: vec![],
        locals: vec![LocalDecl {
            name: "_0".into(),
            ty: IrTy::Unit,
            mutable: false,
            source: LocalSource::Return,
        }],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });
    p
}

#[allow(dead_code)]
pub fn program_with_adts_and_fn() -> Program {
    use mty_types::AdtId;
    let mut p = empty_main();

    // struct Point { x: i32, y: i32 }
    p.adts.push(AdtRef {
        adt: AdtId(0),
        name: "Point".into(),
        kind: AdtRefKind::Struct,
        variants: vec![VariantRef {
            name: "Point".into(),
            fields: vec![
                FieldRef {
                    name: Some("x".into()),
                    ty: IrTy::Int(IntKind::I32),
                },
                FieldRef {
                    name: Some("y".into()),
                    ty: IrTy::Int(IntKind::I32),
                },
            ],
        }],
    });

    // enum Color { Red, Green, Blue }
    p.adts.push(AdtRef {
        adt: AdtId(1),
        name: "Color".into(),
        kind: AdtRefKind::Enum,
        variants: vec![
            VariantRef {
                name: "Red".into(),
                fields: vec![],
            },
            VariantRef {
                name: "Green".into(),
                fields: vec![],
            },
            VariantRef {
                name: "Blue".into(),
                fields: vec![],
            },
        ],
    });

    // enum Shape { Circle(f64), Square(f64) }   (variant with payload)
    use mty_types::FloatKind;
    p.adts.push(AdtRef {
        adt: AdtId(2),
        name: "Shape".into(),
        kind: AdtRefKind::Enum,
        variants: vec![
            VariantRef {
                name: "Circle".into(),
                fields: vec![FieldRef {
                    name: None,
                    ty: IrTy::Float(FloatKind::F64),
                }],
            },
            VariantRef {
                name: "Square".into(),
                fields: vec![FieldRef {
                    name: None,
                    ty: IrTy::Float(FloatKind::F64),
                }],
            },
        ],
    });

    // fn add(a: i32, b: i32) -> i32
    let mut add = Function {
        id: IrFnId(1),
        name: "add".into(),
        params: vec![mty_ir::ir::Local(1), mty_ir::ir::Local(2)],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "a".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: false,
                source: LocalSource::Param,
            },
            LocalDecl {
                name: "b".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: false,
                source: LocalSource::Param,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Int(0, IntKind::I32))),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Int(IntKind::I32),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    };
    add.params = vec![mty_ir::ir::Local(1), mty_ir::ir::Local(2)];
    p.fns.push(add);

    p
}
