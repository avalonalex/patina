use thiserror::Error;

/// Runtime errors that can occur during evaluation
#[derive(Error, Debug, Clone)]
pub enum RuntimeError {
    #[error("Type error: {0}")]
    TypeError(String),

    #[error("Arity error: {0}")]
    ArityError(String),

    #[error("Undefined variable: {0}")]
    UndefinedVariable(String),

    #[error("Division by zero")]
    DivisionByZero,

    #[error("Index out of bounds: {0}")]
    IndexOutOfBounds(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Runtime error: {0}")]
    General(String),
}
