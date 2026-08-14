//! End-to-end tests for loading .sld library files
//!
//! These tests verify that the complete library loading pipeline works:
//! - Finding .sld files in search paths
//! - Parsing define-library forms
//! - Resolving imports
//! - Evaluating library bodies
//! - Collecting exports

mod common;

use patina_tree_walker::Evaluator;
use std::path::PathBuf;

/// Get the test resources directory
fn test_resources_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("test-libraries")
}

/// Create an evaluator configured for testing
fn test_evaluator() -> Evaluator {
    let eval = Evaluator::new();
    // Add test resources directory to search path
    eval.add_library_search_path(test_resources_dir());
    eval
}

#[test]
fn test_load_simple_library() {
    let eval = test_evaluator();

    // Load the (test simple) library
    let lib = eval
        .load_library(&["test".to_string(), "simple".to_string()])
        .expect("Failed to load (test simple) library");

    // Verify library name
    assert_eq!(lib.name, vec!["test", "simple"]);

    // Verify exports
    assert!(lib.exports_identifier("double"));
    assert!(lib.exports_identifier("triple"));
    assert!(lib.exports_identifier("add-one"));

    // Verify export count (should only export what's listed)
    assert_eq!(lib.export_names().len(), 3);

    // Verify library is now in registry
    assert!(eval.is_library_loaded(&["test".to_string(), "simple".to_string()]));
}

#[test]
fn test_library_with_renamed_export() {
    let eval = test_evaluator();

    // Load library with renamed export
    let lib = eval
        .load_library(&["test".to_string(), "with-rename".to_string()])
        .expect("Failed to load (test with-rename) library");

    // Verify exports
    assert!(lib.exports_identifier("square"));
    assert!(lib.exports_identifier("cube")); // Renamed from internal-cube
    assert!(!lib.exports_identifier("internal-cube")); // Internal name not exported

    assert_eq!(lib.export_names().len(), 2);
}

#[test]
fn test_library_not_found() {
    let eval = test_evaluator();

    // Try to load non-existent library
    let result = eval.load_library(&["nonexistent".to_string(), "library".to_string()]);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[test]
fn test_library_cached_after_load() {
    let eval = test_evaluator();

    // Load library first time
    let lib1 = eval
        .load_library(&["test".to_string(), "simple".to_string()])
        .unwrap();

    // Load same library again - should return cached version
    let lib2 = eval
        .load_library(&["test".to_string(), "simple".to_string()])
        .unwrap();

    // Should be the same library (both point to same data)
    assert_eq!(lib1.name, lib2.name);
    assert_eq!(lib1.export_names(), lib2.export_names());
}

#[test]
fn test_library_search_paths() {
    let eval = test_evaluator();

    // Verify search paths include both default and test directory
    let paths = eval.library_search_paths();

    // Should have at least ./lib in search paths
    assert!(!paths.is_empty());

    // At least one path should end with "lib"
    assert!(paths.iter().any(|p| p.ends_with("lib")));

    // Should include test resources directory
    assert!(paths.iter().any(|p| p.ends_with("test-libraries")));
}

#[test]
fn test_find_library_in_lib_directory() {
    let eval = test_evaluator();

    // The test libraries are in resources/test-libraries/test/
    // Verify they can be found
    let simple_path = eval.find_library_file(&["test".to_string(), "simple".to_string()]);
    assert!(simple_path.is_some());

    let path = simple_path.unwrap();
    assert!(path.exists());
    assert!(path.to_string_lossy().contains("test"));
    assert!(path.to_string_lossy().contains("simple.sld"));
}

#[test]
fn test_library_with_imports() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create a base library
    fs::write(
        lib_dir.join("base.sld"),
        r#"
        (define-library (test base)
          (import (scheme base))
          (export base-value add-ten)
          (begin
            (define base-value 100)
            (define (add-ten x) (+ x 10))))
    "#,
    )
    .unwrap();

    // Create a library that imports the base library
    fs::write(
        lib_dir.join("user.sld"),
        r#"
        (define-library (test user)
          (import (scheme base) (test base))
          (export computed-value)
          (begin
            (define computed-value (add-ten base-value))))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "user".to_string()])
        .expect("Failed to load library with imports");

    assert!(
        lib.exports_identifier("computed-value"),
        "Should export computed-value"
    );

    // Verify the computed value is correct (100 + 10 = 110)
    let tv = lib
        .env
        .get("computed-value")
        .expect("computed-value should be defined");
    assert_eq!(
        tv.as_fixnum(),
        Some(110),
        "computed-value should be 110 (base-value + 10)"
    );
}

#[test]
fn test_circular_dependency_detection() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create two libraries that import each other (circular dependency)
    fs::write(
        lib_dir.join("lib-a.sld"),
        r#"
        (define-library (test lib-a)
          (import (test lib-b))
          (export a-value)
          (begin (define a-value 1)))
    "#,
    )
    .unwrap();

    fs::write(
        lib_dir.join("lib-b.sld"),
        r#"
        (define-library (test lib-b)
          (import (test lib-a))
          (export b-value)
          (begin (define b-value 2)))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    // Attempting to load either library should fail with circular dependency error
    let result = eval.load_library(&["test".to_string(), "lib-a".to_string()]);
    assert!(result.is_err(), "Loading circular dependency should fail");

    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("ircular") || err.contains("already being loaded"),
        "Error should mention circular dependency: {}",
        err
    );
}

#[test]
fn test_import_modifiers() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create a source library with multiple exports
    fs::write(
        lib_dir.join("source.sld"),
        r#"
        (define-library (test source)
          (export foo bar baz qux)
          (begin
            (define foo 1)
            (define bar 2)
            (define baz 3)
            (define qux 4)))
    "#,
    )
    .unwrap();

    // Test 'only' modifier
    fs::write(
        lib_dir.join("only-test.sld"),
        r#"
        (define-library (test only-test)
          (import (scheme base) (only (test source) foo bar))
          (export result)
          (begin (define result (+ foo bar))))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "only-test".to_string()])
        .expect("Failed to load library with 'only' modifier");

    let tv = lib.env.get("result").expect("result should be defined");
    assert_eq!(
        tv.as_fixnum(),
        Some(3),
        "result should be 3 (foo + bar = 1 + 2)"
    );

    // Test 'except' modifier
    fs::write(
        lib_dir.join("except-test.sld"),
        r#"
        (define-library (test except-test)
          (import (scheme base) (except (test source) baz qux))
          (export result)
          (begin (define result (+ foo bar))))
    "#,
    )
    .unwrap();

    let lib = eval
        .load_library(&["test".to_string(), "except-test".to_string()])
        .expect("Failed to load library with 'except' modifier");

    let tv = lib.env.get("result").expect("result should be defined");
    assert_eq!(
        tv.as_fixnum(),
        Some(3),
        "result should be 3 (foo + bar = 1 + 2)"
    );

    // Test 'prefix' modifier
    fs::write(
        lib_dir.join("prefix-test.sld"),
        r#"
        (define-library (test prefix-test)
          (import (scheme base) (prefix (test source) src:))
          (export result)
          (begin (define result (+ src:foo src:bar))))
    "#,
    )
    .unwrap();

    let lib = eval
        .load_library(&["test".to_string(), "prefix-test".to_string()])
        .expect("Failed to load library with 'prefix' modifier");

    let tv = lib.env.get("result").expect("result should be defined");
    assert_eq!(
        tv.as_fixnum(),
        Some(3),
        "result should be 3 (src:foo + src:bar = 1 + 2)"
    );

    // Test 'rename' modifier
    fs::write(
        lib_dir.join("rename-test.sld"),
        r#"
        (define-library (test rename-test)
          (import (scheme base) (rename (test source) (foo first) (bar second)))
          (export result)
          (begin (define result (+ first second))))
    "#,
    )
    .unwrap();

    let lib = eval
        .load_library(&["test".to_string(), "rename-test".to_string()])
        .expect("Failed to load library with 'rename' modifier");

    let tv = lib.env.get("result").expect("result should be defined");
    assert_eq!(
        tv.as_fixnum(),
        Some(3),
        "result should be 3 (first + second = 1 + 2)"
    );
}

// ============================================================================
// L0: inline define-library (both backends)
// ============================================================================

#[test]
fn test_inline_define_library() {
    // The vendor-specific (declare ...) clause also proves lenient unknown
    // declarations compose with the inline path.
    common::assert_program_eval_to(
        r#"
        (define-library (inline demo)
          (import (scheme base))
          (declare (pure))
          (export twice)
          (begin (define (twice x) (* 2 x))))
        (import (inline demo))
        (twice 21)
        "#,
        "42",
    );
}

#[test]
fn test_inline_define_library_redefinition_wins() {
    common::assert_program_eval_to(
        r#"
        (define-library (inline redef)
          (import (scheme base))
          (export v)
          (begin (define v 1)))
        (define-library (inline redef)
          (import (scheme base))
          (export v)
          (begin (define v 2)))
        (import (inline redef))
        v
        "#,
        "2",
    );
}

// ============================================================================
// L0: unknown declarations in .sld files load leniently
// ============================================================================

#[test]
fn test_sld_with_unknown_clause_loads() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    fs::write(
        lib_dir.join("vendor-clause.sld"),
        r#"
        (define-library (test vendor-clause)
          (import (scheme base))
          (declare (pure) (no-side-effects))
          (export stable-export)
          (begin (define stable-export 42)))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "vendor-clause".to_string()])
        .expect("a library with an unknown declaration must still load");

    assert!(lib.exports_identifier("stable-export"));
    let tv = lib.env.get("stable-export").unwrap();
    assert_eq!(tv.as_fixnum(), Some(42));
}

// ============================================================================
// Include Declaration Tests (R7RS §5.6.1)
// ============================================================================

#[test]
fn test_library_with_include() {
    let eval = test_evaluator();

    // Load library that uses include
    let lib = eval
        .load_library(&["test".to_string(), "with-include".to_string()])
        .expect("Failed to load (test with-include) library");

    // Verify library was loaded
    assert_eq!(lib.name, vec!["test", "with-include"]);

    // Verify exports from included file
    assert!(lib.exports_identifier("double-it"));
    assert!(lib.exports_identifier("triple-it"));
    assert_eq!(lib.export_names().len(), 2);
}

#[test]
fn test_library_with_multiple_includes() {
    let eval = test_evaluator();

    // Load library with multiple includes (SRFI-1 pattern)
    let lib = eval
        .load_library(&["test".to_string(), "multi-include".to_string()])
        .expect("Failed to load (test multi-include) library");

    // Verify exports from both included files
    assert!(lib.exports_identifier("add"));
    assert!(lib.exports_identifier("sub"));
    assert!(lib.exports_identifier("mul"));
    assert!(lib.exports_identifier("proper-list?"));
    assert_eq!(lib.export_names().len(), 4);
}

#[test]
fn test_include_order_matters() {
    let eval = test_evaluator();

    // Load library that tests declaration order
    // begin defines x=1, include sets x=42, begin captures result
    let lib = eval
        .load_library(&["test".to_string(), "include-order".to_string()])
        .expect("Failed to load (test include-order) library");

    // Verify the export exists
    assert!(lib.exports_identifier("result"));

    // result should be 42 (set by included file)
    let tv = lib
        .env
        .get("result")
        .expect("result should be defined in library");
    assert_eq!(
        tv.as_fixnum(),
        Some(42),
        "result should be 42, set by included file"
    );
}

#[test]
fn test_include_file_not_found() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create library that includes non-existent file
    fs::write(
        lib_dir.join("bad-include.sld"),
        r#"
        (define-library (test bad-include)
          (export foo)
          (include "nonexistent.scm"))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let result = eval.load_library(&["test".to_string(), "bad-include".to_string()]);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found") || err.contains("No such file"),
        "Error should mention file not found: {}",
        err
    );
}

// ============================================================================
// Include-CI Tests (R7RS §5.6.1 - Case-insensitive include)
// ============================================================================

#[test]
fn test_include_ci_folds_identifiers() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create file with uppercase identifiers (legacy style)
    fs::write(
        lib_dir.join("legacy.scm"),
        r#"
        (define MY-CONST 42)
        (define (COMPUTE-IT X) (* X MY-CONST))
    "#,
    )
    .unwrap();

    // Create library that uses include-ci to read the file
    fs::write(
        lib_dir.join("with-include-ci.sld"),
        r#"
        (define-library (test with-include-ci)
          (import (scheme base))
          (export my-const compute-it)
          (include-ci "legacy.scm"))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "with-include-ci".to_string()])
        .expect("Failed to load library with include-ci");

    // Exports should be lowercase (case-folded)
    assert!(
        lib.exports_identifier("my-const"),
        "Should export my-const (lowercase)"
    );
    assert!(
        lib.exports_identifier("compute-it"),
        "Should export compute-it (lowercase)"
    );
    assert_eq!(lib.export_names().len(), 2);

    // Verify my-const has correct value
    let tv = lib.env.get("my-const").expect("my-const should be defined");
    assert_eq!(tv.as_fixnum(), Some(42), "my-const should be 42");
}

#[test]
fn test_include_ci_vs_include() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create file with mixed case
    fs::write(
        lib_dir.join("mixed.scm"),
        r#"
        (define MY-VALUE 123)
    "#,
    )
    .unwrap();

    // Library using regular include should preserve case
    fs::write(
        lib_dir.join("case-sensitive.sld"),
        r#"
        (define-library (test case-sensitive)
          (export MY-VALUE)
          (include "mixed.scm"))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "case-sensitive".to_string()])
        .expect("Failed to load case-sensitive library");

    // Regular include preserves case
    assert!(
        lib.exports_identifier("MY-VALUE"),
        "Regular include should preserve MY-VALUE"
    );
}

// ============================================================================
// Include-Library-Declarations Tests (R7RS §5.6.1)
// ============================================================================

#[test]
fn test_include_library_declarations_export() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create file with library declarations (exports)
    fs::write(
        lib_dir.join("exports.scm"),
        r#"
        (export foo bar)
    "#,
    )
    .unwrap();

    // Create library that includes declarations
    fs::write(
        lib_dir.join("decl-lib.sld"),
        r#"
        (define-library (test decl-lib)
          (include-library-declarations "exports.scm")
          (begin
            (define foo 1)
            (define bar 2)
            (define internal 3)))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "decl-lib".to_string()])
        .expect("Failed to load library with include-library-declarations");

    // Should have exports from included file
    assert!(lib.exports_identifier("foo"), "Should export foo");
    assert!(lib.exports_identifier("bar"), "Should export bar");
    assert!(
        !lib.exports_identifier("internal"),
        "Should not export internal"
    );
    assert_eq!(lib.export_names().len(), 2);
}

#[test]
fn test_include_library_declarations_import() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create file with import declaration
    fs::write(
        lib_dir.join("imports.scm"),
        r#"
        (import (scheme base))
    "#,
    )
    .unwrap();

    // Create library that includes import declaration
    fs::write(
        lib_dir.join("import-decl.sld"),
        r#"
        (define-library (test import-decl)
          (include-library-declarations "imports.scm")
          (export double-value)
          (begin
            (define (double-value x) (* x 2))))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "import-decl".to_string()])
        .expect("Failed to load library with include-library-declarations for imports");

    // Library should work (has access to * from scheme base)
    assert!(
        lib.exports_identifier("double-value"),
        "Should export double-value"
    );
}

#[test]
fn test_include_library_declarations_begin() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create file with begin declaration
    fs::write(
        lib_dir.join("body.scm"),
        r#"
        (begin
          (define x 100)
          (define (get-x) x))
    "#,
    )
    .unwrap();

    // Create library that includes begin declaration
    fs::write(
        lib_dir.join("begin-decl.sld"),
        r#"
        (define-library (test begin-decl)
          (export x get-x)
          (include-library-declarations "body.scm"))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "begin-decl".to_string()])
        .expect("Failed to load library with include-library-declarations for begin");

    assert!(lib.exports_identifier("x"), "Should export x");
    assert!(lib.exports_identifier("get-x"), "Should export get-x");

    // Verify x has correct value
    let tv = lib.env.get("x").expect("x should be defined");
    assert_eq!(tv.as_fixnum(), Some(100), "x should be 100");
}

#[test]
fn test_include_library_declarations_multiple_files() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // First declarations file
    fs::write(
        lib_dir.join("decl1.scm"),
        r#"
        (export foo)
    "#,
    )
    .unwrap();

    // Second declarations file
    fs::write(
        lib_dir.join("decl2.scm"),
        r#"
        (export bar)
        (begin
          (define bar 99))
    "#,
    )
    .unwrap();

    // Library including both
    fs::write(
        lib_dir.join("multi-decl.sld"),
        r#"
        (define-library (test multi-decl)
          (include-library-declarations "decl1.scm" "decl2.scm")
          (begin
            (define foo 42)))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "multi-decl".to_string()])
        .expect("Failed to load library with multiple include-library-declarations");

    assert!(lib.exports_identifier("foo"), "Should export foo");
    assert!(lib.exports_identifier("bar"), "Should export bar");
    assert_eq!(lib.export_names().len(), 2);

    // Verify values
    let tv = lib.env.get("foo").expect("foo should be defined");
    assert_eq!(tv.as_fixnum(), Some(42));
    let tv = lib.env.get("bar").expect("bar should be defined");
    assert_eq!(tv.as_fixnum(), Some(99));
}

#[test]
fn test_include_library_declarations_nested() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Inner declarations file with include
    fs::write(
        lib_dir.join("inner.scm"),
        r#"
        (include "impl.scm")
    "#,
    )
    .unwrap();

    // Implementation file
    fs::write(
        lib_dir.join("impl.scm"),
        r#"
        (define deep-value 999)
    "#,
    )
    .unwrap();

    // Library with nested include
    fs::write(
        lib_dir.join("nested-decl.sld"),
        r#"
        (define-library (test nested-decl)
          (export deep-value)
          (include-library-declarations "inner.scm"))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "nested-decl".to_string()])
        .expect("Failed to load library with nested include-library-declarations");

    assert!(
        lib.exports_identifier("deep-value"),
        "Should export deep-value"
    );

    let tv = lib
        .env
        .get("deep-value")
        .expect("deep-value should be defined");
    assert_eq!(tv.as_fixnum(), Some(999));
}

// ============================================================================
// cond-expand (library ...) Tests (R7RS §4.2.1)
// ============================================================================

#[test]
fn test_cond_expand_library_check_available() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create a library that uses cond-expand to check if (scheme base) is available
    // Since (scheme base) is always available, the first branch should be taken
    fs::write(
        lib_dir.join("lib-check.sld"),
        r#"
        (define-library (test lib-check)
          (export result)
          (cond-expand
            ((library (scheme base))
             (begin (define result 'base-available)))
            (else
             (begin (define result 'base-not-available)))))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "lib-check".to_string()])
        .expect("Failed to load library with cond-expand (library ...) check");

    assert!(lib.exports_identifier("result"), "Should export result");

    // Verify the correct branch was taken
    let tv = lib.env.get("result").expect("result should be defined");
    let heap = lib.env.heap();
    let name = heap
        .borrow()
        .get_symbol_name(tv)
        .map(|s| s.to_string())
        .expect("result should be a symbol");
    assert_eq!(
        name, "base-available",
        "(library (scheme base)) should be true"
    );
}

#[test]
fn test_cond_expand_library_check_unavailable() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // Create a library that checks for a non-existent library
    fs::write(
        lib_dir.join("lib-check-missing.sld"),
        r#"
        (define-library (test lib-check-missing)
          (export result)
          (cond-expand
            ((library (nonexistent library that does not exist))
             (begin (define result 'found-nonexistent)))
            (else
             (begin (define result 'correctly-not-found)))))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "lib-check-missing".to_string()])
        .expect("Failed to load library with cond-expand for missing library");

    assert!(lib.exports_identifier("result"), "Should export result");

    // Verify the else branch was taken
    let tv = lib.env.get("result").expect("result should be defined");
    let heap = lib.env.heap();
    let name = heap
        .borrow()
        .get_symbol_name(tv)
        .map(|s| s.to_string())
        .expect("result should be a symbol");
    assert_eq!(
        name, "correctly-not-found",
        "Non-existent library should not be found"
    );
}

#[test]
fn test_cond_expand_library_check_sld_library() {
    use std::fs;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    // First create a simple .sld library
    fs::write(
        lib_dir.join("helper.sld"),
        r#"
        (define-library (test helper)
          (export helper-value)
          (begin (define helper-value 42)))
    "#,
    )
    .unwrap();

    // Create a library that checks for the .sld library
    fs::write(
        lib_dir.join("check-helper.sld"),
        r#"
        (define-library (test check-helper)
          (export result)
          (cond-expand
            ((library (test helper))
             (begin (define result 'helper-available)))
            (else
             (begin (define result 'helper-not-available)))))
    "#,
    )
    .unwrap();

    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());

    let lib = eval
        .load_library(&["test".to_string(), "check-helper".to_string()])
        .expect("Failed to load library that checks for .sld library");

    assert!(lib.exports_identifier("result"), "Should export result");

    // Verify (test helper) was found
    let tv = lib.env.get("result").expect("result should be defined");
    let heap = lib.env.heap();
    let name = heap
        .borrow()
        .get_symbol_name(tv)
        .map(|s| s.to_string())
        .expect("result should be a symbol");
    assert_eq!(
        name, "helper-available",
        "(library (test helper)) should be found"
    );
}
