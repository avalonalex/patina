//! End-to-end tests for loading .sld library files
//!
//! These tests verify that the complete library loading pipeline works:
//! - Finding .sld files in search paths
//! - Parsing define-library forms
//! - Resolving imports
//! - Evaluating library bodies
//! - Collecting exports

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
#[ignore = "Requires creating test files with dependencies"]
fn test_library_with_imports() {
    // TODO: Create a test library that imports another library
    // and verify that imports are resolved correctly
}

#[test]
#[ignore = "Requires circular dependency test files"]
fn test_circular_dependency_detection() {
    // TODO: Create two libraries that import each other
    // and verify that circular dependency is detected
}

#[test]
#[ignore = "Requires library with import modifiers"]
fn test_import_modifiers() {
    // TODO: Create a library that uses only, except, prefix, rename
    // and verify they work correctly
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

    // Get the value of result from the library environment
    let result_value = lib
        .env
        .get("result")
        .expect("result should be defined in library");

    // result should be 42 (set by included file)
    match result_value {
        patina_runtime::Value::Integer(n) => {
            assert_eq!(n, 42, "result should be 42, set by included file");
        }
        _ => panic!("result should be an integer"),
    }
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
