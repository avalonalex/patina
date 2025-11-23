//! Core pipeline trait and types

use crate::error::PipelineResult;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Evaluation strategy for the pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationStrategy {
    /// Direct evaluation of Value AST (tree-walker)
    Direct,

    /// Desugar to CoreExpr IR before evaluation
    CoreExpr,

    /// Future: Compile to bytecode and run in VM
    #[allow(dead_code)]
    Bytecode,

    /// Future: JIT compilation
    #[allow(dead_code)]
    Jit,
}

/// Main pipeline trait for orchestrating Scheme evaluation
///
/// This trait defines the interface for different pipeline configurations.
/// Implementations can use different parsers, macro expanders, desugaring
/// strategies, and evaluators.
pub trait Pipeline {
    /// Evaluate a single Scheme expression
    ///
    /// # Arguments
    /// - `code`: Scheme source code (single expression)
    /// - `env`: Runtime environment for variable bindings
    ///
    /// # Returns
    /// The result of evaluating the expression
    fn eval(&self, code: &str, env: &Rc<Environment>) -> PipelineResult<Value>;

    /// Evaluate a Scheme program (multiple expressions)
    ///
    /// # Arguments
    /// - `code`: Scheme source code (multiple expressions)
    /// - `env`: Runtime environment for variable bindings
    ///
    /// # Returns
    /// The result of the last expression
    fn eval_program(&self, code: &str, env: &Rc<Environment>) -> PipelineResult<Value>;

    /// Get the evaluation strategy used by this pipeline
    fn strategy(&self) -> EvaluationStrategy;
}
