//! Integration tests for (scheme process-context) library
//!
//! R7RS (scheme process-context) procedures:
//! - command-line: Get command line arguments
//! - exit: Exit program (with handlers)
//! - emergency-exit: Exit immediately
//! - get-environment-variable: Get single env var
//! - get-environment-variables: Get all env vars

mod common;
use common::assert_program_eval_to;

// ============================================================================
// command-line tests
// ============================================================================

#[test]
fn test_command_line_returns_list() {
    // command-line should return a list
    assert_program_eval_to(
        "(import (scheme process-context))
         (list? (command-line))",
        "#t",
    );
}

#[test]
fn test_command_line_first_element_is_string() {
    // First element should be a string (the program name)
    assert_program_eval_to(
        "(import (scheme process-context))
         (string? (car (command-line)))",
        "#t",
    );
}

// ============================================================================
// get-environment-variable tests
// ============================================================================

#[test]
fn test_get_environment_variable_path() {
    // PATH should exist on most systems and be a string
    assert_program_eval_to(
        "(import (scheme process-context))
         (string? (get-environment-variable \"PATH\"))",
        "#t",
    );
}

#[test]
fn test_get_environment_variable_not_found() {
    // Non-existent variable should return #f
    assert_program_eval_to(
        "(import (scheme process-context))
         (get-environment-variable \"PATINA_NONEXISTENT_VAR_12345\")",
        "#f",
    );
}

// Note: Can't test type error without guard, which isn't implemented yet
// #[test]
// fn test_get_environment_variable_type_error() {
//     // Should require a string argument
//     assert_program_eval_to(
//         "(import (scheme process-context))
//          (guard (e (#t 'type-error))
//            (get-environment-variable 123))",
//         "type-error",
//     );
// }

// ============================================================================
// get-environment-variables tests
// ============================================================================

#[test]
fn test_get_environment_variables_returns_list() {
    // Should return a list
    assert_program_eval_to(
        "(import (scheme process-context))
         (list? (get-environment-variables))",
        "#t",
    );
}

#[test]
fn test_get_environment_variables_returns_alist() {
    // Each entry should be a pair (alist entry)
    assert_program_eval_to(
        "(import (scheme process-context))
         (let ((vars (get-environment-variables)))
           (if (null? vars)
               #t  ; empty list is trivially valid
               (pair? (car vars))))",
        "#t",
    );
}

#[test]
fn test_get_environment_variables_string_keys() {
    // Keys should be strings
    assert_program_eval_to(
        "(import (scheme process-context))
         (let ((vars (get-environment-variables)))
           (if (null? vars)
               #t
               (string? (car (car vars)))))",
        "#t",
    );
}

#[test]
fn test_get_environment_variables_string_values() {
    // Values should be strings
    assert_program_eval_to(
        "(import (scheme process-context))
         (let ((vars (get-environment-variables)))
           (if (null? vars)
               #t
               (string? (cdr (car vars)))))",
        "#t",
    );
}

// ============================================================================
// exit tests (can't actually test exit behavior in unit tests)
// ============================================================================

#[test]
fn test_exit_exists() {
    // Verify exit is a procedure
    assert_program_eval_to(
        "(import (scheme process-context))
         (procedure? exit)",
        "#t",
    );
}

#[test]
fn test_emergency_exit_exists() {
    // Verify emergency-exit is a procedure
    assert_program_eval_to(
        "(import (scheme process-context))
         (procedure? emergency-exit)",
        "#t",
    );
}

// ============================================================================
// R7RS spec compliance tests
// ============================================================================

#[test]
fn test_r7rs_environment_lookup() {
    // PATH should exist on most systems
    assert_program_eval_to(
        "(import (scheme process-context))
         (string? (get-environment-variable \"PATH\"))",
        "#t",
    );
}

#[test]
fn test_command_line_at_least_one_element() {
    // R7RS: The first element is an implementation-dependent string
    // identifying the calling program (may be empty)
    assert_program_eval_to(
        "(import (scheme process-context))
         (>= (length (command-line)) 1)",
        "#t",
    );
}
