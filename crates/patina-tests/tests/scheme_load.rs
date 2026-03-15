//! Tests for R7RS (scheme load) and (scheme repl) libraries

mod common;
use common::*;

// Resolve path to test resource files relative to the test crate root
fn resource_path(name: &str) -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{}/resources/load-test/{}", manifest, name)
}

// =============================================================================
// (scheme load) — basic load
// =============================================================================

#[test]
fn test_load_defines_variables() {
    let path = resource_path("definitions.scm");
    let code = format!(
        r#"
        (import (scheme load))
        (load "{}")
        loaded-x
        "#,
        path
    );
    assert_program_eval_to(&code, "42");
}

#[test]
fn test_load_defines_string() {
    let path = resource_path("definitions.scm");
    let code = format!(
        r#"
        (import (scheme load))
        (load "{}")
        loaded-y
        "#,
        path
    );
    assert_program_eval_to(&code, "\"hello\"");
}

#[test]
fn test_load_defines_procedures() {
    let path = resource_path("definitions.scm");
    let code = format!(
        r#"
        (import (scheme load))
        (load "{}")
        (loaded-add 10 20)
        "#,
        path
    );
    assert_program_eval_to(&code, "30");
}

#[test]
fn test_load_multiple_expressions() {
    let path = resource_path("multiple-exprs.scm");
    let code = format!(
        r#"
        (import (scheme load))
        (load "{}")
        expr-count
        "#,
        path
    );
    assert_program_eval_to(&code, "3");
}

#[test]
fn test_load_side_effects() {
    let path = resource_path("side-effects.scm");
    let code = format!(
        r#"
        (import (scheme load))
        (define side-effect-counter 0)
        (load "{}")
        side-effect-counter
        "#,
        path
    );
    assert_program_eval_to(&code, "1");
}

#[test]
fn test_load_multiple_times() {
    let path = resource_path("side-effects.scm");
    let code = format!(
        r#"
        (import (scheme load))
        (define side-effect-counter 0)
        (load "{path}")
        (load "{path}")
        (load "{path}")
        side-effect-counter
        "#,
        path = path
    );
    assert_program_eval_to(&code, "3");
}

// =============================================================================
// (scheme load) — error cases
// =============================================================================

#[test]
fn test_load_nonexistent_file() {
    assert_program_eval_error(
        r#"
        (import (scheme load))
        (load "/tmp/patina_nonexistent_file_12345.scm")
        "#,
    );
}

// =============================================================================
// (scheme repl) — interaction-environment
// =============================================================================

#[test]
fn test_interaction_environment_returns_environment() {
    let code = r#"
        (import (scheme repl) (scheme eval))
        (eval '(+ 1 2) (interaction-environment))
    "#;
    assert_program_eval_to(code, "3");
}

#[test]
fn test_interaction_environment_is_mutable() {
    let code = r#"
        (import (scheme repl) (scheme eval))
        (eval '(define ie-test-var 99) (interaction-environment))
        ie-test-var
    "#;
    assert_program_eval_to(code, "99");
}

#[test]
fn test_load_with_interaction_environment() {
    let path = resource_path("definitions.scm");
    let code = format!(
        r#"
        (import (scheme load) (scheme repl))
        (load "{}" (interaction-environment))
        loaded-x
        "#,
        path
    );
    assert_program_eval_to(&code, "42");
}

// =============================================================================
// (scheme load) — load returns unspecified
// =============================================================================

#[test]
fn test_load_result_is_not_error() {
    // load should succeed without error; we just check it doesn't crash
    let path = resource_path("definitions.scm");
    let code = format!(
        r#"
        (import (scheme load))
        (load "{}")
        (loaded-add 1 2)
        "#,
        path
    );
    assert_program_eval_to(&code, "3");
}
