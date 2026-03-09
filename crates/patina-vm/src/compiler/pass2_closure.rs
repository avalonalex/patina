//! Pass 2 — Closure Conversion
//!
//! Transforms `CoreExpr` into `ClosedExpr` by making all free-variable captures
//! explicit. After this pass every lambda knows exactly which variables it
//! captures and in what order.
//!
//! Key transformations:
//! - Each `Lambda` gains an explicit `capture_list: Vec<Symbol>` drawn from
//!   `AnalysisInfo::free_vars`.
//! - Variables that are `set!`-mutated after capture are wrapped in a
//!   `Box` node at the binding site; reads become `Unbox` and writes become
//!   `SetBox`. The actual heap type is `HeapObjectData::MutableCell`.
//!
//! Output: `ClosedExpr` — a `CoreExpr`-like tree with explicit capture lists
//! and box/unbox annotations.
//!
//! See VM_COMPILER.md §Pass 2.

use super::pass1_analysis::AnalysisInfo;
use patina_core::core_expr::{CoreExpr, Symbol};

/// A closure-converted expression.
///
/// Mirrors the shape of `CoreExpr` but lambdas carry an explicit capture list
/// and mutable variables are wrapped in box/unbox nodes.
///
/// Phase 2A: this is a placeholder enum — extend as passes are implemented.
#[derive(Debug, Clone)]
pub enum ClosedExpr {
    /// Wraps an original `CoreExpr` node that needed no transformation.
    /// Replaced incrementally as the pass is fleshed out.
    Raw(CoreExpr),

    /// A lambda with an explicit capture list.
    Lambda {
        params: Vec<Symbol>,
        capture_list: Vec<Symbol>,
        body: Vec<Box<ClosedExpr>>,
    },

    /// Allocate a `MutableCell` on the heap wrapping `inner`.
    Box(Box<ClosedExpr>),

    /// Read the current value from a `MutableCell`.
    Unbox(Box<ClosedExpr>),

    /// Write `val` into the `MutableCell` held in `cell`.
    SetBox {
        cell: Box<ClosedExpr>,
        val: Box<ClosedExpr>,
    },
}

pub struct Pass2Closure;

impl Pass2Closure {
    /// Run closure conversion over `expr` using the pre-computed `analysis`.
    pub fn run(expr: &CoreExpr, _analysis: &AnalysisInfo) -> ClosedExpr {
        // TODO(A2): implement closure conversion
        ClosedExpr::Raw(expr.clone())
    }
}
