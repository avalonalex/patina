//! Test helpers for R7RS compliance testing
//!
//! Provides utilities for writing concise tests comparing Patina output
//! against expected values.
//!
//! When the `vm-backend` feature is enabled, all helpers use VmBackend
//! instead of the tree-walker. This allows running the full test suite
//! against the VM with:
//!
//!     cargo test --package patina-tests --features vm-backend

#![allow(dead_code)]

use patina_core::tagged_value::TaggedValue;
use patina_primitives::primitives::io::datum_writer::format_write_tagged;
use std::cell::RefCell;

// ─── Backend selection ───────────────────────────────────────────────────────

#[cfg(not(feature = "vm-backend"))]
use patina_interpreter::TreeWalkInterpreter;

#[cfg(feature = "vm-backend")]
use patina_interpreter::Interpreter;
#[cfg(feature = "vm-backend")]
use patina_runtime::Backend;
#[cfg(feature = "vm-backend")]
use patina_vm::VmBackend;

/// Format a TaggedValue for display (backend-agnostic).
fn display_tagged(tv: TaggedValue, heap: &RefCell<patina_core::heap::Heap>) -> String {
    // Unpack multiple values (R7RS: each value displayed on its own line)
    let vals = heap.borrow().get_values(tv).map(|v| v.to_vec());
    if let Some(vals) = vals {
        return vals
            .iter()
            .map(|v| format_write_tagged(*v, heap))
            .collect::<Vec<_>>()
            .join("\n");
    }
    format_write_tagged(tv, heap)
}

// ─── Tree-walker helpers (default) ───────────────────────────────────────────

#[cfg(not(feature = "vm-backend"))]
fn make_interp() -> TreeWalkInterpreter {
    TreeWalkInterpreter::new_tree_walker()
}

#[cfg(not(feature = "vm-backend"))]
fn interp_display(interp: &TreeWalkInterpreter, tv: TaggedValue) -> String {
    interp.display_tagged(tv)
}

// ─── VM backend helpers ──────────────────────────────────────────────────────

#[cfg(feature = "vm-backend")]
fn make_interp() -> Interpreter<VmBackend> {
    Interpreter::new(VmBackend::new())
}

#[cfg(feature = "vm-backend")]
fn interp_display(interp: &Interpreter<VmBackend>, tv: TaggedValue) -> String {
    let heap = interp.backend().global_env().heap();
    display_tagged(tv, heap)
}

// ─── Public helpers (backend-agnostic) ───────────────────────────────────────

/// Assert that evaluating a Scheme expression produces the expected result
pub fn assert_eval_to(expr: &str, expected: &str) {
    let interp = make_interp();
    let result = interp
        .eval_str(expr)
        .unwrap_or_else(|e| panic!("Failed to evaluate '{}': {}", expr, e));

    let result_str = interp_display(&interp, result);

    assert_eq!(
        result_str, expected,
        "\nExpression: {}\nExpected: {}\nGot: {}",
        expr, expected, result_str
    );
}

/// Assert that evaluating a Scheme expression produces an error
pub fn assert_eval_error(expr: &str) {
    let interp = make_interp();
    let result = interp.eval_str(expr);

    assert!(
        result.is_err(),
        "Expected error for '{}', but got: {:?}",
        expr,
        result.unwrap()
    );
}

/// Evaluate multiple expressions in sequence, return last result
pub fn eval_program(code: &str) -> String {
    let interp = make_interp();
    let result = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("Failed to evaluate program: {}", e));
    interp_display(&interp, result)
}

/// Assert that a multi-expression program produces expected result
pub fn assert_program_eval_to(code: &str, expected: &str) {
    let result = eval_program(code);
    assert_eq!(
        result, expected,
        "\nProgram:\n{}\nExpected: {}\nGot: {}",
        code, expected, result
    );
}

/// Assert that evaluating a multi-expression program produces an error
pub fn assert_program_eval_error(code: &str) {
    let interp = make_interp();
    let result = interp.eval_program(code);

    assert!(
        result.is_err(),
        "Expected error for program, but got: {:?}",
        result.unwrap()
    );
}

/// Assert that evaluating an expression with (scheme char) imported produces expected result
pub fn assert_eval_with_scheme_char(expr: &str, expected: &str) {
    let code = format!("(import (scheme char)) {}", expr);
    let interp = make_interp();
    let result = interp
        .eval_program(&code)
        .unwrap_or_else(|e| panic!("Failed to evaluate '{}': {}", expr, e));

    let result_str = interp_display(&interp, result);

    assert_eq!(
        result_str, expected,
        "\nExpression: {}\nExpected: {}\nGot: {}",
        expr, expected, result_str
    );
}

/// Assert that a multi-expression program produces expected result
/// This function now always uses CPS evaluation (the default mode).
/// The `use_cps` parameter is kept for backward compatibility but is ignored.
#[allow(unused_variables)]
pub fn assert_program_eval_to_with_cps(code: &str, expected: &str, use_cps: bool) {
    let interp = make_interp();
    let result = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("Failed to evaluate program: {}", e));
    let result_str = interp_display(&interp, result);
    assert_eq!(
        result_str, expected,
        "\nProgram:\n{}\nExpected: {}\nGot: {}",
        code, expected, result_str
    );
}
