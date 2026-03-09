//! Pass 1 — Analysis
//!
//! Two sub-passes, run together in a single tree walk:
//!
//! **1a. NodeId assignment** — assigns a monotonic `NodeId` to every `Lambda`
//! node. This id is the key into `AnalysisInfo` maps so later passes can look
//! up per-lambda metadata without re-walking the tree.
//!
//! **1b. Free-variable analysis + mutation detection** — for each lambda,
//! computes:
//! - `free_vars`: variables referenced but not bound by that lambda.
//! - `mutated`: variables targeted by `Set!` anywhere in the lambda body
//!   (including nested lambdas), used by Pass 2 to decide which bindings need
//!   `MutableCell` boxing.
//!
//! Output: `AnalysisInfo` — a cheap-to-clone summary consumed by Pass 2.
//!
//! See VM_COMPILER.md §Pass 1.

use patina_core::core_expr::{CoreExpr, Symbol};
use std::collections::{HashMap, HashSet};

/// Stable id for a `Lambda` node in the `CoreExpr` tree.
/// Assigned during the NodeId pre-pass (top-down, pre-order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// Per-lambda analysis results.
#[derive(Debug, Clone, Default)]
pub struct LambdaInfo {
    /// Free variables referenced by this lambda (not including nested lambdas'
    /// free vars — each lambda has its own entry).
    pub free_vars: Vec<Symbol>,

    /// Variables bound in this lambda's scope that are targeted by `set!`
    /// (either directly in this body or in a nested lambda that captures them).
    pub mutated_bindings: HashSet<Symbol>,
}

/// The complete output of Pass 1.
#[derive(Debug, Default)]
pub struct AnalysisInfo {
    /// Maps each `NodeId` → per-lambda analysis.
    pub lambdas: HashMap<NodeId, LambdaInfo>,

    /// Set of all variable names that appear as `set!` targets anywhere in
    /// the analysed expression. Quick global check for Pass 2.
    pub all_mutated: HashSet<Symbol>,
}

pub struct Pass1Analysis;

impl Pass1Analysis {
    /// Run the analysis pass over `expr` and return the collected info.
    pub fn run(_expr: &CoreExpr) -> AnalysisInfo {
        // TODO(A2): implement NodeId assignment + free-var/mutation analysis
        AnalysisInfo::default()
    }
}
