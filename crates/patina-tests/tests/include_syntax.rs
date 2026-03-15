//! Tests for R7RS expression-level `include`, `include-ci`, and `syntax-error`

mod common;
use common::*;

// Resolve path to test resource files relative to the test crate root
fn resource_path(name: &str) -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{}/resources/load-test/{}", manifest, name)
}

// =============================================================================
// include — expression-level file inclusion
// =============================================================================

#[test]
fn test_include_defines_variable() {
    let path = resource_path("include-defs.scm");
    let code = format!(
        r#"
        (include "{}")
        included-val
        "#,
        path
    );
    assert_program_eval_to(&code, "77");
}

#[test]
fn test_include_defines_procedure() {
    let path = resource_path("include-defs.scm");
    let code = format!(
        r#"
        (include "{}")
        (included-fn 5)
        "#,
        path
    );
    assert_program_eval_to(&code, "10");
}

#[test]
fn test_include_multiple_files() {
    let path1 = resource_path("include-defs.scm");
    let path2 = resource_path("definitions.scm");
    let code = format!(
        r#"
        (include "{}" "{}")
        (+ included-val loaded-x)
        "#,
        path1, path2
    );
    assert_program_eval_to(&code, "119"); // 77 + 42
}

#[test]
fn test_include_in_begin() {
    let path = resource_path("include-defs.scm");
    let code = format!(
        r#"
        (begin
          (include "{}")
          (included-fn 3))
        "#,
        path
    );
    assert_program_eval_to(&code, "6");
}

#[test]
fn test_include_nonexistent_file_error() {
    assert_program_eval_error(r#"(include "/tmp/patina_nonexistent_file_12345.scm")"#);
}

// =============================================================================
// include-ci — case-insensitive file inclusion
// =============================================================================

#[test]
fn test_include_ci_folds_case() {
    let path = resource_path("include-ci-defs.scm");
    let code = format!(
        r#"
        (include-ci "{}")
        ci-included-val
        "#,
        path
    );
    assert_program_eval_to(&code, "\"folded\"");
}

#[test]
fn test_include_ci_procedure() {
    let path = resource_path("include-ci-defs.scm");
    let code = format!(
        r#"
        (include-ci "{}")
        (ci-included-fn "test")
        "#,
        path
    );
    assert_program_eval_to(&code, "\"test-ok\"");
}

// =============================================================================
// syntax-error — compile-time error signaling
// =============================================================================

#[test]
fn test_syntax_error_basic() {
    assert_program_eval_error(r#"(syntax-error "this should fail")"#);
}

#[test]
fn test_syntax_error_with_irritants() {
    assert_program_eval_error(r#"(syntax-error "bad" 'x 42)"#);
}

#[test]
fn test_syntax_error_in_cond_macro() {
    // The cond macro uses syntax-error for invalid (else => proc) patterns
    assert_program_eval_error(
        r#"
        (import (scheme base))
        (cond (else => display))
        "#,
    );
}

#[test]
fn test_syntax_error_not_reached() {
    // syntax-error in an unselected cond-expand branch should not fire
    let code = r#"
        (cond-expand
          (r7rs 42)
          (else (syntax-error "should not reach here")))
    "#;
    assert_program_eval_to(code, "42");
}
