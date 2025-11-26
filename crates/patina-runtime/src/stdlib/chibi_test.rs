//! (chibi test) - Minimal testing framework
//!
//! Provides basic testing functionality compatible with chibi-scheme's test suite.
//! Implemented as native Rust for simplicity and performance.
//!
//! Exports:
//! - test-begin: Start a test suite
//! - test-end: End a test suite
//! - test: Run a single test (implemented as syntax)

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Test state shared across test operations
#[derive(Debug, Default)]
struct TestState {
    suite_name: Option<String>,
    tests_run: usize,
    tests_passed: usize,
    tests_failed: usize,
}

thread_local! {
    static TEST_STATE: RefCell<TestState> = RefCell::new(TestState::default());
}

/// Build the (chibi test) library
pub fn build_chibi_test(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["chibi".to_string(), "test".to_string()];

    // test-begin: Start a test suite
    env.define(
        "test-begin".to_string(),
        Value::Procedure(Box::new(Procedure::Primitive {
            name: "test-begin",
            arity: Arity::Exact(1),
            library: library_name.clone(),
        })),
    );

    // test-end: End a test suite
    env.define(
        "test-end".to_string(),
        Value::Procedure(Box::new(Procedure::Primitive {
            name: "test-end",
            arity: Arity::Min(0),
            library: library_name.clone(),
        })),
    );

    // test-increment-passed: Increment passed count (internal)
    env.define(
        "test-increment-passed".to_string(),
        Value::Procedure(Box::new(Procedure::Primitive {
            name: "test-increment-passed",
            arity: Arity::Exact(0),
            library: library_name.clone(),
        })),
    );

    // test-increment-failed: Increment failed count (internal)
    env.define(
        "test-increment-failed".to_string(),
        Value::Procedure(Box::new(Procedure::Primitive {
            name: "test-increment-failed",
            arity: Arity::Exact(0),
            library: library_name.clone(),
        })),
    );

    // Note: 'test' is defined as a macro in bootstrap.scm and is globally available
    // We don't need to export it from this library since it's already available

    vec![
        "test-begin".to_string(),
        "test-end".to_string(),
        "test-increment-passed".to_string(),
        "test-increment-failed".to_string(),
    ]
}

/// Implementation for test-begin primitive
pub fn test_begin(name: &str) {
    TEST_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.suite_name = Some(name.to_string());
        state.tests_run = 0;
        state.tests_passed = 0;
        state.tests_failed = 0;

        println!("Running test suite: {}", name);
    });
}

/// Implementation for test-end primitive
pub fn test_end() {
    TEST_STATE.with(|state| {
        let state = state.borrow();
        println!(
            "Tests run: {}, Passed: {}, Failed: {}",
            state.tests_run, state.tests_passed, state.tests_failed
        );
        println!();
    });
}

/// Increment passed test count
pub fn test_increment_passed() {
    TEST_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.tests_run += 1;
        state.tests_passed += 1;
    });
}

/// Increment failed test count
pub fn test_increment_failed() {
    TEST_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.tests_run += 1;
        state.tests_failed += 1;
    });
}

/// Record a test result
#[allow(dead_code)]
pub fn record_test(passed: bool, expr_str: &str, expected: &Value, actual: &Value) {
    TEST_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.tests_run += 1;

        if passed {
            state.tests_passed += 1;
        } else {
            state.tests_failed += 1;
            println!("FAIL: {}", expr_str);
            println!("  expected {}", expected);
            println!("  but got {}", actual);
        }
    });
}
