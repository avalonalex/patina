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

pub mod backend;
pub mod compiler;
pub mod disasm;
pub mod error;
pub mod runtime;
pub mod types;

pub use backend::VmBackend;
pub use disasm::disassemble;
