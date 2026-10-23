//! Tests for the Library value type
//!
//! These tests verify that the Library value type is properly integrated
//! and that the library? predicate works correctly.

// The bare-value `eval_*` forms (#605), deprecated until stage 5e removes them.
#![allow(deprecated)]

mod common;

use patina_interpreter::TreeWalkInterpreter;
use patina_runtime::Library;

/// Every export of `(patina debug)`, called on both backends (#742). Of the
/// nine names it exported, the VM had no primitive for seven, and neither
/// backend for five of those.
#[test]
fn every_export_of_patina_debug_runs_on_both_backends() {
    common::assert_program_eval_to(
        "(import (scheme base) (patina debug))
         (list (macro-debug-mode 'status) (library? 42) (begin (gc) 'collected) (list? (gc-stats)))",
        "(disabled #f collected #t)",
    );
}

/// The five `debug-*` names left the library, since no backend implemented
/// them (owner decision on #742): an import of one fails at the import.
#[test]
fn the_debug_names_without_an_implementation_are_not_exported() {
    for name in [
        "debug-enable",
        "debug-disable",
        "debug-clear",
        "debug-status",
        "debug-mode",
    ] {
        common::assert_program_eval_error(&format!("(import (only (patina debug) {name}))"));
    }
}

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
