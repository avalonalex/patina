//! Error types for pipeline execution

use thiserror::Error;

/// Errors that can occur during pipeline execution
#[derive(Error, Debug)]
pub enum PipelineError {
    /// Frontend error (lexing, parsing)
    #[error("Frontend error: {0}")]
    Frontend(#[from] patina_frontend::error::FrontendError),

    /// Macro expansion error
    #[error("Macro expansion error: {0}")]
    MacroExpansion(#[from] patina_macros::MacroError),

    /// Desugaring error (CoreExpr conversion)
    #[error("Desugaring error: {0}")]
    Desugaring(String),

    /// Evaluation error
    #[error("Evaluation error: {0}")]
    Evaluation(String),

    /// Configuration error
    #[error("Pipeline configuration error: {0}")]
    Configuration(String),
}

/// Result type for pipeline operations
pub type PipelineResult<T> = Result<T, PipelineError>;
