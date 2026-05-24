//! Component Model wrapping.
//!
//! Given a core Wasm module (the existing `emit::compile_program_to_bytes`
//! output) and a [`WitDocument`], embed the WIT as a `component-type`
//! custom section and run the bytes through
//! [`wit_component::ComponentEncoder`]. The result is a binary that
//! `wasm-tools component validate` accepts.
//!
//! Closes amendment A47: full Component Model output is no longer
//! deferred — every `mty build --target wasm32-*` now emits a
//! component by default. Pass `--no-component` to fall back to the
//! bare core module (useful for debugging the lowering, or for
//! shipping into a runtime that doesn't yet support the Component
//! Model).

use crate::error::{CompileResult, WasmError};
use crate::wit::WitDocument;

/// Wrap `core_module` bytes into a Component Model component using
/// `doc` as the WIT contract.
///
/// Returns the component-encoded bytes; on validation failure the
/// raw `wit-component` error is surfaced through
/// [`WasmError::Invalid`].
pub fn wrap_as_component(core_module: &[u8], doc: &WitDocument) -> CompileResult<Vec<u8>> {
    // 1. Resolve the WIT document into a (Resolve, WorldId).
    let (resolve, _pkg_id, world_id) = doc.resolve()?;

    // 2. Embed the WIT metadata as a custom section on the core
    //    module. `wit-component` reads this back from the module in
    //    step 3.
    let mut module_bytes = core_module.to_vec();
    wit_component::embed_component_metadata(
        &mut module_bytes,
        &resolve,
        world_id,
        wit_component::StringEncoding::UTF8,
    )
    .map_err(|e| WasmError::Invalid(format!("embed wit metadata: {e:#}")))?;

    // 3. Run through the ComponentEncoder.
    let mut enc = wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module_bytes)
        .map_err(|e| WasmError::Invalid(format!("component encoder module: {e:#}")))?;
    let bytes = enc
        .encode()
        .map_err(|e| WasmError::Invalid(format!("component encode: {e:#}")))?;

    Ok(bytes)
}

/// Cheap structural check: a Component Model component starts with
/// the magic bytes `\0asm` and a version/layer word that encodes
/// `version = 0x000d`, `layer = 0x0001` little-endian — i.e. bytes
/// `[0x0d, 0x00, 0x01, 0x00]` immediately after the magic.
///
/// A core module's version word is `[0x01, 0x00, 0x00, 0x00]`.
pub fn is_component(bytes: &[u8]) -> bool {
    if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
        return false;
    }
    // The version word is little-endian; the top 16 bits are the
    // layer (0x0001 = component, 0x0000 = core).
    let layer = u16::from_le_bytes([bytes[6], bytes[7]]);
    layer == 0x0001
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::WasmTarget;
    use crate::wit::emit_wit;
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program,
        Term,
    };

    fn empty_main() -> Program {
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

    #[test]
    fn wraps_to_component_for_empty_main() {
        let core =
            crate::emit::compile_program_to_bytes(&empty_main(), WasmTarget::Wasi).expect("core");
        let doc = emit_wit(&empty_main(), "hello", WasmTarget::Wasi).expect("wit");
        let comp = wrap_as_component(&core, &doc).expect("wrap");
        assert!(is_component(&comp), "expected component preamble");
        // Component-model validator should accept it.
        let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
        v.validate_all(&comp).expect("component validates");
    }

    #[test]
    fn core_module_is_not_a_component() {
        let core =
            crate::emit::compile_program_to_bytes(&empty_main(), WasmTarget::Wasi).expect("core");
        assert!(!is_component(&core));
    }
}
