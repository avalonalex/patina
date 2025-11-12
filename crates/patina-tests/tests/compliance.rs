//! R7RS Compliance Tests
//!
//! This test suite is organized to mirror the R7RS specification structure.
//! Each module corresponds to a section of the spec.
//!
//! Reference: https://small.r7rs.org/attachment/r7rs.pdf

#[path = "common/mod.rs"]
mod common;

// Section 4.1: Primitive expression types
#[path = "compliance/primitives.rs"]
mod primitives;

// Section 4.2: Derived expression types
#[path = "compliance/derived.rs"]
mod derived;

// Section 6.2: Numbers
#[path = "compliance/numbers.rs"]
mod numbers;

// Section 6.2.1: Rational Numbers (comprehensive tests)
#[path = "compliance/rationals.rs"]
mod rationals;

// Section 6.4: Pairs and lists
#[path = "compliance/lists.rs"]
mod lists;

// Section 6.x: Predicates
#[path = "compliance/predicates.rs"]
mod predicates;

// Section 6.7: Strings
#[path = "compliance/strings.rs"]
mod strings;

// Section 6.8: Vectors
#[path = "compliance/vectors.rs"]
mod vectors;

// Section 6.10: Control features
#[path = "compliance/control.rs"]
mod control;

// Section 4.2.8: Quasiquotation
#[path = "compliance/quasiquote.rs"]
mod quasiquote;

// Section 4.3: Advanced Macros (comprehensive macro system tests)
#[path = "compliance/macros_advanced.rs"]
mod macros_advanced;
