//! Numeric error types for the Scheme numeric tower (R7RS Section 6.2)
//!
//! All numeric operations are now implemented directly on the Heap via TaggedValue
//! in `heap/numeric.rs`. This module retains only the shared error type.

mod error;

pub use error::NumericError;
