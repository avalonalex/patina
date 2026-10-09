//! Syntactic keywords are bindings, not spellings.
//!
//! `begin`, `if`, `lambda` and 20 others used to be recognized by name, in
//! every scope, unconditionally — they had no entry in any environment. Import
//! sets and export resolution are binding-based, so neither could reach them,
//! and the consequences showed up in six unrelated-looking places. These pin
//! the ones stage 1 closes. What stage 2 still owes is narrower than the design
//! predicted — only that a *bare* spelling resolves after an import set has
//! excluded or moved it — and it is pinned as *still lenient* below, so that
//! stage flipping it is a visible change rather than a silent one.
//!
//! Design and staging: `PRD/macro/SYNTAX_KEYWORD_BINDINGS_DESIGN.md`.
//! Every helper here runs both backends.
//!
//! **Three rows moved** to `tests/scheme/expansion/keyword-bindings.scm` (#193
//! Phase 2) — local bindings shadowing a keyword, a rebound `else`, and a
//! keyword renamed on import — where chibi and Gauche answer them too. What
//! stays rebinds a keyword at the top level, builds a `define-library`, or
//! imports at the top level, none of which a shared suite file can do without
//! changing every row after it.

mod common;
use common::{assert_program_eval_error, assert_program_eval_to};

// ============================================================================
// A definition shadows a keyword (R7RS §5.3.1)
// ============================================================================

/// R7RS §5.3.1 is normative and names this exact case: "if ⟨variable⟩ is not
/// bound, *or is a syntactic keyword*, then the definition will bind
/// ⟨variable⟩ to a new location before performing the assignment."
///
/// Patina used to answer `2` here, because the desugarer's name `match` claimed
/// the form before anything asked what `if` was bound to.
#[test]
fn test_define_shadows_a_syntactic_keyword() {
    assert_program_eval_to(
        "(import (scheme base))
         (define (if a b c) (list 'proc a b c))
         (if 1 2 3)",
        "(proc 1 2 3)",
    );
}

/// The half that always worked, kept beside it: the two must not disagree
/// again. A macro binding won because macros *were* looked up; that asymmetry
/// is the whole diagnosis.
#[test]
fn test_define_syntax_also_shadows_a_syntactic_keyword() {
    assert_program_eval_to(
        "(import (scheme base))
         (define-syntax if (syntax-rules () ((_ a b c) (list 'mymacro a b c))))
         (if 1 2 3)",
        "(mymacro 1 2 3)",
    );
}

/// `define-library` and `library` are not keywords: no library binds them, and
/// a top-level form headed by either defines a library only while the name is
/// unbound. Bound to a procedure, a macro or an import, the form is a call or
/// a use, as chibi 0.12 and Gauche 0.9.15 answer every row here (#610). The
/// backends used to send the form to the library loader by its spelling, which
/// then refused it as a malformed library.
#[test]
fn a_top_level_form_headed_by_a_bound_library_keyword_is_not_a_library() {
    for name in ["define-library", "library"] {
        assert_program_eval_to(
            &format!(
                "(import (scheme base))
                 (define ({name} . args) (list 'called args))
                 ({name} 1 2)"
            ),
            "(called (1 2))",
        );
    }
    assert_program_eval_to(
        "(import (scheme base))
         (define-syntax define-library
           (syntax-rules () ((_ x ...) (list 'expanded x ...))))
         (define-library 1 2)",
        "(expanded 1 2)",
    );
    assert_program_eval_to(
        "(import (scheme base) (rename (only (scheme base) list) (list define-library)))
         (define-library 1 2)",
        "(1 2)",
    );
}

/// The other half: unbound where the form is evaluated, the name defines a
/// library, even after a program bound it in a body, where the binding does
/// not reach the top level.
#[test]
fn a_top_level_form_headed_by_an_unbound_library_keyword_is_a_library() {
    assert_program_eval_to(
        "(import (scheme base))
         (define (f) (define (define-library . args) args) (define-library 'inner))
         (define-library (test unbound-keyword)
           (export x)
           (import (scheme base))
           (begin (define x 'loaded)))
         (import (test unbound-keyword))
         (list x (f))",
        "(loaded (inner))",
    );
}

// #463: a begin is expanded before it executes, but definitions must already
// affect subsequent expansion. Fresh programs keep these top-level keyword
// redefinitions from poisoning unrelated Scheme suite rows. Chibi 0.12 and
// Gauche 0.9.15 agree on the original three heads and recursive definitions.
#[test]
fn top_level_begin_definitions_shadow_heads_and_value_references() {
    for (name, formals, call) in [
        ("when", "x", "(when 1)"),
        ("if", "a b c", "(if 1 2 3)"),
        ("apply", "f xs", "(apply + '(1 2))"),
    ] {
        assert_program_eval_to(
            &format!(
                "(import (scheme base))
                      (begin (define ({name} {formals}) 'mine)
                             (list (procedure? {name}) {call}))"
            ),
            "(#t mine)",
        );
    }
}

#[test]
fn top_level_definition_is_visible_in_its_own_body() {
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define (apply f xs)
                  (if (null? xs) 'mine (apply f (cdr xs))))
                (apply + '(1 2)))",
        "mine",
    );
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define when (lambda (n) (if (= n 0) 'mine (when (- n 1)))))
                (when 2))",
        "mine",
    );
}

#[test]
fn top_level_definitions_do_not_reexpand_earlier_function_bodies() {
    assert_program_eval_to(
        "(import (scheme base))
         (define-syntax k (syntax-rules () ((_) 'old)))
         (begin (define (call-k) (k))
                (define (k) 'new)
                (call-k))",
        "old",
    );
}

#[test]
fn top_level_declaration_does_not_overwrite_a_runtime_value() {
    // Preserve Patina's existing value/assignment behavior (also Gauche's).
    // Chibi instead exposes an uninitialized location in this initializer.
    assert_program_eval_to(
        "(import (scheme base))
         (define x 10)
         (begin (define x (+ x 1)) (define x (+ x 1)) x)",
        "12",
    );
}

#[test]
fn nested_and_generated_top_level_definitions_take_effect() {
    assert_program_eval_to(
        "(import (scheme base))
         (define-syntax def (syntax-rules () ((_ name) (define (name x) 'mine))))
         (begin (begin (def when)) (when 1))",
        "mine",
    );
    assert_program_eval_to(
        "(import (scheme base))
         (define-syntax run
           (syntax-rules ()
             ((_ result) (begin (define (when x) 'private)
                                (define result (when 1))))))
         (begin (run result) (list result (when #t 'public)))",
        "(private public)",
    );
}

#[test]
fn top_level_variable_and_syntax_declarations_follow_source_order() {
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define (k) 'variable)
                (define-syntax k (syntax-rules () ((_) 'macro)))
                (k))",
        "macro",
    );
    // Chibi follows the later variable declaration. Gauche keeps expanding
    // the macro in this same-begin syntax/variable redefinition case.
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define-syntax k (syntax-rules () ((_) 'macro)))
                (define (k) 'variable)
                (k))",
        "variable",
    );
}

#[test]
fn top_level_declarations_participate_in_literal_matching() {
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define else #f) (cond (else 'wrong) (#t 'right)))",
        "right",
    );
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define token #f)
                (define-syntax m (syntax-rules (token)
                  ((_ token) 'literal) ((_ x) 'other)))
                (m token))",
        "literal",
    );
}

#[test]
fn macro_compilation_sees_declared_variables_but_retains_the_live_environment() {
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define ... #f)
                (define-syntax m (syntax-rules () ((_ x ...) (list ... x))))
                (m 1 2))",
        "(2 1)",
    );
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define (when x) 'first)
                (define-syntax m (syntax-rules () ((_) (when 1)))))
         (set! when (lambda (x) 'later))
         (m)",
        "later",
    );
}

#[test]
fn top_level_declarations_preserve_imported_macro_bindings() {
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define (if a b c) 'mine)
                (list (if 1 2 3) (when #t 'library)))",
        "(mine library)",
    );
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define (when x) 'mine)
                (let-syntax ((m (syntax-rules () ((_) (when 1))))) (m)))",
        "mine",
    );
}

#[test]
fn top_level_declarations_work_in_libraries_and_eval() {
    assert_program_eval_to(
        "(define-library (test declarations)
           (import (scheme base)) (export result)
           (begin (begin (define (when x) 'library)
                         (define result (when 1)))))
         (import (test declarations)) result",
        "library",
    );
    assert_program_eval_to(
        "(import (scheme base) (scheme eval) (scheme repl))
         (eval '(begin (define (when x) 'evaluated) (when 1))
               (interaction-environment))",
        "evaluated",
    );
}

#[test]
fn imports_supersede_only_the_declarations_they_install() {
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define (when x) 'mine)
                (import (only (scheme base) when))
                (when #t 'imported))",
        "imported",
    );
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define (when x) 'mine)
                (import (only (scheme base) quote))
                (when 1))",
        "mine",
    );
    assert_program_eval_to(
        "(define-library (test mutable-declaration)
           (import (scheme base)) (export value change!)
           (begin (define value 1) (define (change!) (set! value 42))))
         (import (scheme base))
         (begin (define pending #f)
                (import (test mutable-declaration))
                (change!) value)",
        "42",
    );
}

#[test]
fn top_level_declarations_keep_lexical_syntax_and_splicing_scopes() {
    assert_program_eval_to(
        "(import (scheme base))
         (begin (define (when x) 'global)
                (list (let-syntax ((when (syntax-rules () ((_) 'local)))) (when))
                      (when 1)))",
        "(local global)",
    );
    assert_program_eval_to(
        "(import (scheme base) (srfi 188))
         (begin (splicing-let-syntax () (define (when x) 'spliced))
                (when 1))",
        "spliced",
    );
}

#[test]
fn failed_expansion_does_not_leave_variable_declarations_behind() {
    fn check<B: patina_runtime::Backend>(interpreter: patina_interpreter::Interpreter<B>) {
        interpreter
            .eval_program("(import (scheme base)) (define x 10)")
            .unwrap();
        assert!(interpreter.eval_str(
            "(begin (define x 99) (define (if a b c) 'mine) (syntax-error \"stop\"))",
        ).is_err());
        let value = interpreter.eval_str("(if #t x 0)").unwrap();
        assert_eq!(value, patina_core::TaggedValue::fixnum(10));
    }
    check(common::tree_walker_interpreter());
    check(common::vm_interpreter());
}

#[test]
fn import_initializers_cannot_observe_expansion_placeholders() {
    // File libraries initialize on import; inline define-library initializes
    // eagerly, before the callback below has been installed.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("test")).unwrap();
    std::fs::write(
        dir.path().join("test/declaration-observer.sld"),
        "(define-library (test declaration-observer)
           (import (scheme base) (test declaration-callback)) (export observed)
           (begin (define observed (read-value))))",
    )
    .unwrap();
    let program = "(define-library (test declaration-callback)
           (import (scheme base)) (export install! read-value)
           (begin (define getter #f)
                  (define (install! proc) (set! getter proc))
                  (define (read-value) (getter))))
         (import (scheme base) (test declaration-callback))
         (define x 10)
         (install! (lambda () x))
         (begin (define x (+ x 1))
                (import (test declaration-observer))
                (+ x observed))";
    let vm = common::vm_interpreter();
    vm.backend()
        .add_library_search_path(dir.path().to_path_buf());
    assert_eq!(vm.eval_program(program).unwrap().as_fixnum(), Some(21));
    let tw = common::tree_walker_interpreter();
    tw.backend()
        .add_library_search_path(dir.path().to_path_buf());
    assert_eq!(tw.eval_program(program).unwrap().as_fixnum(), Some(21));
}

// ============================================================================
// Auxiliary syntax
// ============================================================================

/// In head position an auxiliary keyword is a mistake, and saying which beats
/// reporting that a symbol is not a procedure — which is what `(else 1)` used
/// to report, via the variable binding.
#[test]
fn test_auxiliary_syntax_in_head_position_is_an_error() {
    assert_program_eval_error("(import (scheme base)) (else 1 2)");
    assert_program_eval_error("(import (scheme base)) (=> 1 2)");
}

// That `else` and `=>` still do their real job — matching as `syntax-rules`
// literals inside `cond` and `case` — is already covered on both backends by
// `tests/scheme/expansion/derived-forms.scm` ("cond falls through to else",
// "cond's => passes the test's value", "case falls through to else"). Not
// restated here: those are the regression guards
// for `cond`/`case`, and a second copy only splits the failure across two
// files.

// ============================================================================
// Keywords travel through import sets
// ============================================================================

// A renamed keyword is the *same* interned marker, not a copy: pinned by
// `core_syntax_is_interned_per_heap` in `patina-core`, which is where it moved
// when `(eqv? begin blk)` stopped evaluating.

/// `only` selecting a keyword works because there is now something to select.
/// It used to *reject the program* — "Identifier 'begin' not found in import
/// set" — which is what the first consumer of a blanket re-export does.
#[test]
fn test_only_can_select_a_keyword() {
    assert_program_eval_to(
        "(import (only (scheme base) begin quote list) (scheme write))
         (begin (list 1 2))",
        "(1 2)",
    );
}

// ============================================================================
// Stage 2 — an import set scopes keywords like any other binding
// ============================================================================

/// The keyword form of the rule stage 2 exists for: a library gets syntax only
/// by importing it. `begin` is excluded here, so the body cannot use it — which
/// is chibi's and Gauche's answer, and was impossible to reach while the
/// desugarer recognized keywords by spelling wherever they were unbound.
#[test]
fn test_except_hides_a_keyword_from_a_library() {
    assert_program_eval_error(
        r#"
        (define-library (syn ex)
          (import (except (scheme base) begin))
          (export go)
          (begin (define (go) (begin 1 2))))
        (import (syn ex))
        (go)
        "#,
    );
}

/// And `prefix` moves it rather than duplicating it: the prefixed name works
/// (that much was true from stage 1) and the bare one no longer does.
#[test]
fn test_prefix_moves_a_keyword_out_of_the_bare_spelling() {
    assert_program_eval_to(
        r#"
        (define-library (syn pfx)
          (import (prefix (scheme base) s:))
          (export go)
          ;; The outer `begin` is a library declaration, which the .sld parser
          ;; reads structurally and no import set touches. Everything inside it
          ;; is ordinary code and must use the prefixed names.
          (begin (s:define (go) (s:begin 1 2))))
        (import (syn pfx))
        (go)
        "#,
        "2",
    );
    assert_program_eval_error(
        r#"
        (define-library (syn pfx2)
          (import (prefix (scheme base) s:))
          (export go)
          (begin (s:define (go) (begin 1 2))))
        (import (syn pfx2))
        (go)
        "#,
    );
}

/// `(null-environment 5)` is null now. It used to admit any R7RS form the
/// desugarer knew by spelling — `cond-expand`, `include`, `import`,
/// `syntax-error` — because none of them was bound there to be left out.
#[test]
fn test_null_environment_admits_only_r5rs_syntax() {
    assert_program_eval_error(
        "(import (scheme base) (scheme eval) (scheme r5rs))
         (eval '(cond-expand (else 42)) (null-environment 5))",
    );
    // The R5RS keywords it *should* have still work, so the environment is
    // narrowed rather than empty.
    assert_program_eval_to(
        "(import (scheme base) (scheme eval) (scheme r5rs) (scheme write))
         (eval '(begin 1 2) (null-environment 5))",
        "2",
    );
}

// ============================================================================
// The top level is not covered by any of the above, and not because of keywords
// ============================================================================

/// At the top level an import set does not remove anything, because
/// `load_bootstrap` seeds every `(scheme base)` export into the global
/// environment before a program runs — that is what lets a script with no
/// `import` at all work, which Patina supports and chibi does not.
///
/// **Not a keyword defect**, which is the point of asserting `car` beside
/// `begin`: an ordinary procedure survives `except` exactly the same way. The
/// design doc listed the top-level `(except … begin)` case under stage 2, and
/// that was a misattribution — deleting the spelling fallback fixed the library
/// case above and could not have fixed this one. Its fix is to stop pre-seeding
/// the global environment, which is a deliberate REPL affordance and a separate
/// decision.
#[test]
fn test_a_top_level_import_set_removes_nothing_keyword_or_not() {
    assert_program_eval_to("(import (except (scheme base) begin)) (begin 1 2)", "2");
    assert_program_eval_to(
        "(import (except (scheme base) car) (scheme write)) (car (list 1 2))",
        "1",
    );
}
