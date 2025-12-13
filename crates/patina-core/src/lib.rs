//! Patina Core - Foundation types for Patina Scheme interpreter
//!
//! This crate provides the foundational data types shared across all Patina components:
//! - `Value`: The Scheme value representation (numbers, lists, procedures, etc.)
//! - `Environment`: Lexical environment for variable bindings
//! - `CoreExpr`: Core intermediate representation for evaluation
//! - `ScopeId`, `ScopeSet`: Scope tracking for macro hygiene
//! - `PVRef`, `MatchEnv`: Pattern variable references for macro expansion
//! - `Library`: R7RS library representation
//! - `CompiledMacro`: Compiled syntax-rules macros
//! - `ErrorKind`, `ErrorDetail`: Unified error handling
//!
//! By placing these types in a foundation crate, we avoid circular dependencies
//! and enable type-safe representations (no `dyn Any` needed).

pub mod compiled_macro;
pub mod core_expr;
pub mod cps_expr;
pub mod debug_format;
pub mod environment;
pub mod error;
pub mod library;
pub mod macro_debug;
pub mod numeric;
pub mod port;
pub mod pvref;
pub mod scope;
pub mod value;

// Re-export main types for convenience
pub use compiled_macro::{
    CompiledMacro, CompiledRule, Identifier, LiteralBinding, Pattern, Template,
};
pub use core_expr::{CoreExpr, Formals, ScopedParam, Symbol};
pub use cps_expr::{CpsExpr, CpsParam, CpsPrimitive, PromptTag};
pub use environment::{Environment, ScopedBinding};
pub use error::{ErrorDetail, ErrorKind, SourceLocation};
pub use library::Library;
pub use port::{Port, PortData, PortDirection, PortKind, StdioKind, StringPortData};
pub use pvref::{MatchEnv, MatchValue, PVRef};
pub use scope::{ScopeId, ScopeSet};
pub use value::{
    Arity, ExceptionKind, ExceptionObject, IdentifierData, LambdaBody, Procedure, PromiseState,
    RecordTypeDescriptor, Value,
};

#[cfg(test)]
pub use scope::reset_scope_counter;
