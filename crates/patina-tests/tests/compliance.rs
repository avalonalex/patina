//! R7RS Compliance Tests
//!
//! This test suite is organized to mirror the R7RS specification structure.
//! Each module corresponds to a section of the spec.
//!
//! Reference: https://small.r7rs.org/attachment/r7rs.pdf

#[path = "common/mod.rs"]
mod common;

// Section 4.3: Advanced Macros (comprehensive macro system tests)
#[path = "compliance/macros_advanced.rs"]
mod macros_advanced;
