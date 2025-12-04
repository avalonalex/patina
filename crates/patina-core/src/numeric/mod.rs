//! Numeric operations for the Scheme numeric tower (R7RS Section 6.2)
//!
//! This module provides numeric operations as methods on `Value`, implementing:
//! - Automatic type promotion (Integer → BigInteger → Rational → Real → Complex)
//! - Exactness contagion (inexact + exact = inexact)
//! - NaN propagation (any NaN operand produces NaN result)
//! - R7RS-compliant semantics for all numeric operations
//!
//! # Design
//!
//! Operations are implemented directly on `Value` to avoid conversion overhead.
//! The numeric tower hierarchy is:
//!
//! ```text
//! Integer (i64)
//!    ↓ overflow
//! BigInteger (arbitrary precision)
//!    ↓ division
//! Rational (exact fraction)
//!    ↓ inexact operation
//! Real (f64)
//!    ↓ complex operation
//! Complex (real + imaginary, each preserving exactness)
//! ```

mod arithmetic;
mod comparison;
mod complex;
mod error;
mod exactness;
mod integer_div;
mod predicates;
mod rounding;
mod transcendental;

pub use error::NumericError;

// Re-export key functions that aren't methods
pub use complex::{make_polar, make_rectangular};
pub use exactness::{apply_contagion, result_exactness, result_exactness_many, Exactness};
pub use integer_div::{numeric_gcd, numeric_gcd_many, numeric_lcm, numeric_lcm_many};
pub use rounding::bigint_to_value;
