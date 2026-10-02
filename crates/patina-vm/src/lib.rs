//! Patina VM — register-based bytecode virtual machine backend.
//!
//! This crate implements the `Backend` trait using a register-based bytecode VM.
//! It compiles `CoreExpr` (from `patina-frontend`) to `CodeObject` bytecode,
//! then executes that bytecode in `VmState`.
//!
//! # Architecture
//!
//! The VM is organized into three layers:
//!
//! - **`types`** — `CodeObject`, `Instruction`, `CallFrame`, `VmClosure`, and
//!   supporting types. These compile-time types define the VM's data model.
//! - **`compiler`** — 5-pass pipeline from `CoreExpr → CodeObject`.
//! - **`runtime`** — `VmState`, execution loop, continuation machinery.
//!
//! # Reference docs
//!
//! - `PRD/phase2/VM_ISA.md` — instruction set and machine model
//! - `PRD/phase2/VM_COMPILER.md` — compiler passes
//! - `PRD/phase2/VM_RUNTIME.md` — runtime structures and execution loop
//! - `PRD/phase2/VM_DECISIONS.md` — all settled design decisions
#![expect(
    clippy::disallowed_macros,
    reason = "the crate's `thread_local!` statics, none a heap value: `runtime::control`'s \
              `EMPTY_REENTRY` and `EMPTY_HANDLERS`, the shared empty stacks that captures and wind \
              records point at, and `test_support::DROP_HIGHEST_LIVE`, a switch per test thread \
              compiled only with `test-support`. Clippy takes this lint only at a crate root, so a \
              new `thread_local!` in the crate goes on this list, which a test in \
              `reentry_lint_control.rs` holds to the crate's sources (#622)"
)]

pub mod backend;
pub mod compiler;
pub mod disasm;
pub mod error;
pub mod runtime;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod test_support;
pub mod tracer;
pub mod types;

pub use backend::{VmBackend, VmBackendError};
pub use disasm::disassemble;
pub use tracer::{StepTracer, TracerHandle};
