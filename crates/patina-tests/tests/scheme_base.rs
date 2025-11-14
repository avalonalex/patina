//! Tests for (scheme base) library

use patina_interpreter::TreeWalkInterpreter;
use patina_tree_walker::Evaluator;

#[test]
fn test_can_load_scheme_base() {
    let eval = Evaluator::new();

    // Load (scheme base)
    let lib = eval
        .load_library(&["scheme".to_string(), "base".to_string()])
        .expect("Failed to load (scheme base)");

    // Verify library name
    assert_eq!(lib.name, vec!["scheme", "base"]);

    // Verify some key exports
    assert!(lib.exports_identifier("+"));
    assert!(lib.exports_identifier("-"));
    assert!(lib.exports_identifier("cons"));
    assert!(lib.exports_identifier("car"));
    assert!(lib.exports_identifier("cdr"));
    assert!(lib.exports_identifier("list"));
    assert!(lib.exports_identifier("map"));
    assert!(lib.exports_identifier("display"));
    assert!(lib.exports_identifier("equal?"));

    // Should have many exports
    assert!(lib.export_names().len() > 100);
}

#[test]
fn test_scheme_base_is_cached() {
    let eval = Evaluator::new();

    // Load first time
    let lib1 = eval
        .load_library(&["scheme".to_string(), "base".to_string()])
        .unwrap();

    // Load second time - should be cached
    let lib2 = eval
        .load_library(&["scheme".to_string(), "base".to_string()])
        .unwrap();

    // Should be same library
    assert_eq!(lib1.name, lib2.name);
    assert_eq!(lib1.export_names().len(), lib2.export_names().len());
}

#[test]
fn test_scheme_base_primitives_work() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Load (scheme base) via evaluator
    let eval = interp.evaluator();
    let _lib = eval
        .load_library(&["scheme".to_string(), "base".to_string()])
        .expect("Failed to load (scheme base)");

    // The primitives should still work globally (backward compatibility)
    // Eventually we may want to test import mechanism
    let result = interp.eval_str("(+ 1 2 3)").unwrap();
    assert_eq!(result.to_string(), "6");

    let result = interp.eval_str("(cons 1 2)").unwrap();
    assert_eq!(result.to_string(), "(1 . 2)");
}
