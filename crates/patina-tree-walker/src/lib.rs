//! Patina Tree-Walking Interpreter
//!
//! This crate contains the tree-walking interpreter backend for Patina Scheme.
//!
//! # Components
//!
//! - **TreeWalker** - Backend trait implementation wrapping the evaluator
//! - **Evaluator** - Core evaluation engine (eval module)
//! - **Special Forms** - Built-in special forms (quote, lambda, if, etc.)
//! - **Primitives** - Built-in procedures organized by category
//!
//! # Architecture
//!
//! The tree-walking interpreter directly evaluates the AST produced by the frontend.
//! It uses lexical scoping with environment chains and supports full R7RS semantics.
//!
//! # Using as a Backend
//!
//! ```ignore
//! use patina_tree_walker::TreeWalker;
//! use patina_runtime::Backend;
//!
//! let backend = TreeWalker::new();
//! let result = backend.eval_global(expr)?;
//! ```
#![expect(
    clippy::disallowed_macros,
    reason = "the crate's `thread_local!` statics, all in `cps_eval::types`: `PENDING_ESCAPE`, a \
              continuation's value parked between its jump and the trampoline that resumes it, \
              which the safe point roots (`trace_pending_escape`) and which leaves `thread_local!` \
              in the redesign (PRD/GC_PRD.md §11.3); `ACTIVE_TRAMPOLINES`, `NEXT_TRAMPOLINE` and \
              `UNHANDLED_IN_CALLBACK`, ids and flags of the running trampolines. Clippy takes this \
              lint only at a crate root, so a new `thread_local!` in the crate goes on this list, \
              which a test in `reentry_lint_control.rs` holds to the crate's sources (#622)"
)]

pub mod backend;
pub mod eval;
pub mod library_support;

// Re-export main types
pub use backend::TreeWalker;
pub use eval::{CpsEvaluator, EvalError, Evaluator, eval_cps};
pub use library_support::SchemeLibraryLoader;
