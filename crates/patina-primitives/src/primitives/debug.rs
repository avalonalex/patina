//! Debug primitives stub
//!
//! Debug primitives (debug-enable, debug-disable, etc.) require backend-specific
//! access to internal debug configuration. They are registered by each backend
//! separately. This stub exists only so the module is compiled; it registers nothing.

use crate::registry::PrimitiveRegistry;

/// Register debug primitives (stub — backends register their own debug prims)
pub(super) fn register(_registry: &mut PrimitiveRegistry) {
    // Debug primitives are backend-specific; each backend registers its own.
}
