//! Tests for R7RS record types (Section 5.5)

use patina_interpreter::TreeWalkInterpreter;

fn eval(code: &str) -> String {
    let interp = TreeWalkInterpreter::new_tree_walker();
    match interp.eval_str(code) {
        Ok(v) => format!("{}", v),
        Err(e) => format!("ERROR: {}", e),
    }
}

fn eval_program(code: &str) -> String {
    let interp = TreeWalkInterpreter::new_tree_walker();
    match interp.eval_program(code) {
        Ok(v) => format!("{}", v),
        Err(e) => format!("ERROR: {}", e),
    }
}

// =============================================================================
// Low-level primitive tests
// =============================================================================

#[test]
fn test_make_record_type() {
    // %make-record-type creates a record type descriptor
    let result = eval("(%make-record-type 'point '(x y))");
    assert!(
        result.starts_with("#<record-type"),
        "Expected record-type, got: {}",
        result
    );
    assert!(
        result.contains("point"),
        "Expected 'point' in name, got: {}",
        result
    );
}

#[test]
fn test_record_type_predicate() {
    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y)))
        (%record-type? rtd)
    "#,
    );
    assert_eq!(result, "#t");

    let result = eval("(%record-type? 42)");
    assert_eq!(result, "#f");

    let result = eval("(%record-type? '())");
    assert_eq!(result, "#f");
}

#[test]
fn test_make_record() {
    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y)))
        (define r (%make-record rtd (vector 10 20)))
        (%record? r)
    "#,
    );
    assert_eq!(result, "#t");
}

#[test]
fn test_record_ref() {
    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y)))
        (define r (%make-record rtd (vector 10 20)))
        (%record-ref r 0)
    "#,
    );
    assert_eq!(result, "10");

    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y)))
        (define r (%make-record rtd (vector 10 20)))
        (%record-ref r 1)
    "#,
    );
    assert_eq!(result, "20");
}

#[test]
fn test_record_set() {
    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y)))
        (define r (%make-record rtd (vector 10 20)))
        (%record-set! r 0 100)
        (%record-ref r 0)
    "#,
    );
    assert_eq!(result, "100");
}

#[test]
fn test_record_type_of() {
    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y)))
        (define r (%make-record rtd (vector 10 20)))
        (eq? (%record-type-of r) rtd)
    "#,
    );
    assert_eq!(result, "#t");
}

#[test]
fn test_record_type_name() {
    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y)))
        (%record-type-name rtd)
    "#,
    );
    assert_eq!(result, "point");
}

#[test]
fn test_record_type_fields() {
    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y z)))
        (%record-type-fields rtd)
    "#,
    );
    assert_eq!(result, "(x y z)");
}

#[test]
fn test_record_type_field_index() {
    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y z)))
        (%record-type-field-index rtd 'y)
    "#,
    );
    assert_eq!(result, "1");

    let result = eval_program(
        r#"
        (define rtd (%make-record-type 'point '(x y z)))
        (%record-type-field-index rtd 'w)
    "#,
    );
    assert_eq!(result, "#f");
}

// =============================================================================
// define-record-type macro tests
// =============================================================================

#[test]
fn test_define_record_type_basic() {
    // The example from R7RS spec
    let result = eval_program(
        r#"
        (define-record-type <pare>
          (kons x y)
          pare?
          (x kar set-kar!)
          (y kdr))
        (pare? (kons 1 2))
    "#,
    );
    assert_eq!(result, "#t");
}

#[test]
fn test_record_type_disjoint_from_pair() {
    let result = eval_program(
        r#"
        (define-record-type <pare>
          (kons x y)
          pare?
          (x kar set-kar!)
          (y kdr))
        (pare? (cons 1 2))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_accessors() {
    let result = eval_program(
        r#"
        (define-record-type <pare>
          (kons x y)
          pare?
          (x kar set-kar!)
          (y kdr))
        (kar (kons 1 2))
    "#,
    );
    assert_eq!(result, "1");

    let result = eval_program(
        r#"
        (define-record-type <pare>
          (kons x y)
          pare?
          (x kar set-kar!)
          (y kdr))
        (kdr (kons 1 2))
    "#,
    );
    assert_eq!(result, "2");
}

#[test]
fn test_record_mutator() {
    let result = eval_program(
        r#"
        (define-record-type <pare>
          (kons x y)
          pare?
          (x kar set-kar!)
          (y kdr))
        (let ((k (kons 1 2)))
          (set-kar! k 3)
          (kar k))
    "#,
    );
    assert_eq!(result, "3");
}

#[test]
fn test_record_type_disjoint_from_vector() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (vector? (make-point 1 2))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_is_not_pair() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (pair? (make-point 1 2))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_is_not_null() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (null? (make-point 1 2))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_generative_semantics() {
    // Two record types with same structure should be different
    let result = eval_program(
        r#"
        (define-record-type <point1>
          (make-point1 x y)
          point1?
          (x point1-x)
          (y point1-y))
        (define-record-type <point2>
          (make-point2 x y)
          point2?
          (x point2-x)
          (y point2-y))
        (point1? (make-point2 1 2))
    "#,
    );
    assert_eq!(result, "#f");

    let result = eval_program(
        r#"
        (define-record-type <point1>
          (make-point1 x y)
          point1?
          (x point1-x)
          (y point1-y))
        (define-record-type <point2>
          (make-point2 x y)
          point2?
          (x point2-x)
          (y point2-y))
        (point2? (make-point1 1 2))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_with_three_fields() {
    let result = eval_program(
        r#"
        (define-record-type <point3d>
          (make-point3d x y z)
          point3d?
          (x point3d-x)
          (y point3d-y)
          (z point3d-z set-point3d-z!))
        (let ((p (make-point3d 1 2 3)))
          (list (point3d-x p) (point3d-y p) (point3d-z p)))
    "#,
    );
    assert_eq!(result, "(1 2 3)");
}

#[test]
fn test_record_display() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (make-point 1 2)
    "#,
    );
    assert!(
        result.contains("record"),
        "Expected record display, got: {}",
        result
    );
    assert!(
        result.contains("point"),
        "Expected 'point' in display, got: {}",
        result
    );
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn test_record_with_single_field() {
    let result = eval_program(
        r#"
        (define-record-type <wrapper>
          (wrap value)
          wrapper?
          (value unwrap set-unwrap!))
        (unwrap (wrap 42))
    "#,
    );
    assert_eq!(result, "42");
}

#[test]
fn test_record_field_holds_any_value() {
    // Records can hold any Scheme value
    let result = eval_program(
        r#"
        (define-record-type <box>
          (make-box contents)
          box?
          (contents unbox set-box!))
        (unbox (make-box '(a b c)))
    "#,
    );
    assert_eq!(result, "(a b c)");

    let result = eval_program(
        r#"
        (define-record-type <box>
          (make-box contents)
          box?
          (contents unbox set-box!))
        (unbox (make-box (lambda (x) x)))
    "#,
    );
    assert!(
        result.contains("procedure"),
        "Expected procedure, got: {}",
        result
    );
}

#[test]
fn test_nested_records() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (define-record-type <rect>
          (make-rect top-left bottom-right)
          rect?
          (top-left rect-top-left)
          (bottom-right rect-bottom-right))
        (let ((r (make-rect (make-point 0 0) (make-point 10 10))))
          (point-x (rect-bottom-right r)))
    "#,
    );
    assert_eq!(result, "10");
}

// =============================================================================
// Constructor field ordering tests
// =============================================================================

#[test]
fn test_constructor_field_order_differs_from_declaration() {
    // Constructor can specify fields in different order than declaration
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point y x)  ; reversed order in constructor
          point?
          (x point-x)
          (y point-y))
        (let ((p (make-point 20 10)))  ; y=20, x=10
          (list (point-x p) (point-y p)))
    "#,
    );
    assert_eq!(result, "(10 20)");
}

#[test]
fn test_constructor_with_subset_of_fields() {
    // Constructor only initializes some fields
    let result = eval_program(
        r#"
        (define-record-type <point3d>
          (make-point x y)  ; only x and y in constructor
          point3d?
          (x point-x)
          (y point-y)
          (z point-z set-point-z!))
        (let ((p (make-point 1 2)))
          (set-point-z! p 3)
          (list (point-x p) (point-y p) (point-z p)))
    "#,
    );
    assert_eq!(result, "(1 2 3)");
}

// =============================================================================
// Equality tests for records
// =============================================================================

#[test]
fn test_record_eq_same_instance() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (let ((p (make-point 1 2)))
          (eq? p p))
    "#,
    );
    assert_eq!(result, "#t");
}

#[test]
fn test_record_eq_different_instances() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (eq? (make-point 1 2) (make-point 1 2))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_eqv_same_instance() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (let ((p (make-point 1 2)))
          (eqv? p p))
    "#,
    );
    assert_eq!(result, "#t");
}

#[test]
fn test_record_eqv_different_instances() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (eqv? (make-point 1 2) (make-point 1 2))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_equal_same_contents() {
    // equal? should compare structurally
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (equal? (make-point 1 2) (make-point 1 2))
    "#,
    );
    assert_eq!(result, "#t");
}

#[test]
fn test_record_equal_different_contents() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (equal? (make-point 1 2) (make-point 1 3))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_equal_different_types() {
    // equal? should return #f for different record types even with same values
    let result = eval_program(
        r#"
        (define-record-type <point1>
          (make-point1 x y)
          point1?
          (x point1-x)
          (y point1-y))
        (define-record-type <point2>
          (make-point2 x y)
          point2?
          (x point2-x)
          (y point2-y))
        (equal? (make-point1 1 2) (make-point2 1 2))
    "#,
    );
    assert_eq!(result, "#f");
}

// =============================================================================
// Record type identity tests
// =============================================================================

#[test]
fn test_record_type_is_first_class() {
    // The record type itself is a first-class value
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (%record-type? <point>)
    "#,
    );
    assert_eq!(result, "#t");
}

#[test]
fn test_record_type_name_introspection() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (%record-type-name <point>)
    "#,
    );
    assert_eq!(result, "<point>");
}

#[test]
fn test_record_type_fields_introspection() {
    let result = eval_program(
        r#"
        (define-record-type <point>
          (make-point x y)
          point?
          (x point-x)
          (y point-y))
        (%record-type-fields <point>)
    "#,
    );
    assert_eq!(result, "(x y)");
}

// =============================================================================
// Mutation tests
// =============================================================================

#[test]
fn test_record_mutation_doesnt_affect_other_instances() {
    let result = eval_program(
        r#"
        (define-record-type <box>
          (make-box value)
          box?
          (value unbox set-box!))
        (let ((b1 (make-box 1))
              (b2 (make-box 1)))
          (set-box! b1 99)
          (list (unbox b1) (unbox b2)))
    "#,
    );
    assert_eq!(result, "(99 1)");
}

#[test]
fn test_record_shared_reference_mutation() {
    let result = eval_program(
        r#"
        (define-record-type <box>
          (make-box value)
          box?
          (value unbox set-box!))
        (let* ((b (make-box 1))
               (alias b))
          (set-box! alias 99)
          (unbox b))
    "#,
    );
    assert_eq!(result, "99");
}

// =============================================================================
// Type disjointness tests
// =============================================================================

#[test]
fn test_record_is_not_boolean() {
    let result = eval_program(
        r#"
        (define-record-type <box>
          (make-box value)
          box?
          (value unbox))
        (boolean? (make-box #t))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_is_not_number() {
    let result = eval_program(
        r#"
        (define-record-type <box>
          (make-box value)
          box?
          (value unbox))
        (number? (make-box 42))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_is_not_string() {
    let result = eval_program(
        r#"
        (define-record-type <box>
          (make-box value)
          box?
          (value unbox))
        (string? (make-box "hello"))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_is_not_symbol() {
    let result = eval_program(
        r#"
        (define-record-type <box>
          (make-box value)
          box?
          (value unbox))
        (symbol? (make-box 'hello))
    "#,
    );
    assert_eq!(result, "#f");
}

#[test]
fn test_record_is_not_procedure() {
    let result = eval_program(
        r#"
        (define-record-type <box>
          (make-box value)
          box?
          (value unbox))
        (procedure? (make-box (lambda () 1)))
    "#,
    );
    assert_eq!(result, "#f");
}
