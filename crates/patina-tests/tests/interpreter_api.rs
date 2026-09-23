//! Tests for the high-level Interpreter API
//!
//! These tests verify the public API provided by the main `patina` crate

use patina_interpreter::{Interpreter, TaggedValue, TreeWalkInterpreter};
use patina_runtime::Backend;
use patina_vm::VmBackend;

#[test]
fn test_interpreter_basic_arithmetic() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp.eval_str("(+ 1 2 3)").unwrap();
    assert_eq!(result.as_fixnum(), Some(6));
}

#[test]
fn test_interpreter_define_and_use() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(define x 42)").unwrap();
    let result = interp.eval_str("x").unwrap();
    assert_eq!(result.as_fixnum(), Some(42));
}

#[test]
fn test_eval_program() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp
        .eval_program(
            r#"
            (define x 10)
            (define y 20)
            (+ x y)
        "#,
        )
        .unwrap();
    assert_eq!(result.as_fixnum(), Some(30));
}

#[test]
fn test_macro_when() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Define the when macro
    let define_result = interp.eval_str(
        r#"
(define-syntax test-when
  (syntax-rules ()
    ((test-when test body ...)
     (if test (begin body ...)))))
"#,
    );

    if let Err(e) = define_result {
        panic!("Failed to define when macro: {}", e);
    }

    // Test single body
    let result = interp.eval_str("(test-when #t 42)");
    match &result {
        Ok(val) => println!("when macro result: {}", interp.display_tagged(*val)),
        Err(e) => panic!("when macro expansion error: {}", e),
    }
    let result = result.unwrap();
    assert_eq!(result.as_fixnum(), Some(42));

    // Test multiple body forms
    let result = interp.eval_str("(test-when #t 1 2 3)").unwrap();
    assert_eq!(result.as_fixnum(), Some(3));

    // Test false condition
    let result = interp.eval_str("(test-when #f 42)").unwrap();
    assert_eq!(result, TaggedValue::UNSPECIFIED);
}

#[test]
fn test_macro_unless() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Define the unless macro
    interp
        .eval_str(
            r#"
(define-syntax test-unless
  (syntax-rules ()
    ((test-unless test body ...)
     (if (not test) (begin body ...)))))
"#,
        )
        .unwrap();

    // Test with false condition (should execute)
    let result = interp.eval_str("(test-unless #f 42)").unwrap();
    assert_eq!(result.as_fixnum(), Some(42));

    // Test with true condition (should not execute)
    let result = interp.eval_str("(test-unless #t 42)").unwrap();
    assert_eq!(result, TaggedValue::UNSPECIFIED);
}

#[test]
fn test_gcd_with_let_values() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp
        .eval_program(
            r#"
            (define (quotient-and-remainder a b)
              (values (quotient a b) (remainder a b)))

            (define (gcd a b)
              (if (= b 0)
                  a
                  (let-values (((q r) (quotient-and-remainder a b)))
                    (gcd b r))))

            (gcd 48 18)
        "#,
        )
        .unwrap();

    assert_eq!(result.as_fixnum(), Some(6));
}

/// A form that fails must not leave the interpreter in the middle of it. The
/// REPL and a script run with `-k` both evaluate form by form on one
/// interpreter, and an error is where the VM used to be left holding the
/// frames, handlers and winds of the form it had abandoned: the next form's
/// own error was then delivered to the *previous* form's handler, or its
/// return landed in a frame that no longer had a program behind it (review of
/// triage families 22/28, 2026-09-01). Both failing shapes below stop the
/// program with something still installed — an unhandled exception from an
/// after-thunk mid-unwind, and a handler raising from inside a handler — and
/// the probe after each is that `car`'s own error is what comes back, then
/// that a plain form evaluates.
///
/// The after-thunk shape has no handler at all, on purpose: with a `guard`
/// around it the secondary is *caught* under the `finally` rule (R7RS 6.10,
/// `tests/scheme/control/wind-thunk-exceptions.scm`), so that form stops being
/// an error on a backend that meets the rule, and it did on the tree-walker.
#[test]
fn an_error_leaves_the_interpreter_ready_for_the_next_form() {
    fn check<B: Backend>(interp: Interpreter<B>, name: &str) {
        for failing in [
            "(call/cc (lambda (k) (dynamic-wind (lambda () #f) (lambda () (k 1)) (lambda () (raise 'sec)))))",
            "(with-exception-handler (lambda (e) (raise 'inner)) (lambda () (car 5)))",
        ] {
            assert!(
                interp.eval_str(failing).is_err(),
                "{name}: `{failing}` should fail"
            );
            let next = interp.eval_str("(car 5)");
            let message = next.as_ref().map(|_| ()).map_err(|e| e.to_string());
            assert!(
                message.as_ref().is_err_and(|m| m.contains("car")),
                "{name}: after `{failing}`, `(car 5)` should fail as car's own error, got {message:?}"
            );
            let result = interp
                .eval_str("(+ 1 2)")
                .unwrap_or_else(|e| panic!("{name}: after `{failing}`, `(+ 1 2)` failed: {e}"));
            assert_eq!(result.as_fixnum(), Some(3), "{name}: after `{failing}`");
        }
    }
    check(TreeWalkInterpreter::new_tree_walker(), "tree-walker");
    check(Interpreter::new(VmBackend::new()), "vm");
}

#[test]
fn source_map_entries_pruned_after_collection() {
    // GC_DESIGN.md §9.1: SourceMap is keyed by raw bits, so a slot the GC
    // reclaims must lose its entry before the slot can be reused — otherwise
    // a later value inherits the old datum's source location. The two
    // programs are identical except that one collects; the quoted list is
    // garbage by then, so its entries must be gone from the returned map.
    fn map_len(middle_form: &str) -> usize {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let program =
            format!("(import (scheme base) (patina debug))\n'(a b c d e f)\n{middle_form}\n42\n");
        let (result, source_map) = interp.eval_program_with_source_name(&program, "prune-test");
        assert_eq!(result.unwrap().as_fixnum(), Some(42));
        let len = source_map.borrow().len();
        assert!(len > 0, "parser recorded nothing");
        len
    }

    let collected = map_len("(gc)");
    let uncollected = map_len("(list)");
    assert!(
        collected < uncollected,
        "collection did not prune the source map: {collected} entries with (gc) \
         vs {uncollected} with (list)"
    );
}

// =============================================================================
// Input that ends inside a datum (#329)
// =============================================================================

use patina_interpreter::{InterpreterError, ParseError};

/// Programs cut short inside a datum, after zero or more complete forms.
const CUT_SHORT: &[&str] = &[
    "(+ 1",
    "42 (+ 1",
    "(define x 1)\n(define y (+ x",
    "#(1 2",
    "#u8(1",
    "'",
    "`(a ,",
    "(1 .",
    "1 #;",
    "#1=(a",
];

/// Programs whose text after the last form is whitespace and complete comments.
const CLEANLY_ENDED: &[&str] = &[
    "42",
    "42 \n",
    "42 ; a line comment",
    "42 #| a block comment |#",
    "42 #;(a datum comment) ",
    "42 #; #; 1 2",
];

fn is_cut_short<E: std::error::Error>(err: &InterpreterError<E>) -> bool {
    matches!(
        err,
        InterpreterError::Parse(ParseError::IncompleteDatum { .. })
    )
}

#[test]
fn eval_program_rejects_input_cut_short_inside_a_datum() {
    let tree_walker = TreeWalkInterpreter::new_tree_walker();
    let vm = Interpreter::new(VmBackend::new());
    for input in CUT_SHORT {
        let err = tree_walker.eval_program(input).expect_err(input);
        assert!(is_cut_short(&err), "tree-walker {input:?}: {err}");
        let err = vm.eval_program(input).expect_err(input);
        assert!(is_cut_short(&err), "vm {input:?}: {err}");
    }
}

#[test]
fn eval_program_ends_cleanly_after_trailing_whitespace_and_comments() {
    let tree_walker = TreeWalkInterpreter::new_tree_walker();
    let vm = Interpreter::new(VmBackend::new());
    for input in CLEANLY_ENDED {
        let results = [
            (
                "tree-walker",
                tree_walker.eval_program(input).map_err(|e| e.to_string()),
            ),
            ("vm", vm.eval_program(input).map_err(|e| e.to_string())),
        ];
        for (backend, result) in results {
            let value = result.unwrap_or_else(|e| panic!("{backend} {input:?}: {e}"));
            assert_eq!(value.as_fixnum(), Some(42), "{backend} {input:?}");
        }
    }
}

#[test]
fn eval_program_names_where_the_unfinished_datum_began() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let err = interp
        .eval_program("(define x 1)\n(define y\n  (+ x")
        .expect_err("cut short");
    assert!(
        matches!(
            err,
            InterpreterError::Parse(ParseError::IncompleteDatum { line: 2, column: 1 })
        ),
        "{err}"
    );
    assert_eq!(
        err.to_string(),
        "Parse error: Unexpected end of input inside the datum beginning at line 2, column 1"
    );
}

#[test]
fn eval_str_requires_a_datum() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    for input in ["", "  ", "; only a comment", "#;(only a datum comment)"] {
        let err = interp.eval_str(input).expect_err(input);
        assert!(
            matches!(err, InterpreterError::Parse(ParseError::UnexpectedEof)),
            "{input:?}: {err}"
        );
    }
    let err = interp.eval_str("(+ 1").expect_err("cut short");
    assert!(is_cut_short(&err), "{err}");
}

/// `eval_str` evaluates the first expression only, but it must still read
/// what follows: `"42 (+ 1"` reporting 42 is the same silent acceptance as a
/// truncated script exiting 0.
#[test]
fn eval_str_rejects_a_malformed_suffix_after_the_expression() {
    let tree_walker = TreeWalkInterpreter::new_tree_walker();
    let vm = Interpreter::new(VmBackend::new());
    for input in ["42 (+ 1", "42 '", "42 #;"] {
        let err = tree_walker.eval_str(input).expect_err(input);
        assert!(is_cut_short(&err), "tree-walker {input:?}: {err}");
        let err = vm.eval_str(input).expect_err(input);
        assert!(is_cut_short(&err), "vm {input:?}: {err}");
    }
    // A complete trailing form is still not evaluated, and still not an error.
    assert_eq!(
        tree_walker.eval_str("42 (+ 1 2)").unwrap().as_fixnum(),
        Some(42)
    );
    let err = tree_walker
        .eval_str_tracked("42 (+ 1")
        .expect_err("tracked");
    assert!(is_cut_short(&err), "{err}");
    let (result, _map) = tree_walker.eval_str_with_source_name("42 (+ 1", "<test>");
    assert!(is_cut_short(&result.expect_err("with source name")));
}

/// The diagnostic locates the unfinished form in the file, the way an
/// evaluation error does — a bare line number is little help in a long file.
#[test]
fn a_parse_error_is_rendered_with_its_source_line_and_caret() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let (result, source_map) =
        interp.eval_program_with_source_name("(define x 1)\n(define y\n  (+ 1", "cut.scm");
    let err = result.expect_err("cut short");
    let rendered = patina_interpreter::format_interpreter_error(&err, &source_map.borrow());
    assert!(rendered.contains("  at cut.scm:2:1"), "{rendered}");
    assert!(rendered.contains("(define y"), "{rendered}");
    assert!(rendered.contains('^'), "{rendered}");
}

/// Run `program` as the file `name` and render the error it stops at the way
/// the command line does.
fn rendered_error<B: Backend>(interp: &Interpreter<B>, program: &str, name: &str) -> String
where
    B::Error: patina_core::error::HasSourceLocation,
{
    let (result, source_map) = interp.eval_program_with_source_name(program, name);
    let err = result.expect_err("the program is malformed");
    patina_interpreter::format_backend_error_with_source(&err, &source_map.borrow())
}

/// Render on both backends, labelled, for assertions that hold on each.
fn rendered_on_both(program: &str, name: &str) -> [(&'static str, String); 2] {
    [
        (
            "tree-walker",
            rendered_error(&TreeWalkInterpreter::new_tree_walker(), program, name),
        ),
        (
            "vm",
            rendered_error(&Interpreter::new(VmBackend::new()), program, name),
        ),
    ]
}

/// A program rejected before it runs is placed at the form that was wrong,
/// not left without a position (#432). The `case` is on line 3, inside a
/// definition: the innermost form with a position is the one reported, and a
/// macro's use site stands for everything its expansion raised.
#[test]
fn a_desugar_error_is_rendered_at_the_form_that_raised_it() {
    let program = "(define before 1)\n\
                   (define (f n)\n  \
                   (case n\n    \
                   ((0) 'zero)\n    \
                   (else 'many)\n    \
                   ((1) 'one)))";
    for (backend, rendered) in rendered_on_both(program, "case.scm") {
        assert!(
            rendered.contains("  at case.scm:3:3"),
            "[{backend}] {rendered}"
        );
        assert!(
            rendered.contains("   3 |   (case n"),
            "[{backend}] {rendered}"
        );
        assert!(rendered.contains('^'), "[{backend}] {rendered}");
    }
}

/// `case` and `cond` are `syntax-rules` macros, so a malformed use could only
/// ever fail as "no pattern matches". Each common mistake is now named, with
/// the clause at fault (#432). Gauche rejects an `else` before `case`'s last
/// clause; chibi accepts it and never reaches the clauses after.
///
/// `cond` has no such rule — see the next test — so its mid-`else` is
/// rejected as syntax used as a value, which still has to be placed.
#[test]
fn a_malformed_case_or_cond_names_the_clause_at_fault() {
    let cases = [
        (
            "(case 1 ((0) 'zero) (else 'many) ((1) 'one))",
            "case: an else clause must be the last clause, and is followed by ((1) (quote one))",
        ),
        (
            "(case 1 (else => car) ((1) 'one))",
            "case: an else clause must be the last clause, and is followed by ((1) (quote one))",
        ),
        (
            "(case 1 (0 'zero) (else 'many))",
            "case: a clause must be ((datum ...) expression ...) or (else expression ...), \
             not (0 (quote zero))",
        ),
        (
            "(cond ((= 1 0) 'zero) 1)",
            "cond: a clause must be (test expression ...), (test => receiver) or \
             (else expression ...), not 1",
        ),
        (
            "(cond ())",
            "cond: a clause must be (test expression ...), (test => receiver) or \
             (else expression ...), not ()",
        ),
        (
            "(cond ((= 1 0) 'zero) (else 'many) ((= 1 1) 'one))",
            "`else` is a syntactic keyword",
        ),
    ];
    for (program, expected) in cases {
        for (backend, rendered) in rendered_on_both(program, "clauses.scm") {
            assert!(
                rendered.contains(expected),
                "[{backend}] {program}\nexpected: {expected}\ngot: {rendered}"
            );
            assert!(
                rendered.contains("  at clauses.scm:1:1"),
                "[{backend}] {program}\n{rendered}"
            );
        }
    }
}

/// A program that defines `else` and then writes it before `cond`'s last
/// clause means the variable, and runs: chibi and Gauche answer 1. A rule
/// diagnosing a mid-`else` in `cond` rejected it, because `else` still
/// matches the macro's literal after the program defines over the import,
/// where chibi and Gauche no longer match it (#450). Pinned so the diagnosis
/// is not added back while that is so.
#[test]
fn a_program_defined_else_before_conds_last_clause_is_its_variable() {
    let program = "(define else 3) (cond (else 1) (#t 2))";
    let tree_walker = TreeWalkInterpreter::new_tree_walker();
    let vm = Interpreter::new(VmBackend::new());
    assert_eq!(
        tree_walker.eval_program(program).unwrap().as_fixnum(),
        Some(1)
    );
    assert_eq!(vm.eval_program(program).unwrap().as_fixnum(), Some(1));
}

/// A use that no rule of a macro accepts says so once, naming the macro. It
/// used to read "Invalid syntax: Macro expansion failed: Invalid syntax: No
/// matching pattern for macro …" (#432).
#[test]
fn a_use_no_rule_accepts_names_the_macro_once() {
    let program = "(define-syntax two (syntax-rules () ((_ a b) (list a b))))\n(two 1)";
    for (backend, rendered) in rendered_on_both(program, "two.scm") {
        assert!(
            rendered
                .contains("Invalid syntax: no `syntax-rules` pattern of `two` matches this use"),
            "[{backend}] {rendered}"
        );
        assert_eq!(
            rendered.matches("Invalid syntax").count(),
            1,
            "[{backend}] {rendered}"
        );
        assert!(
            rendered.contains("  at two.scm:2:1"),
            "[{backend}] {rendered}"
        );
    }
}

#[test]
fn tracked_eval_program_variants_reject_input_cut_short_inside_a_datum() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let err = interp.eval_program_tracked("42 (+ 1").expect_err("tracked");
    assert!(is_cut_short(&err), "{err}");
    let (result, _source_map) = interp.eval_program_with_source_name("42 (+ 1", "<test>");
    let err = result.expect_err("with source name");
    assert!(is_cut_short(&err), "{err}");
    // The forms before the cut have run by then, as they would under `load`.
    let (result, _source_map) =
        interp.eval_program_with_source_name("(define ran-before-cut 7)\n(define y (+", "<test>");
    assert!(is_cut_short(&result.expect_err("with source name")));
    assert_eq!(
        interp.eval_str("ran-before-cut").unwrap().as_fixnum(),
        Some(7)
    );
}
