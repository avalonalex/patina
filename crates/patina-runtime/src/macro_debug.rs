//! Macro expansion debug support
//!
//! This module re-exports the unified macro debug from patina-core.
//! When enabled, the macro system prints detailed information about:
//! - Pattern matching attempts (success/failure)
//! - Pattern variable bindings
//! - Template expansion steps
//! - Hygiene transformations
//! - Environment scoped lookups
//!
//! Enable via `(macro-debug-mode 'on)` in Scheme code.

// Re-export everything from patina-core's macro_debug
// This keeps backwards compatibility while having a single source of truth
pub use patina_core::macro_debug::*;
