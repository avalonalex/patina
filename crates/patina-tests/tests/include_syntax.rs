//! Tests for R7RS expression-level `include`, `include-ci`, and `syntax-error`

mod common;
use common::*;
use tempfile::TempDir;

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

// ---------------------------------------------------------------------------
// From `larceny_families.rs` (Larceny family 1), moved here by #193 Phase 1
// because this is the file about `include`. It stays in Rust rather than
// joining the `.scm` suite for the reason the driver's header gives: it writes
// files to a temporary directory and includes them by path, and the driver
// evaluates a file's *text*, so a relative include has no directory to resolve
// against.
// ---------------------------------------------------------------------------

/// `outer.scm` (included by absolute path) includes `sub/middle.scm` by
/// absolute path, and `middle.scm` includes `"leaf.scm"` relatively. Every
/// implementation that runs Larceny's `base` suite resolves that last one
/// beside `middle.scm`; Patina used to look in the first file the source map
/// happened to yield, then the cwd, and find nothing.
///
/// Fixed 2026-08-24: the desugarer keeps a stack of include directories.
#[test]
fn a_nested_include_resolves_relative_to_the_including_file() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
    let middle = scratch_path(&dir, "sub/middle.scm");
    std::fs::write(
        dir.path().join("outer.scm"),
        format!("(include \"{middle}\")"),
    )
    .expect("write outer");
    std::fs::write(&middle, "(include \"leaf.scm\")").expect("write middle");
    std::fs::write(
        dir.path().join("sub/leaf.scm"),
        "(define leaf-value 'found)",
    )
    .expect("write leaf");

    let program = format!(
        "(include \"{}\") leaf-value",
        scratch_path(&dir, "outer.scm")
    );
    assert_program_eval_to(&program, "found");
}
