//! Pass 3 — Tail Position Marking
//!
//! Annotates every `App` (function application) node in `ClosedExpr` with
//! `is_tail: bool`. An application is in tail position when its result is
//! returned directly from the enclosing lambda without further processing.
//!
//! Rules (R7RS §3.5 / proper tail calls):
//! - The last expression of a `begin` / lambda body is in tail position.
//! - The consequent and alternative of an `if` are in tail position when the
//!   `if` itself is in tail position.
//! - `let`/`cond`/`case` etc. are already desugared to `if` + `begin` by the
//!   frontend, so no special handling is needed here.
//!
//! Output: `TailedExpr` — same shape as `ClosedExpr` with `is_tail` flags.
//!
//! See VM_COMPILER.md §Pass 3.

use super::pass2_closure::ClosedExpr;

/// A tail-annotated expression.
///
/// Phase 2A placeholder — extended as passes are implemented.
#[derive(Debug, Clone)]
pub enum TailedExpr {
    /// A `ClosedExpr` node that required no tail annotation.
    Raw(ClosedExpr),

    /// A function application with an explicit tail flag.
    App {
        func: Box<TailedExpr>,
        args: Vec<TailedExpr>,
        is_tail: bool,
    },
}

pub struct Pass3Tail;

impl Pass3Tail {
    /// Annotate tail positions in `expr`.
    pub fn run(expr: &ClosedExpr) -> TailedExpr {
        // TODO(A2): implement tail-position marking
        TailedExpr::Raw(expr.clone())
    }
}
