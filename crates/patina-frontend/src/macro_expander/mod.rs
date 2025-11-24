//! Macro expander test utilities
//!
//! This module provides test helpers for macro expansion.
//! The actual macro system is in `patina-macros`.

mod interface;

// Re-export test helper
pub use interface::TestExpander;
