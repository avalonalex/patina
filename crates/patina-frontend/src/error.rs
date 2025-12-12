use patina_core::{ErrorDetail, ErrorKind};
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

impl FrontendError {
    /// Get the error kind for classification
    pub fn to_error_kind(&self) -> ErrorKind {
        match self {
            FrontendError::InvalidSyntax(_) => ErrorKind::Syntax,
            FrontendError::LexError(_) => ErrorKind::Read,
            FrontendError::ParseError(_) => ErrorKind::Read,
            FrontendError::MacroError(_) => ErrorKind::Syntax,
            FrontendError::TypeError(_) => ErrorKind::Type,
            FrontendError::General(_) => ErrorKind::Internal,
        }
    }

    /// Convert to ErrorDetail for rich error reporting
    ///
    /// Note: Source location will be None until SOURCE_INFO_PLAN.md is implemented.
    pub fn to_error_detail(&self) -> ErrorDetail {
        ErrorDetail::new(self.to_error_kind(), self.to_string())
    }
}

// Conversions from specific error types
impl From<crate::lexer::LexError> for FrontendError {
    fn from(e: crate::lexer::LexError) -> Self {
        FrontendError::LexError(e.to_string())
    }
}

impl From<crate::parser::ParseError> for FrontendError {
    fn from(e: crate::parser::ParseError) -> Self {
        FrontendError::ParseError(e.to_string())
    }
}

// Conversion to ErrorDetail
impl From<FrontendError> for ErrorDetail {
    fn from(e: FrontendError) -> Self {
        e.to_error_detail()
    }
}
