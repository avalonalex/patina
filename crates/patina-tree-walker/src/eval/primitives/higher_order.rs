//! Higher-order function primitives (R7RS Section 6.4)
//!
//! NOTE: map and for-each are now implemented in Scheme (lib/scheme/base/higher_order.scm)
//! for CPS compatibility with call/cc. This file is kept for potential future use.

/// No-op register function - map and for-each are implemented in Scheme
pub(super) fn register(_registry: &mut super::PrimitiveRegistry) {
    // map and for-each are now in lib/scheme/base/higher_order.scm
}
