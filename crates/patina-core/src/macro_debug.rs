//! Unified macro and hygiene debug support
//!
//! This module provides the single debug flag for all macro-related tracing.
//! When enabled, prints detailed information about:
//! - Macro expansion steps (pattern matching, template expansion)
//! - Hygiene transformations (flip-scope operations)
//! - Scoped binding lookups (which candidates, which one wins)
//! - Scope set operations (flip, subset checks)
//! - Binding creation with scopes
//!
//! Enable via `(macro-debug-mode 'on)` in Scheme code.

use std::sync::atomic::{AtomicBool, Ordering};

// Single unified flag for all macro/hygiene debugging
static MACRO_DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Check if macro/hygiene debugging is enabled
pub fn is_enabled() -> bool {
    MACRO_DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Enable macro/hygiene debugging
pub fn enable() {
    MACRO_DEBUG_ENABLED.store(true, Ordering::Relaxed);
}

/// Disable macro/hygiene debugging
pub fn disable() {
    MACRO_DEBUG_ENABLED.store(false, Ordering::Relaxed);
}

/// Print a debug message (only if debugging is enabled)
#[macro_export]
macro_rules! macro_debug {
    ($($arg:tt)*) => {
        if $crate::macro_debug::is_enabled() {
            println!($($arg)*);
        }
    };
}
