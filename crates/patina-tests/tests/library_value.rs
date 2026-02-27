//! Tests for the Library value type
//!
//! These tests verify that the Library value type is properly integrated
//! and that the library? predicate works correctly.

use patina_interpreter::TreeWalkInterpreter;
use patina_runtime::Library;

#[test]
fn test_library_value_creation() {
    // Create a library and verify its display format
    let lib = Library::new(vec!["test".to_string(), "lib".to_string()]);
    assert_eq!(lib.to_string(), "#<library:(test lib)>");
}

#[test]
fn test_library_predicate() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Test that library? is available
    // Note: We can't actually create libraries from Scheme yet,
    // but we can test that the predicate exists and works with non-libraries

    // Non-library values should return #f
    let result = interp.eval_str("(library? 42)").unwrap();
    assert_eq!(result, patina_core::TaggedValue::FALSE);

    let result = interp.eval_str("(library? \"hello\")").unwrap();
    assert_eq!(result, patina_core::TaggedValue::FALSE);

    let result = interp.eval_str("(library? '(1 2 3))").unwrap();
    assert_eq!(result, patina_core::TaggedValue::FALSE);

    let result = interp.eval_str("(library? +)").unwrap();
    assert_eq!(result, patina_core::TaggedValue::FALSE);
}

#[test]
fn test_library_with_exports() {
    let mut lib = Library::new(vec!["mylib".to_string()]);

    // Add some exports
    lib.export_tagged("foo".to_string(), patina_core::TaggedValue::fixnum(42));
    lib.export_tagged("bar".to_string(), patina_core::TaggedValue::TRUE);

    // Verify exports
    assert!(lib.exports_identifier("foo"));
    assert!(lib.exports_identifier("bar"));
    assert!(!lib.exports_identifier("baz"));

    // Get export values
    assert_eq!(
        lib.get_export_tagged("foo"),
        Some(patina_core::TaggedValue::fixnum(42))
    );
    assert_eq!(
        lib.get_export_tagged("bar"),
        Some(patina_core::TaggedValue::TRUE)
    );

    // Check export names
    let mut names = lib.export_names();
    names.sort();
    assert_eq!(names, vec!["bar", "foo"]);
}

#[test]
fn test_library_name_formatting() {
    // Single-part name
    let lib1 = Library::new(vec!["base".to_string()]);
    assert_eq!(lib1.name_string(), "(base)");

    // Multi-part name
    let lib2 = Library::new(vec!["scheme".to_string(), "base".to_string()]);
    assert_eq!(lib2.name_string(), "(scheme base)");

    // Long name
    let lib3 = Library::new(vec![
        "mycompany".to_string(),
        "myproject".to_string(),
        "utils".to_string(),
    ]);
    assert_eq!(lib3.name_string(), "(mycompany myproject utils)");
}
