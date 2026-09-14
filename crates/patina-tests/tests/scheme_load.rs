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

// =============================================================================
// (scheme load) — a file cut short inside a datum (#329)
// =============================================================================

#[test]
fn test_load_rejects_a_file_cut_short_inside_a_datum() {
    let path = resource_path("truncated.scm");
    let code = format!(r#"(import (scheme load)) (load "{}")"#, path);
    assert_program_eval_error(&code);
    for result in [
        try_eval_program_vm(&code),
        try_eval_program_tree_walker(&code),
    ] {
        let err = result.expect_err("a file cut short must not load");
        assert!(
            err.contains("truncated.scm") && err.contains("line 4, column 1"),
            "{err}"
        );
    }
}

#[test]
fn test_load_runs_the_forms_before_the_cut_and_raises_at_it() {
    let path = resource_path("truncated.scm");
    let code = format!(
        r#"
        (import (scheme base) (scheme load))
        (define outcome (guard (e (#t 'raised)) (load "{}")))
        (list outcome loaded-before-cut)
        "#,
        path
    );
    assert_program_eval_to(&code, "(raised 42)");
}

/// `load` puts the path it was given into its parse errors, so a path that
/// happens to contain `file` must not turn the read error into a file error.
#[test]
fn test_load_of_a_cut_short_file_raises_a_read_error_whatever_the_path() {
    let path = resource_path("truncated.scm");
    assert!(path.contains("load-test"), "path under test: {path}");
    let code = format!(
        r#"
        (import (scheme base) (scheme load))
        (guard (e ((read-error? e) 'read-error) ((file-error? e) 'file-error) (#t 'other))
          (load "{}"))
        "#,
        path
    );
    assert_program_eval_to(&code, "read-error");
}
