//! Tests for library loading system
//!
//! These tests verify that the library loading infrastructure works correctly
//! for Rust libraries, Scheme libraries (.sld files), and mixed libraries.

mod common;

use patina_interpreter::TreeWalkInterpreter;
use patina_runtime::{Environment, Library, LibraryError};
use std::rc::Rc;

/// Test builder for a simple math library
fn build_simple_math(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let mut heap = env.heap().borrow_mut();

    let pi_tv = heap.alloc_real(std::f64::consts::PI);
    let e_tv = heap.alloc_real(std::f64::consts::E);
    let desc_tv = heap.alloc_string("Simple math constants".to_string());
    drop(heap);

    env.define("pi".to_string(), pi_tv);
    env.define("e".to_string(), e_tv);
    env.define("description".to_string(), desc_tv);

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
    let tv = env.get("pi").expect("pi should be defined");
    let pi = env
        .heap()
        .borrow()
        .get_real(tv)
        .expect("pi should be a Real value");
    assert!((pi - std::f64::consts::PI).abs() < 1e-10);
}

#[test]
fn test_library_with_exports() {
    let mut lib = Library::new(vec!["test".to_string()]);

    // Add exports
    lib.export_tagged("foo".to_string(), patina_core::TaggedValue::fixnum(42));
    lib.export_tagged("bar".to_string(), patina_core::TaggedValue::TRUE);

    // Verify
    assert!(lib.exports_identifier("foo"));
    assert!(lib.exports_identifier("bar"));
    assert_eq!(
        lib.get_export_tagged("foo"),
        Some(patina_core::TaggedValue::fixnum(42))
    );
    assert_eq!(
        lib.get_export_tagged("bar"),
        Some(patina_core::TaggedValue::TRUE)
    );
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
    let interp = TreeWalkInterpreter::new_tree_walker();

    // library? is a Patina extension in (patina debug), not R7RS
    interp.eval_str("(import (patina debug))").unwrap();

    // library? predicate should work
    let result = interp.eval_str("(library? 42)").unwrap();
    assert_eq!(result, patina_core::TaggedValue::FALSE);

    let result = interp.eval_str("(library? \"hello\")").unwrap();
    assert_eq!(result, patina_core::TaggedValue::FALSE);

    // Note: We can't easily create Library values from Scheme yet,
    // but the predicate is registered and works
}

/// **Open defect, pinned.** An imported variable is a copy of its value at
/// import time, not the exporting library's binding, so a mutation the library
/// makes afterwards is invisible to the importer.
///
/// The library's own view is live — `peek` reads 2 — which locates the fault
/// in what `import` installs rather than in how `set!` runs. Gauche and chibi
/// both report 2 for `count` on the same program.
///
/// Both backends do this, and the chibi R7RS suite is 1226/1226 across it,
/// because nothing there mutates a variable another library imported. Found
/// 2026-08-18 while writing an R6RS library test, which is the kind of thing
/// a second corpus is for.
///
/// Asserted as-is rather than as a divergence, because both backends agree
/// and neither fails: they return a plausible wrong answer. **When this
/// converges on 2, delete the `count` assertion and keep the `peek` one.**
#[test]
fn an_imported_variable_is_a_stale_copy_of_its_binding() {
    let program = "(define-library (pinned counter)
                     (export bump count peek)
                     (import (scheme base))
                     (begin
                       (define count 0)
                       (define (bump) (set! count (+ count 1)))
                       (define (peek) count)))
                   (import (pinned counter))
                   (bump)
                   (bump)
                   (list count (peek))";

    assert_eq!(
        common::eval_program_tree_walker(program),
        "(0 2)",
        "tree-walker: expected the pinned wrong answer; if this is now (2 2) the defect is fixed"
    );
    assert_eq!(
        common::eval_program_vm(program),
        "(0 2)",
        "vm: expected the pinned wrong answer; if this is now (2 2) the defect is fixed"
    );
}
