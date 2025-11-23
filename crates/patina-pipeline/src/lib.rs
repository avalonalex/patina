//! Flexible pipeline orchestration for Patina Scheme
//!
//! This crate provides a pluggable pipeline architecture that orchestrates:
//! - Parsing (lexing + parsing)
//! - Macro expansion
//! - Desugaring (optional, for CoreExpr IR)
//! - Evaluation (tree-walker, or future VM/JIT)
//!
//! The pipeline architecture allows different evaluation strategies to coexist
//! and makes it easy to swap components for testing and optimization.

pub mod error;
pub mod pipeline;
pub mod standard;

// Re-export main types
pub use error::{PipelineError, PipelineResult};
pub use pipeline::{EvaluationStrategy, Pipeline};
pub use standard::StandardPipeline;
