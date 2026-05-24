//! mty-codegen-cranelift: SIR → Cranelift IR → native code (slice 8).
//!
//! Two output modes:
//!
//! - **JIT** ([`jit::JitModule`]): used by `sdust run` to compile and
//!   execute in-process. Returns a fn-ptr to `main`.
//! - **Object** ([`object::compile_object`]): used by `sdust build` to
//!   emit a host-format `.o` that the platform linker turns into an
//!   executable.
//!
//! Both modes share the same lowering layer ([`lower`]). The lowering
//! is intentionally conservative: SIR shapes the slice-8 lowerer
//! cannot translate raise [`CodegenError::Unsupported`], which the
//! driver routes back to the interpreter (the slice-7 path). Real
//! errors (cranelift verifier failures, layout impossibility, missing
//! runtime imports) raise their own variants.
//!
//! See:
//! - `docs/internals/codegen-cranelift.md` for the high-level design,
//! - `docs/superpowers/specs/2026-05-24-slice8-codegen-design.md` for
//!   the slice-leader decision record.

pub mod abi;
pub mod aggregate;
pub mod artifact;
pub mod debug;
pub mod error;
pub mod jit;
pub mod layout;
pub mod lower;
pub mod mono;
pub mod object;
pub mod runtime_imports;

pub use artifact::NativeArtifact;
pub use error::{CodegenError, CompileResult};
pub use jit::{JitCompiled, JitMain};
pub use mono::Monomorphizer;
pub use object::{compile_object, compile_object_with_debug, ObjectArtifact};
