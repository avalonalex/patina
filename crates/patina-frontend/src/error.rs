use thiserror::Error;

/// Frontend errors (lexing, parsing, macro expansion)
#[derive(Error, Debug, Clone)]
pub enum FrontendError {
    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    #[error("Lexer error: {0}")]
    LexError(String),

    #[error("Parser error: {0}")]
    ParseError(String),

    #[error("Macro expansion error: {0}")]
    MacroError(String),

    #[error("Type error: {0}")]
    TypeError(String),

    #[error("Frontend error: {0}")]
    General(String),
}
