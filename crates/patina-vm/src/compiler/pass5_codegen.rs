//! Pass 5 — Code Generation
//!
//! Walks `RegExpr` and emits `Instruction`s into a `CodeObject`. Nested lambdas
//! are compiled recursively; each produces its own `CodeObject` referenced by
//! `CodeObjectId` in the parent's `MakeClosure` instruction.
//!
//! Label patching: forward jumps are emitted with a placeholder target (`0`)
//! and patched once the target instruction index is known.
//!
//! Two-pass top-level define:
//! - First sub-pass: scan all top-level `Define` names and register them as
//!   globals (so forward references within the same compilation unit work).
//! - Second sub-pass: compile bodies and emit `Define` instructions.
//!
//! Output: a `CodeObject` for the top-level scope (nested `CodeObject`s are
//! embedded as constants or referenced by id in the parent).
//!
//! See VM_COMPILER.md §Pass 5.

use super::pass4_registers::RegExpr;
use crate::error::CompileError;
use crate::types::code_object::{Arity, CodeObject, CodeObjectId};
use crate::types::instruction::Instruction;

pub struct Pass5Codegen;

impl Pass5Codegen {
    /// Generate a `CodeObject` from the register-allocated expression.
    pub fn run(_expr: &RegExpr) -> Result<CodeObject, CompileError> {
        // TODO(A2): implement code generation
        Ok(CodeObject {
            id: CodeObjectId(0),
            name: None,
            instructions: vec![Instruction::Nop],
            constants: vec![],
            num_regs: 1,
            arity: Arity::Fixed(0),
            source_map: vec![],
        })
    }
}
