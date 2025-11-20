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
mod case_lambda;
mod define;
mod define_syntax;
mod expand;
mod r#if;
mod import;
mod lambda;
mod let_syntax;
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

    // TODO: Organize special forms by library ownership
    //
    // Currently all special forms are registered globally during evaluator initialization.
    // Ideally, special forms should be organized by which library they belong to:
    //
    // - (scheme base): quote, if, begin, define, set!, lambda, apply, import,
    //                  define-syntax, quasiquote
    // - (patina debug): expand (Patina-specific debugging feature)
    //
    // Challenges:
    // 1. Special forms must be available before libraries are loaded (bootstrap problem)
    // 2. Library loading itself requires special forms (import, define-syntax)
    // 3. Core evaluation requires special forms (if, lambda, define, etc.)
    //
    // Potential approaches:
    //
    // Approach 1 (Simple - Code organization only):
    //   - Split registration into functions: register_scheme_base_forms(),
    //     register_patina_debug_forms()
    //   - Keep all forms globally available
    //   - Pro: Clear organization, no behavioral changes
    //   - Con: expand is always available (even without loading (patina debug))
    //
    // Approach 2 (Library-conditional registration):
    //   - Register core R7RS forms globally at initialization
    //   - Register extension forms (expand) only when their library is loaded
    //   - Requires special form registry to be mutable after initialization
    //   - Pro: True library isolation for extensions
    //   - Con: More complex, special forms become library-dependent
    //
    // Approach 3 (Two-tier registry):
    //   - Core registry: Forms needed for bootstrap (if, define, lambda, etc.)
    //   - Library registry: Forms provided by libraries (expand, future extensions)
    //   - Check core registry first, then library registry
    //   - Pro: Clear separation, allows library-specific forms
    //   - Con: Most complex implementation

    // R7RS (scheme base) special forms
    registry.register(Box::new(quote::QuoteForm));
    registry.register(Box::new(r#if::IfForm));
    registry.register(Box::new(begin::BeginForm));
    registry.register(Box::new(define::DefineForm));
    registry.register(Box::new(set::SetForm));
    registry.register(Box::new(lambda::LambdaForm));
    registry.register(Box::new(apply::ApplyForm));
    registry.register(Box::new(import::ImportForm));
    registry.register(Box::new(define_syntax::DefineSyntaxForm));
    registry.register(Box::new(let_syntax::LetSyntaxForm));
    registry.register(Box::new(let_syntax::LetrecSyntaxForm));
    registry.register(Box::new(quasiquote::QuasiquoteForm));

    // R7RS (scheme case-lambda) special forms
    registry.register(Box::new(case_lambda::CaseLambdaForm));

    // Patina debugging extensions
    // TODO: Move to (patina debug) library when we implement library-specific special forms
    registry.register(Box::new(expand::ExpandForm));

    registry
}
