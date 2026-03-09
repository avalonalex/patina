//! 5-pass compiler pipeline: `CoreExpr → CodeObject`.
//!
//! Each pass is a pure stateless transformation. Passes are run sequentially;
//! each feeds its output into the next.
//!
//! ```text
//! CoreExpr
//!   → Pass1Analysis  (NodeId assignment + free-var analysis + mutation detection)
//!   → Pass2Closure   (closure conversion: explicit capture lists, Box/Unbox)
//!   → Pass3Tail      (tail-position annotation)
//!   → Pass4Registers (linear-scan register allocation)
//!   → Pass5Codegen   (code generation → CodeObject)
//! ```
//!
//! See VM_COMPILER.md for the full specification.

pub mod pass1_analysis;
pub mod pass2_closure;
pub mod pass3_tail;
pub mod pass4_registers;
pub mod pass5_codegen;

use crate::error::CompileError;
use crate::types::CodeObject;
use patina_core::core_expr::CoreExpr;

/// Compile a `CoreExpr` into a `CodeObject` (plus any nested `CodeObject`s).
///
/// Returns `(top_level_code, nested_codes)`. The caller should load all of
/// them into `VmState::code_store` before executing.
pub fn compile(expr: &CoreExpr) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
    let analysis = pass1_analysis::Pass1Analysis::run(expr);
    let closed = pass2_closure::Pass2Closure::run(expr, &analysis);
    let tailed = pass3_tail::Pass3Tail::run(&closed);
    let allocated = pass4_registers::Pass4Registers::run(&tailed);
    pass5_codegen::Pass5Codegen::run(&allocated)
}
