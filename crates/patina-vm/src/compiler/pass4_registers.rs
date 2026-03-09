//! Pass 4 — Register Allocation
//!
//! Assigns frame-relative register indices to every variable and temporary in
//! `TailedExpr`, producing `RegExpr`. Uses a simple linear-scan allocator:
//! registers are allocated left-to-right and released when their last use is
//! passed.
//!
//! **Tail-call overlap invariant** (VM_DECISIONS.md §tail-call):
//! Before emitting a `TailCall`, Pass 4 materialises all argument values into
//! fresh temporary registers before any destination parameter register is
//! overwritten. This ensures the runtime can use a simple sequential copy
//! without risk of clobbering a source before it is read. Tests in
//! `patina-vm/tests/tail_call_overlap.rs` enforce this invariant.
//!
//! Output: `RegExpr` — `TailedExpr` nodes augmented with register assignments.
//!
//! See VM_COMPILER.md §Pass 4.

use super::pass3_tail::TailedExpr;

/// A register-allocated expression.
///
/// Phase 2A placeholder.
#[derive(Debug, Clone)]
pub enum RegExpr {
    /// A `TailedExpr` node that has been assigned a destination register.
    Node {
        inner: TailedExpr,
        /// The register that holds this expression's result.
        dst: u16,
    },
}

pub struct Pass4Registers;

impl Pass4Registers {
    /// Allocate registers for `expr`.
    pub fn run(expr: &TailedExpr) -> RegExpr {
        // TODO(A2): implement linear-scan register allocation
        RegExpr::Node {
            inner: expr.clone(),
            dst: 0,
        }
    }
}
