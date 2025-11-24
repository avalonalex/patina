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
// Note: Most special forms are now handled by CoreExpr evaluator
// Only forms not yet in CoreExpr remain here:
mod case_lambda; // Not in CoreExpr yet
mod expand; // Patina debugging extension, not in CoreExpr
mod let_syntax; // Deferred to future (GitHub issue)

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

    // Note: Most R7RS special forms are now handled exclusively by CoreExpr evaluator
    // (quote, if, begin, define, set!, lambda, apply, quasiquote, define-syntax,
    //  import, parameterize)
    //
    // Only forms not yet migrated to CoreExpr are registered here:

    // R7RS special forms still using Value evaluator
    registry.register(Box::new(let_syntax::LetSyntaxForm));
    registry.register(Box::new(let_syntax::LetrecSyntaxForm));

    // R7RS (scheme case-lambda) special form
    registry.register(Box::new(case_lambda::CaseLambdaForm));

    // Patina debugging extensions
    registry.register(Box::new(expand::ExpandForm));

    registry
}
