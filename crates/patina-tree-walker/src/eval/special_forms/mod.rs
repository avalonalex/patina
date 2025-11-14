//! Special forms module
//!
//! This module contains all special form implementations organized as a registry system.
//! Each special form is implemented in its own file and registered with the registry.
//!
//! Special forms are syntactic constructs that don't evaluate their arguments in the
//! normal way. They have full control over evaluation order, can introduce new bindings,
//! and can prevent evaluation of some arguments entirely.
//!
//! # Architecture
//!
//! - `trait.rs` - The `SpecialForm` trait that all forms implement
//! - `registry.rs` - The `SpecialFormRegistry` for managing forms
//! - Individual form files (quote.rs, if.rs, etc.) - Each form's implementation
//!
//! # Adding a New Special Form
//!
//! 1. Create a new file (e.g., `myform.rs`)
//! 2. Implement the `SpecialForm` trait
//! 3. Add `mod myform;` below
//! 4. Register it in `build_registry()`
//!
//! # Example
//!
//! ```ignore
//! // In myform.rs:
//! pub struct MyForm;
//!
//! impl SpecialForm for MyForm {
//!     fn name(&self) -> &'static str { "myform" }
//!     fn eval(...) -> Result<EvalResult, EvalError> { ... }
//! }
//!
//! // In mod.rs:
//! mod myform;
//! use myform::MyForm;
//!
//! pub fn build_registry() -> SpecialFormRegistry {
//!     let mut registry = SpecialFormRegistry::new();
//!     // ...
//!     registry.register(Box::new(MyForm));
//!     registry
//! }
//! ```

// Core modules
#[path = "trait.rs"]
mod trait_def; // Renamed to avoid keyword collision

pub mod registry;

// Re-exports
pub use registry::SpecialFormRegistry;
pub use trait_def::SpecialForm;

// Individual special form implementations
mod apply;
mod begin;
mod define;
mod define_syntax;
mod expand;
mod r#if;
mod import;
mod lambda;
mod quasiquote;
mod quote;
mod set;

/// Build and populate the special form registry with all standard forms
///
/// This function creates a new registry and registers all R7RS special forms.
/// It's called during evaluator initialization.
///
/// # Returns
///
/// A fully populated `SpecialFormRegistry` ready for use.
///
/// # Example
///
/// ```ignore
/// let registry = build_registry();
/// assert!(registry.contains("quote"));
/// assert!(registry.contains("if"));
/// ```
pub fn build_registry() -> SpecialFormRegistry {
    let mut registry = SpecialFormRegistry::new();

    // Register all special forms here as we migrate them
    registry.register(Box::new(quote::QuoteForm));
    registry.register(Box::new(expand::ExpandForm));
    registry.register(Box::new(r#if::IfForm));
    registry.register(Box::new(begin::BeginForm));
    registry.register(Box::new(define::DefineForm));
    registry.register(Box::new(set::SetForm));
    registry.register(Box::new(lambda::LambdaForm));
    registry.register(Box::new(apply::ApplyForm));
    registry.register(Box::new(import::ImportForm));
    registry.register(Box::new(define_syntax::DefineSyntaxForm));
    registry.register(Box::new(quasiquote::QuasiquoteForm));

    registry
}
