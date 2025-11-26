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
//!
//! By placing these types in a foundation crate, we avoid circular dependencies
//! and enable type-safe representations (no `dyn Any` needed).

pub mod compiled_macro;
pub mod core_expr;
pub mod environment;
pub mod library;
pub mod pvref;
pub mod scope;
pub mod value;

// Re-export main types for convenience
// Note: CaseLambdaClause exists in both core_expr (IR) and value (runtime).
// Use explicit paths: core_expr::CaseLambdaClause or value::CaseLambdaClause
pub use compiled_macro::{CompiledMacro, CompiledRule, Identifier, Pattern, Template};
pub use core_expr::{CoreExpr, Formals, Primitive, Symbol};
pub use environment::{Environment, ScopedBinding};
pub use library::Library;
pub use pvref::{MatchEnv, MatchValue, PVRef};
pub use scope::{ScopeId, ScopeSet};
pub use value::{Arity, IdentifierData, LambdaBody, Procedure, PromiseState, Value};

#[cfg(test)]
pub use scope::reset_scope_counter;
