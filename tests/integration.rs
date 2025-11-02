//! Integration Tests
//!
//! End-to-end tests that verify complete workflows and interactions.

// Chibi-scheme comparison tests
#[path = "integration/comparison_test.rs"]
mod comparison_test;

// File runner tests
#[path = "integration/file_runner.rs"]
mod file_runner;

// Scheme runner infrastructure tests
#[path = "integration/scheme_runner.rs"]
mod scheme_runner;
