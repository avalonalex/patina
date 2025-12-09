//! Standard R7RS Library Implementations
//!
//! This module contains builder functions for libraries.
//!
//! ## Architecture
//!
//! Libraries are organized in two layers:
//!
//! 1. **Internal libraries** `(patina internal ...)` - Domain-specific primitives in Rust
//!    - `(patina internal numbers)` - Arithmetic, comparison, transcendental (§6.2)
//!    - `(patina internal lists)` - Pairs and lists (§6.4)
//!    - `(patina internal chars)` - Characters (§6.6)
//!    - `(patina internal strings)` - Strings (§6.7)
//!    - `(patina internal vectors)` - Vectors (§6.8)
//!    - `(patina internal bytevectors)` - Bytevectors (§6.9)
//!    - `(patina internal control)` - Control features (§6.10)
//!    - `(patina internal errors)` - Exceptions (§6.11)
//!    - `(patina internal io)` - Input/output (§6.13)
//!    - `(patina internal predicates)` - Booleans, symbols, equality (§6.1, §6.3)
//!    - `(patina internal records)` - Record types (§5.5)
//!    - `(patina internal params)` - Parameters (§4.2.6)
//!    - `(patina internal time)` - Time operations
//!    - `(patina internal system)` - System/process operations
//!    - `(patina internal lazy)` - Lazy evaluation primitives
//!
//! 2. **R7RS libraries** `(scheme ...)` - Defined in .sld files that import from internal
//!    - `(scheme base)` - lib/scheme/base.sld
//!    - `(scheme char)` - lib/scheme/char.sld
//!    - etc.
//!
//! This separation allows:
//! - Clean domain organization regardless of R7RS library boundaries
//! - Multiple R7RS libraries to share primitives from the same domain
//! - Standard .sld format for all user-facing libraries

// === Internal libraries (patina internal ...) ===
mod internal_bytevectors;
mod internal_chars;
mod internal_control;
mod internal_errors;
mod internal_eval;
mod internal_io;
mod internal_lazy;
mod internal_lists;
mod internal_numbers;
mod internal_params;
mod internal_predicates;
mod internal_r5rs;
mod internal_records;
mod internal_strings;
mod internal_system;
mod internal_time;
mod internal_vectors;

pub use internal_bytevectors::build_internal_bytevectors;
pub use internal_chars::build_internal_chars;
pub use internal_control::build_internal_control;
pub use internal_errors::build_internal_errors;
pub use internal_eval::build_internal_eval;
pub use internal_io::build_internal_io;
pub use internal_lazy::build_internal_lazy;
pub use internal_lists::build_internal_lists;
pub use internal_numbers::build_internal_numbers;
pub use internal_params::build_internal_params;
pub use internal_predicates::build_internal_predicates;
pub use internal_r5rs::build_internal_r5rs;
pub use internal_records::build_internal_records;
pub use internal_strings::build_internal_strings;
pub use internal_system::build_internal_system;
pub use internal_time::build_internal_time;
pub use internal_vectors::build_internal_vectors;

// === User-facing utility libraries ===
mod chibi_test;
mod patina_debug;

pub use chibi_test::{
    build_chibi_test_primitives, test_begin, test_end, test_increment_error, test_increment_failed,
    test_increment_passed,
};
pub use patina_debug::build_patina_debug;
