//! Macro expansion debug support
//!
//! This module provides global flags to enable/disable macro expansion tracing.
//! When enabled, the macro system prints detailed information about:
//! - Pattern matching attempts (success/failure)
//! - Pattern variable bindings
//! - Template expansion steps
//! - Hygiene transformations

use std::sync::atomic::{AtomicBool, Ordering};

static MACRO_DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Check if macro debugging is enabled
pub fn is_enabled() -> bool {
    MACRO_DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Enable macro debugging
pub fn enable() {
    MACRO_DEBUG_ENABLED.store(true, Ordering::Relaxed);
}

/// Disable macro debugging
pub fn disable() {
    MACRO_DEBUG_ENABLED.store(false, Ordering::Relaxed);
}

/// Print a macro debug message (only if debugging is enabled)
#[macro_export]
macro_rules! macro_debug {
    ($($arg:tt)*) => {
        if $crate::macro_debug::is_enabled() {
            println!($($arg)*);
        }
    };
}
