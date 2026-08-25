//! Original test cases for the families of defects that Larceny's R7RS test
//! suites surfaced (Track L §L5.3, 2026-08-24).
//!
//! The suites themselves are LGPL and are not vendored — they run from a
//! reference checkout via `scripts/run_larceny_tests.sh`. Every program here
//! is written from scratch to exhibit the same *family* of problem, so the
//! repo carries its own MIT-licensed reproduction of each. The map from
//! family to the upstream test cases that found it is
//! `scheme_tests/reports/larceny_triage.md` (disposable once the queue is
//! empty); the durable record is Track L PRD §6.
//!
//! Pinning follows the crate's conventions:
//! - a wrong answer both backends agree on is asserted *as-is*, with a message
//!   saying what to do when it converges (the test fails when the bug is
//!   fixed, which is the point);
//! - a backend divergence goes through `assert_divergence`, or explicit
//!   per-backend assertions when the broken side returns a value rather than
//!   failing;
//! - a crash or hang cannot be asserted at all, so those carry the *correct*
//!   expectation under `#[ignore]` with the reason; run them with
//!   `cargo test -p patina-tests --test larceny_families -- --ignored` when
//!   working on the fix, and drop the attribute when they pass.

mod common;

use common::{
    ErrorClass, On, assert_divergence, assert_program_eval_error, assert_program_eval_to,
    eval_program, eval_program_tree_walker, eval_program_vm, scratch_path,
};
use tempfile::TempDir;

const TRIAGE: &str = "scheme_tests/reports/larceny_triage.md";

// ---------------------------------------------------------------------------
// Family 1 — a nested `include` resolves against the wrong directory
// ---------------------------------------------------------------------------

/// `outer.scm` (included by absolute path) includes `sub/middle.scm` by
/// absolute path, and `middle.scm` includes `"leaf.scm"` relatively. Every
/// implementation that runs Larceny's `base` suite resolves that last one
/// beside `middle.scm`; Patina used to look in the first file the source map
/// happened to yield, then the cwd, and find nothing.
///
/// Fixed 2026-08-24: the desugarer keeps a stack of include directories.
#[test]
fn a_nested_include_resolves_relative_to_the_including_file() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
    let middle = scratch_path(&dir, "sub/middle.scm");
    std::fs::write(
        dir.path().join("outer.scm"),
        format!("(include \"{middle}\")"),
    )
    .expect("write outer");
    std::fs::write(&middle, "(include \"leaf.scm\")").expect("write middle");
    std::fs::write(
        dir.path().join("sub/leaf.scm"),
        "(define leaf-value 'found)",
    )
    .expect("write leaf");

    let program = format!(
        "(include \"{}\") leaf-value",
        scratch_path(&dir, "outer.scm")
    );
    assert_program_eval_to(&program, "found");
}

// ---------------------------------------------------------------------------
// Family 2 — `equal?` does not terminate on circular structures (R7RS 6.1)
// ---------------------------------------------------------------------------

/// Two distinct cyclic lists with the same unrolling: period 2 against
/// period 4. `(equal? a a)` is fine because `eq?` short-circuits; this one
/// looped forever until 2026-08-24 (an explicit worklist with a lazily
/// allocated visited set now).
#[test]
fn equal_terminates_on_two_distinct_cyclic_lists() {
    assert_program_eval_to(
        "(define a (list 1 2))       (set-cdr! (cdr a) a)
         (define b (list 1 2 1 2))   (set-cdr! (cdddr b) b)
         (equal? a b)",
        "#t",
    );
}

/// The vector shape of the same defect overflowed the Rust stack instead of
/// hanging, which is why Larceny's `read` suite died rather than stalling.
/// Also asserts the negative: a cycle of a different shape is not equal.
#[test]
fn equal_terminates_on_two_distinct_cyclic_vectors() {
    assert_program_eval_to(
        "(define (cyc x) (let ((v (vector x #f))) (vector-set! v 1 v) v))
         (define a (list 1 2)) (set-cdr! (cdr a) a)
         (list (equal? (cyc 1) (cyc 1)) (equal? (cyc 1) (cyc 2)) (equal? a (cyc 1)))",
        "(#t #f #f)",
    );
}

// ---------------------------------------------------------------------------
// Family 3 — `delay-force` is not iterative (R7RS 7.3)
// ---------------------------------------------------------------------------

/// The reason `delay-force` exists is that a chain of them runs in bounded
/// space. Ours recursed per link and overflowed at a hundred thousand until
/// 2026-08-24 (R7RS 7.3's iterative `force`, inner promise aliased to the
/// outer's box).
#[test]
fn a_long_delay_force_chain_runs_in_bounded_space() {
    assert_program_eval_to(
        "(import (scheme lazy))
         (define (count-down n)
           (if (= n 0) (delay 'done) (delay-force (count-down (- n 1)))))
         (force (count-down 100000))",
        "done",
    );
}

// ---------------------------------------------------------------------------
// Family 4 — VM: a discarded call to `values` poisons the next
//            `call-with-values`
// ---------------------------------------------------------------------------

/// `values` called with one argument in a non-tail position, result thrown
/// away; the next `call-with-values` whose producer returns a plain value
/// must not see it. Until 2026-08-25 the VM kept multiple values in a side
/// buffer that a discarded `values` call left set; multiple values are now
/// only ever a #<values> object in the result register, as on the
/// tree-walker.
#[test]
fn a_discarded_values_call_does_not_leak_into_call_with_values() {
    assert_program_eval_to(
        "(define (call1 f) (f 42))
         (call1 values)
         (call-with-values (lambda () 'fresh) (lambda xs xs))",
        "(fresh)",
    );
}

/// The shapes the removed buffer used to carry, now through the value
/// itself: a producer that calls `values` for effect and then returns
/// something else, several values, and a primitive that returns several.
#[test]
fn call_with_values_sees_only_what_its_producer_returned() {
    assert_program_eval_to(
        "(define (call1 f) (f 1 2 3))
         (list (call-with-values (lambda () (call1 values) 'one) (lambda xs xs))
               (call-with-values (lambda () (values 1 2)) list)
               (call-with-values (lambda () (exact-integer-sqrt 17)) list))",
        "((one) (1 2) (4 1))",
    );
}

/// Tree-walker: a continuation invoked with other than one value. The VM
/// delivers a #<values> object, as `(values …)` would return one, and
/// chibi gives `(4 5)`; the tree-walker raises an arity error.
#[test]
fn a_continuation_invoked_with_two_values_on_the_tree_walker() {
    assert_divergence(
        "(call-with-values (lambda () (call/cc (lambda (k) (k 4 5)))) list)",
        On::Vm,
        "(4 5)",
        ErrorClass::AtRuntime,
        TRIAGE,
    );
}

/// Tree-walker: `(values)` reaches the consumer as one unspecified value
/// instead of none. Not `assert_divergence` — the tree-walker returns a
/// plausible wrong answer rather than failing.
#[test]
fn zero_values_reach_the_consumer_as_no_arguments() {
    const PROGRAM: &str = "(call-with-values (lambda () (values)) (lambda xs xs))";
    assert_eq!(
        eval_program_vm(PROGRAM),
        "()",
        "the VM matches chibi; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_tree_walker(PROGRAM),
        "(#<unspecified>)",
        "\n[tree-walker] NO LONGER DIVERGES — zero values now arrive as none.\n\
         Replace both assertions with assert_program_eval_to(PROGRAM, \"()\") \
         and update {TRIAGE} family 18."
    );
}

// ---------------------------------------------------------------------------
// Family 5 — tree-walker: SRFI 1's n-ary procedures raise a wrong-arity error
// ---------------------------------------------------------------------------

/// `zip` is `(apply map list list1 more-lists)` inside `srfi-1-reference.scm`,
/// against SRFI 1's own n-ary `map`. The same `apply` shape written at top
/// level against `(scheme base)`'s `map` works on the tree-walker, so the
/// fault is in applying the library-internal definition.
#[test]
fn srfi_1_zip_with_two_lists_on_the_tree_walker() {
    assert_divergence(
        "(import (scheme list))
         (zip '(1 2 3) '(4 5 6))",
        On::Vm,
        "((1 4) (2 5) (3 6))",
        ErrorClass::AtRuntime,
        TRIAGE,
    );
}

// ---------------------------------------------------------------------------
// Family 6 — Unicode case mapping beyond the one-to-one table
// ---------------------------------------------------------------------------

/// Fixed 2026-08-24: the *simple* Unicode mappings (R7RS 6.6) — the full one
/// where it is a single character, the tabled simple one where it expands
/// (İ → i, ᾀ → ᾈ), the character itself where there is none (ß); the `-ci`
/// comparisons compare simple foldings (ẞ folds to ß), `string-ci=?` full
/// foldings.
#[test]
fn case_mapping_of_characters_without_a_single_character_mapping() {
    assert_program_eval_to(
        "(import (scheme char))
         (list (char-upcase #\\ß)
               (char-foldcase #\\ß)
               (char-foldcase #\\x1E9E)
               (char-downcase #\\x130)
               (char-upcase #\\x1F80)
               (char-ci=? #\\ς #\\σ)
               (char-ci=? #\\ß #\\x1E9E)
               (char-ci<? #\\ß #\\t)
               (string-ci=? \"Straße\" \"STRASSE\"))",
        "(#\\ß #\\ß #\\ß #\\i #\\ᾈ #t #t #f #t)",
    );
}

// ---------------------------------------------------------------------------
// Family 7 — `string->number` gaps the reader does not have
// ---------------------------------------------------------------------------

/// The reader accepts `+inf.0`, `-nan.0`, `1+2i` and `#e1e400` as literals;
/// `string->number` rejected the first three and read the last through a
/// float. Fixed 2026-08-24: `string->number` *is* the reader's number
/// syntax now (the whole string must lex as one number token), and `#e` on
/// a decimal is the exact value of the text.
#[test]
fn string_to_number_accepts_what_the_reader_accepts() {
    assert_program_eval_to(
        "(list (string->number \"+inf.0\")
               (string->number \"-nan.0\")
               (string->number \"1+2i\")
               (exact? (string->number \"#e1e400\"))
               (= (string->number \"#e1e400\") (expt 10 400))
               (string->number \"#e1.5\")
               (string->number \"1F\" 16)
               (string->number \"#x1F\" 10)
               (string->number \"abc\")
               (string->number \"1 2\")
               (string->number \" 12\")
               (string->number \"\"))",
        "(+inf.0 +nan.0 1+2i #t #t 3/2 31 31 #f #f #f #f)",
    );
}

// ---------------------------------------------------------------------------
// Family 8 — `rationalize` at the infinities (R7RS 6.2.6)
// ---------------------------------------------------------------------------

/// `(rationalize +inf.0 3)` is `+inf.0` and `(rationalize 3 +inf.0)` is
/// `0.0` — the second is exact `0` today, the first `0.0`.
///
/// Fixed 2026-08-24.
#[test]
fn rationalize_with_an_infinite_argument() {
    assert_program_eval_to(
        "(import (scheme inexact))
         (list (rationalize +inf.0 3) (rationalize 3 +inf.0) (rationalize -inf.0 1))",
        "(+inf.0 0.0 -inf.0)",
    );
}

// ---------------------------------------------------------------------------
// Family 9 — `environment` rejects a nested import set
// ---------------------------------------------------------------------------

/// `(prefix (only …) …)` is an ordinary import set; `environment` should
/// accept whatever `import` accepts.
///
/// **When this stops erroring, replace the assertion with
/// `assert_program_eval_to(program, "1")` and update the triage doc.**
#[test]
fn environment_accepts_a_nested_import_set() {
    let program = "(import (scheme eval))
                   (eval '(p:car '(1 2))
                         (environment '(prefix (only (scheme base) car) p:)))";
    assert_program_eval_error(program);
}

// ---------------------------------------------------------------------------
// Family 10 — `input-port-open?` on an output port is an error, not `#f`
// ---------------------------------------------------------------------------

/// R7RS 6.13.1: `input-port-open?` "returns #t if port is still open and
/// capable of performing input" — for an output-only port that is `#f`, not
/// a type error. Larceny's `file` suite maps every port predicate over a
/// freshly opened binary port and dies here.
///
/// Fixed 2026-08-24: `#f` for the other direction, on both predicates.
#[test]
fn input_port_open_on_an_output_only_port_is_false() {
    let dir = TempDir::new().expect("temp dir");
    let path = scratch_path(&dir, "out.bin");
    let program = format!(
        "(import (scheme file))
         (define p (open-binary-output-file \"{path}\"))
         (define q (open-input-string \"\"))
         (list (output-port-open? p) (input-port-open? p)
               (input-port-open? q) (output-port-open? q))"
    );
    assert_program_eval_to(&program, "(#t #f #t #f)");
}

// ---------------------------------------------------------------------------
// Family 14 — a shadowed `...` is no longer the ellipsis (R7RS 4.3.2)
// ---------------------------------------------------------------------------

/// Where `...` is bound as a variable, a `syntax-rules` written in that scope
/// has no ellipsis: `(_ a b ...)` is a three-variable pattern (R7RS 4.3.2
/// identifies the ellipsis by binding). chibi, Larceny, Kawa and Sagittarius
/// agree; Gauche rejects the definition, as Patina does. This blocks
/// Larceny's `base` suite at load time.
///
/// Two attempts were backed out after review (#111, #114): a whole-macro
/// sentinel ellipsis, and a per-token rule keyed on the enclosing bindings'
/// scope sets. Both fail the same way — they cannot tell a token's *own*
/// scopes from the ones the template compiler unions in — so a bound `...`
/// captured an outer macro's `(... ...)` escape, a generated macro's
/// ellipsis, or (in the second attempt) a SRFI 46 `:::`. The fix is a
/// binding-resolution the desugarer and the macro compiler share, ordered
/// innermost-first, which is `PRD/macro/SYNTAX_KEYWORD_BINDINGS_DESIGN.md`'s
/// project; the cases in the triage doc's family 14 are its acceptance
/// tests.
///
/// **When this stops erroring, replace the assertion with
/// `assert_program_eval_to(program, "((2 1 3) (1))")` and update the triage
/// doc.**
#[test]
fn a_shadowed_ellipsis_is_an_ordinary_pattern_variable() {
    let program = "(define-syntax def-first
                     (syntax-rules ()
                       ((_ name) (define-syntax name (syntax-rules () ((_ a b (... ...)) (list a)))))))
                   (let ((... 'dots))
                     (define-syntax swap-first-two
                       (syntax-rules () ((_ a b ...) (list b a ...))))
                     (def-first first-of)
                     (list (swap-first-two 1 2 3) (first-of 1 2 3 4)))";
    assert_program_eval_error(program);
}

// ---------------------------------------------------------------------------
// Family 15 — a template's reference to a definition-site local that spells
//             a keyword is rejected as syntax
// ---------------------------------------------------------------------------

/// The macro is defined *inside* the binding, so its template's free `...`
/// (or `if`) is that local variable — the definition scopes say so. The
/// syntax-as-a-value check is by spelling and exempts scoped references
/// (hygiene: an *outer* macro's `if` must stay the special form), so it
/// reports the keyword instead. A containment test on the enclosing
/// bindings' scope sets (#114) was backed out: it also captured a parameter
/// named `if` in a `define` shorthand, let an outer variable veto an inner
/// `let-syntax`, and broke macro-generating macros. Same project as family
/// 14; this is what gates Larceny's `base` suite behind it.
///
/// **When this stops erroring, replace the assertion with
/// `assert_program_eval_to(program, "((1 dots) (2 nineteen) outer-if-is-syntax)")`
/// and update the triage doc.**
#[test]
fn a_template_may_refer_to_a_definition_site_local_spelled_like_a_keyword() {
    let program = "(define-syntax my-if (syntax-rules () ((_ c a b) (if c a b))))
                   (let ((... 'dots) (if 'nineteen))
                     (define-syntax mention-dots (syntax-rules () ((_ a) (list a ...))))
                     (define-syntax mention-if (syntax-rules () ((_ a) (list a if))))
                     (list (mention-dots 1) (mention-if 2) (my-if #t 'outer-if-is-syntax 'no)))";
    assert_program_eval_error(program);
}

// ---------------------------------------------------------------------------
// Family 16 — a line comment is not ended by a bare return
// ---------------------------------------------------------------------------

/// R7RS 7.1.1: a line ending is newline, return, or return+newline, and a
/// `;` comment runs to the line ending. Ours ran to the newline only, so a
/// datum after a return-terminated comment was swallowed. Fixed 2026-08-24.
#[test]
fn a_line_comment_ends_at_a_bare_return() {
    assert_program_eval_to(
        "(import (scheme read))
         (let ((p (open-input-string \"first ; comment\\rsecond ; another\\r\\nthird\")))
           (list (read p) (read p) (read p) (eof-object? (read p))))",
        "(first second third #t)",
    );
}

// ---------------------------------------------------------------------------
// Review of #112 — the cases its review found, kept as they were verified
// ---------------------------------------------------------------------------

/// A promise's box can be re-pointed by a force nested inside its own thunk
/// (`promise_update` aliases the inner promise to the outer's box). The
/// outer force must then look its box up again rather than store into the
/// one it captured before the thunk: R7RS 7.3's reference gives (2 2 2).
#[test]
fn a_force_reentered_through_promise_update_memoizes_once() {
    assert_program_eval_to(
        "(import (scheme lazy))
         (define n 0)
         (define q #f)
         (define p (delay-force q))
         (set! q (delay (let ((me (begin (set! n (+ n 1)) n)))
                          (if (= me 1) (force p))
                          me)))
         (list (force q) (force q) (force p))",
        "(2 2 2)",
    );
}

/// `(delay e)` wraps its value in a *done* promise (R7RS 7.3), so forcing a
/// delay whose value is a promise yields that promise, not its value —
/// forcing through is what `delay-force` is for.
#[test]
fn forcing_a_delay_of_a_promise_yields_the_promise() {
    assert_program_eval_to(
        "(import (scheme lazy))
         (define a (delay 7))
         (list (promise? (force (delay (delay 5))))
               (eq? (force (delay a)) a)
               (force (delay-force (delay 5))))",
        "(#t #t 5)",
    );
}

/// `equal?` walks record fields on the same worklist as pairs and vectors,
/// so a cycle through a record terminates too.
#[test]
fn equal_terminates_through_a_record_field_cycle() {
    assert_program_eval_to(
        "(define-record-type <box> (mk v) box? (v box-v box-set-v!))
         (define a (mk #f)) (box-set-v! a a)
         (define b (mk #f)) (box-set-v! b b)
         (define c (mk 1))  (box-set-v! c (list c))
         (list (equal? a b) (equal? a c) (equal? (mk 1) (mk 1)) (equal? (mk 1) (mk 2)))",
        "(#t #f #t #f)",
    );
}

/// `string->number` is the reader's number syntax and nothing more: a
/// comment, a block comment or a `#!` line before the digits is not part
/// of a number; an exactness prefix applies to both parts of a complex; a
/// pure imaginary needs its sign; an exponent no bignum should hold is
/// refused rather than computed.
#[test]
fn string_to_number_is_exactly_one_number_token() {
    assert_program_eval_to(
        "(list (string->number \"1;2\")
               (string->number \"#|c|#1\")
               (string->number \"#!fold-case 1\")
               (string->number \"#e1.5+2i\")
               (string->number \"#i1+2i\")
               (string->number \"1i\")
               (string->number \"+1i\")
               (string->number \"#e1e1000000\")
               (string->number \"#e1.00e-9223372036854775807\")
               (string->number \"+123\")
               (string->number \"-99999999999999999999\"))",
        "(#f #f #f 3/2+2i 1.0+2.0i #f +i #f #f 123 -99999999999999999999)",
    );
}

/// A shebang line, like a `;` comment, ends at a bare return.
#[test]
fn a_shebang_line_ends_at_a_bare_return() {
    assert_program_eval_to(
        "(import (scheme read))
         (read (open-input-string \"#!/usr/bin/env patina\\r42\"))",
        "42",
    );
}

// ---------------------------------------------------------------------------
// Family 19 — the macro expander's walkers never returned on a cyclic datum
// ---------------------------------------------------------------------------

/// A quoted datum with labels is a legitimate macro argument (Larceny's
/// `base` suite hands `test` several), and until 2026-08-25 the expander's
/// identifier scan and scope flip walked the cycle forever — the whole suite
/// hung at load. Cycles only come from the reader, so a revisited pair holds
/// no identifiers and is skipped.
#[test]
fn a_cyclic_quoted_datum_can_be_a_macro_argument() {
    assert_program_eval_to(
        "(define-syntax same? (syntax-rules () ((_ x y) (equal? x y))))
         (list (same? '#0=(a b . #0#) '#1=(a b a b . #1#))
               (same? '#2=(a b . #2#) '#3=(a b c . #3#)))",
        "(#t #f)",
    );
}

/// A datum label's scope is the outermost datum it appears in (R7RS 2.4).
/// The parser that reads a whole program datum by datum (`parse`, used by
/// the script runner and `eval_program`; `read` builds a fresh parser per
/// call and never had the problem) kept the table across data and rejected
/// a reused label — and a `#n#` whose `#n=` never came was left in the
/// datum as a placeholder object rather than reported.
#[test]
fn datum_labels_are_scoped_to_one_datum() {
    assert_program_eval_to(
        "(define a '#0=(x . #0#))
         (define b '#0=(y . #0#))
         (list (car a) (car b) (eq? a (cdr a)))",
        "(x y #t)",
    );
    assert_program_eval_error("(define a '#0=(x . #0#)) (define b '(y #0# z)) b");
}

/// The expander's scope flip copies a macro argument pair by pair; the copy
/// must share where the original shared and close on itself where the
/// original did — a memo from the first pair — rather than splice the
/// original's tail in after a budget, which lost `eq?` identity across the
/// cycle and left `write` a shape it could not print.
#[test]
fn a_flipped_cyclic_argument_is_a_closed_copy() {
    assert_program_eval_to(
        "(define-syntax both (syntax-rules () ((_ x) (list x x))))
         (let ((v '#0=(1 . #0#)))
           (list (eq? v (cdr v))
                 (let ((w (car (both v)))) (eq? w (cdr w)))
                 (let ((p (both '#1=(a b . #1#)))) (eq? (car p) (cadr p)))))",
        "(#t #t #t)",
    );
}

// ---------------------------------------------------------------------------
// What `base` found once it ran (2026-08-25) — each pinned as it is today,
// so the fix trips the test. chibi and Gauche agree on every expectation.
// ---------------------------------------------------------------------------

/// R7RS 4.2.2: `let-values` evaluates every init in the outer environment
/// before binding any of the formals; ours bound each clause before
/// evaluating the next, like `let*-values`, until 2026-08-25 (now the
/// reference implementation of R7RS 7.3). Also covers a dotted and a rest
/// formal. (A `(() (values))` clause works on the VM but not the
/// tree-walker — that is family 18, zero values arriving as one.)
#[test]
fn let_values_binds_all_clauses_in_parallel() {
    assert_program_eval_to(
        "(let ((a 'a) (b 'b) (x 'x) (y 'y))
           (list (let-values (((a b) (values x y)) ((x y) (values a b)))
                   (list a b x y))
                 (let-values (((p . q) (values 1 2 3)) (r (values 4 5)))
                   (list p q r))
                 (let*-values (((a b) (values x y)) ((x y) (values a b)))
                   (list a b x y))))",
        "((x y a b) (1 (2 3) (4 5)) (x y x y))",
    );
}

/// R7RS 4.2.7: a `guard` with no matching clause re-raises "in the dynamic
/// environment of the original call to `raise`" — so the `dynamic-wind`
/// before-thunk runs again on the way back in, and the after-thunk again
/// on the way out to the outer guard. Ours re-raises from the guard's own
/// environment and runs neither.
///
/// **When this converges on `(out in out in)`, replace with
/// `assert_program_eval_to` and update the triage doc.**
#[test]
fn a_guard_reraise_reenters_the_dynamic_extent() {
    assert_eq!(
        eval_program(
            "(define v '())
             (guard (exn ((equal? exn 5) 'five))
               (guard (exn ((equal? exn 6) 'six))
                 (dynamic-wind (lambda () (set! v (cons 'in v)))
                               (lambda () (raise 5))
                               (lambda () (set! v (cons 'out v))))))
             v"
        ),
        "(out in)",
        "expected the pinned wrong answer; if this is now (out in out in) the defect is fixed"
    );
}

/// R7RS 4.3.1: a `let-syntax` body is a body — its definitions are local
/// to it, not spliced into the enclosing one — and the macro names it binds
/// are visible in the transformers only under `letrec-syntax`. Ours
/// splices the definition out (`x` becomes 56 outside) and resolves a
/// `let-syntax` transformer's reference to a sibling keyword.
///
/// **When this converges on `((13 70) (1 2) (1 1))`, replace with
/// `assert_program_eval_to` and update the triage doc.**
#[test]
fn let_syntax_body_definitions_and_transformer_scope() {
    assert_eq!(
        eval_program(
            "(define (defs)
               (let ((x 13))
                 (define y 14)
                 (let-syntax ((def (syntax-rules () ((_ var val) (define var val)))))
                   (def x 56)
                   (set! y (+ x y)))
                 (list x y)))
             (define (scope)
               (let ((f (lambda (x) (+ x 1))))
                 (let-syntax ((f (syntax-rules () ((f x) x)))
                              (g (syntax-rules () ((g x) (f x)))))
                   (list (f 1) (g 1)))))
             (define (rec-scope)
               (let ((f (lambda (x) (+ x 1))))
                 (letrec-syntax ((f (syntax-rules () ((f x) x)))
                                 (g (syntax-rules () ((g x) (f x)))))
                   (list (f 1) (g 1)))))
             (list (defs) (scope) (rec-scope))"
        ),
        "((56 70) (2 1) (2 1))",
        "expected the pinned wrong answer; if this is now ((13 70) (1 2) (1 1)) the defect is fixed"
    );
}

/// `with-exception-handler` takes a continuation as its handler — the R7RS
/// idiom for capturing a raised object, `(call/cc (lambda (k)
/// (with-exception-handler k …)))`. The VM used to reject it ("expected a
/// procedure, got object") until 2026-08-25: its type check asked for a
/// procedure, and its generic call path could not invoke a continuation.
/// With the object in hand, `read-error?` and `file-error?` answer `#f` as
/// R7RS 6.11 requires.
#[test]
fn a_continuation_is_a_procedure_for_with_exception_handler() {
    assert_program_eval_to(
        "(define e (call/cc (lambda (k) (with-exception-handler k (lambda () (error \"plain\"))))))
         (list (error-object? e) (error-object-message e)
               (read-error? e) (file-error? e) (read-error? 42) (file-error? 'x)
               (call/cc (lambda (k) (with-exception-handler k (lambda () (raise 'obj))))))",
        "(#t \"plain\" #f #f #f #f obj)",
    );
}

/// `read-line` ends a line at a bare return as well as at a newline and at
/// return+newline (R7RS 6.13.2 defers to 7.1.1's line endings; chibi and
/// Gauche split all three). Fixed 2026-08-25; a return+newline pair is one
/// ending, so the second line of "abc\r\ndef" is "def", not "".
#[test]
fn read_line_ends_at_a_bare_return() {
    assert_program_eval_to(
        "(define (lines s)
           (let ((p (open-input-string s)))
             (let loop ((acc '()))
               (let ((l (read-line p)))
                 (if (eof-object? l) (reverse acc) (loop (cons l acc)))))))
         (map lines '(\"abc\\ndef\" \"abc\\rdef\" \"abc\\r\\ndef\" \"abc\\r\" \"\\r\\n\\n\"))",
        "((\"abc\" \"def\") (\"abc\" \"def\") (\"abc\" \"def\") (\"abc\") (\"\" \"\"))",
    );
}
