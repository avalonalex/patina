//! Tests for library loading system
//!
//! These tests verify that the library loading infrastructure works correctly
//! for Rust libraries, Scheme libraries (.sld files), and mixed libraries.

use patina_interpreter::Interpreter;
use patina_runtime::{Environment, Library, LibraryError, Value};
use std::rc::Rc;

/// Test builder for a simple math library
fn build_simple_math(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    use std::cell::RefCell;

    // Add some simple math functions
    env.define("pi".to_string(), Value::Real(std::f64::consts::PI));
    env.define("e".to_string(), Value::Real(std::f64::consts::E));
    env.define(
        "description".to_string(),
        Value::String(Rc::new(RefCell::new("Simple math constants".to_string()))),
    );

    // Export only the constants
    vec!["pi".to_string(), "e".to_string()]
}

#[test]
fn test_library_creation_basic() {
    // Create a simple library
    let lib = Library::new(vec!["test".to_string()]);
    assert_eq!(lib.name, vec!["test"]);
    assert_eq!(lib.name_string(), "(test)");
}

#[test]
fn test_rust_library_builder() {
    // Create environment and build library
    let env = Rc::new(Environment::new());
    let exports = build_simple_math(vec!["math".to_string()], env.clone());

    // Verify exports
    assert_eq!(exports.len(), 2);
    assert!(exports.contains(&"pi".to_string()));
    assert!(exports.contains(&"e".to_string()));

    // Verify values are in environment
    assert!(env.get("pi").is_some());
    assert!(env.get("e").is_some());
    assert!(env.get("description").is_some()); // Private, not exported

    // Verify pi value
    if let Some(Value::Real(pi)) = env.get("pi") {
        assert!((pi - std::f64::consts::PI).abs() < 1e-10);
    } else {
        panic!("pi should be a Real value");
    }
}

#[test]
fn test_library_with_exports() {
    let mut lib = Library::new(vec!["test".to_string()]);

    // Add exports
    lib.export("foo".to_string(), Value::Integer(42));
    lib.export("bar".to_string(), Value::Boolean(true));

    // Verify
    assert!(lib.exports_identifier("foo"));
    assert!(lib.exports_identifier("bar"));
    assert_eq!(lib.get_export("foo").unwrap().to_string(), "42");
    assert_eq!(lib.get_export("bar").unwrap().to_string(), "#t");
}

#[test]
fn test_evaluator_library_registry() {
    use patina_tree_walker::Evaluator;

    let eval = Evaluator::new();

    // Initially no libraries are loaded (except bootstrap)
    assert!(!eval.is_library_loaded(&["mylib".to_string()]));

    // TODO: Test loading when we have actual loadable libraries
}

#[test]
fn test_library_display_format() {
    let lib1 = Library::new(vec!["scheme".to_string(), "base".to_string()]);
    assert_eq!(lib1.to_string(), "#<library:(scheme base)>");

    let lib2 = Library::new(vec!["srfi".to_string(), "1".to_string()]);
    assert_eq!(lib2.to_string(), "#<library:(srfi 1)>");
}

#[test]
fn test_library_error_display() {
    let err = LibraryError::NotFound(vec!["nonexistent".to_string()]);
    assert_eq!(err.to_string(), "Library (nonexistent) not found");

    let err2 = LibraryError::AlreadyLoaded(vec!["scheme".to_string(), "base".to_string()]);
    assert_eq!(err2.to_string(), "Library (scheme base) is already loaded");
}

#[test]
fn test_circular_dependency_error() {
    let chain = vec![
        vec!["lib1".to_string()],
        vec!["lib2".to_string()],
        vec!["lib1".to_string()],
    ];

    let err = LibraryError::CircularDependency(chain);
    let msg = err.to_string();

    assert!(msg.contains("Circular library dependency detected"));
    assert!(msg.contains("(lib1)"));
    assert!(msg.contains("(lib2)"));
}

// Integration test with interpreter
#[test]
fn test_library_value_type() {
    let interp = Interpreter::new();

    // library? predicate should work
    let result = interp.eval_str("(library? 42)").unwrap();
    assert_eq!(result.to_string(), "#f");

    let result = interp.eval_str("(library? \"hello\")").unwrap();
    assert_eq!(result.to_string(), "#f");

    // Note: We can't easily create Library values from Scheme yet,
    // but the predicate is registered and works
}
