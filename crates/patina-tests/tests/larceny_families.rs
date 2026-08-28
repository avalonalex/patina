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
    eval_program, scratch_path,
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

/// A continuation invoked with other than one value delivers a `#<values>`
/// object, as `(values …)` returns one — the VM since #113, the tree-walker
/// since 2026-08-25. The tree-walker used to raise a wrong-arity error, which
/// is what made SRFI 1's n-ary procedures unusable there (family 5).
#[test]
fn a_continuation_invoked_with_two_values_delivers_them() {
    assert_program_eval_to(
        "(call-with-values (lambda () (call/cc (lambda (k) (k 4 5)))) list)",
        "(4 5)",
    );
}

/// `(values)` reaches a consumer as no arguments, and `(k)` likewise —
/// R7RS 6.10, and what chibi and Gauche both answer. The tree-walker gave
/// `(#<unspecified>)` for the first until 2026-08-25, because the rule "one
/// value is itself, any other count is a #<values> object" was written out
/// in four places and the `values` primitive's copy special-cased zero. One
/// `Heap::values_from` now serves all four.
#[test]
fn zero_values_reach_the_consumer_as_no_arguments() {
    assert_program_eval_to(
        "(list (call-with-values (lambda () (values)) (lambda xs xs))
               (call-with-values (lambda () (call/cc (lambda (k) (k)))) (lambda xs xs)))",
        "(() ())",
    );
}

// ---------------------------------------------------------------------------
// Family 5 — tree-walker: SRFI 1's n-ary procedures raise a wrong-arity error
// ---------------------------------------------------------------------------

/// `zip` is `(apply map list list1 more-lists)` inside `srfi-1-reference.scm`,
/// against SRFI 1's own n-ary `map`. It failed on the tree-walker with a
/// wrong-arity error, and the cause turned out to be family 17, not `apply`:
/// SRFI 1's `%cars+cdrs` bails out of an exhausted list with
/// `(abort '() '())`, invoking a continuation with two values, which the
/// tree-walker refused. Fixed 2026-08-25 with that one; every n-ary
/// procedure that walks more than one list reaches the same abort.
#[test]
fn srfi_1_n_ary_procedures_walk_more_than_one_list() {
    assert_program_eval_to(
        "(import (scheme list))
         (list (zip '(1 2 3) '(4 5 6))
               (fold + 0 '(1 2) '(3 4))
               (every < '(1 2) '(3 4))
               (any (lambda (a b) (if (< a b) 'yes #f)) '(1 2 3) '(0 1 4))
               (filter-map (lambda (x y) (and (number? x) (* x y))) '(a 1 b 3) '(9 9 9 9))
               (list-index = '(1 2 3) '(9 2 9)))",
        "(((1 4) (2 5) (3 6)) 10 #t yes (9 27) 1)",
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
/// agree; Gauche rejects the definition, as Patina used to. Fixed 2026-08-25.
///
/// Both halves matter and pull in opposite directions. `swap-first-two` is
/// written inside the binding, so its `...` is that variable and not an
/// ellipsis. `first-of` is *generated* by `def-first`, which is defined
/// outside and escapes an ellipsis into it with `(... ...)`; that one is an
/// ellipsis, even though the macro it lands in is compiled inside the
/// binding. Deciding once per macro (#111) got the first and lost the
/// second. The rule now asks per token, and reads the token's own scopes
/// when it has an identity of its own — which is what an escaped `(... ...)`
/// now carries — and the macro's definition scopes otherwise.
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
    assert_program_eval_to(program, "((2 1 3) (1))");
}

// ---------------------------------------------------------------------------
// Family 15 — a template's reference to a definition-site local that spells
//             a keyword is rejected as syntax
// ---------------------------------------------------------------------------

/// The macro is defined *inside* the binding, so its template's free `...`
/// (or `if`) is that local variable — the definition scopes say so — and a
/// template may refer to it. Reporting it as a keyword was the second of the
/// pair gating Larceny's `base` suite. Fixed 2026-08-25.
///
/// What made it hard is that the opposite case looks identical by spelling:
/// an *outer* macro's `if` must stay the special form even where the use
/// site binds `if`, which is `test_special_form_not_captured` in the hygiene
/// suite. Both now fall out of one resolution — a local variable is an
/// ordinary binding, and each reference resolves in the scopes it stands in
/// — where before a spelling set vetoed the check for one and could not see
/// the other. The third element pins that the outer `my-if` still expands.
#[test]
fn a_template_may_refer_to_a_definition_site_local_spelled_like_a_keyword() {
    let program = "(define-syntax my-if (syntax-rules () ((_ c a b) (if c a b))))
                   (let ((... 'dots) (if 'nineteen))
                     (define-syntax mention-dots (syntax-rules () ((_ a) (list a ...))))
                     (define-syntax mention-if (syntax-rules () ((_ a) (list a if))))
                     (list (mention-dots 1) (mention-if 2) (my-if #t 'outer-if-is-syntax 'no)))";
    assert_program_eval_to(program, "((1 dots) (2 nineteen) outer-if-is-syntax)");
}

/// A declared SRFI 46 ellipsis is a *declaration*, so a binding of `...`
/// around it has no bearing on it. #114 looked the binding up for the
/// spelling `...` and broke this.
#[test]
fn a_declared_ellipsis_is_unaffected_by_a_binding_of_dots() {
    assert_program_eval_to(
        "(let ((... 'dots))
           (define-syntax m3 (syntax-rules ::: () ((_ a b :::) (list b ::: a))))
           (m3 1 2 3))",
        "(2 3 1)",
    );
}

/// A macro defined at top level generates one used inside `(let ((if …)) …)`.
/// The generated template's `if` came from the generator, where `if` is the
/// special form, and must stay so. #114 captured it.
#[test]
fn a_generated_macro_keeps_its_keywords_inside_a_binding_of_that_name() {
    assert_program_eval_to(
        "(define-syntax def-mid
           (syntax-rules ()
             ((_ name) (define-syntax name (syntax-rules () ((_ c) (if c 'yes 'no)))))))
         (let ((if 'shadowed)) (def-mid mid5) (mid5 #t))",
        "yes",
    );
}

/// The same, with the binding coming from a `define` shorthand parameter
/// rather than a `let`.
///
/// The shorthand is the case that has no `let` to give it a scope, and taking
/// the enclosing scopes unchanged left the set *empty* at top level. An empty
/// scope set is not a narrow scope but no scope at all — `insert_scoped`
/// routes it to a plain `define` — so the marker for the parameter became a
/// name-visible global that shadowed the special form for every reference,
/// including macro-introduced ones. The internal-definition variant below it
/// is the same fault reached through `body_definition_names`.
#[test]
fn a_generated_macro_keeps_its_keywords_under_a_shorthand_parameter() {
    assert_program_eval_to(
        "(define-syntax def-mid
           (syntax-rules ()
             ((_ n) (define-syntax n (syntax-rules () ((_ c) (if c 'yes 'no)))))))
         (def-mid mid)
         (define (fx if) (mid #t))
         (define (fv) (define if 1) (mid #t))
         (list (fx 'shadowed) (fv))",
        "(yes yes)",
    );
}

/// A parameter named `if` in the `define` shorthand, and a template that
/// introduces its own `(let ((if 1)) …)` around a user macro's `if`. Both are
/// the direction hygiene fixes: a binder at the use site does not capture a
/// template's keyword. #114 broke both.
#[test]
fn a_use_site_binder_does_not_capture_a_templates_keyword() {
    assert_program_eval_to(
        "(define-syntax my-if6 (syntax-rules () ((_ c a b) (if c a b))))
         (define (f6 if) (my-if6 #t 'ok 'no))
         (define-syntax user7 (syntax-rules () ((_ e) (if #t e 'b))))
         (define-syntax wrap7 (syntax-rules () ((_ e) (let ((if 1)) (user7 e)))))
         (list (f6 1) (wrap7 'a))",
        "(ok a)",
    );
}

/// An inner `let-syntax` keyword outranks an enclosing variable of the same
/// spelling. The spelling veto this replaced had no ordering, so the outer
/// variable won wherever one existed.
#[test]
fn an_inner_keyword_outranks_an_enclosing_variable_of_the_same_name() {
    assert_program_eval_to(
        "(list (let-syntax ((f (syntax-rules () ((f x) x)))) (f 1))
               (let ((f (lambda (x) (+ x 1))))
                 (let-syntax ((f (syntax-rules () ((f x) x)))) (f 1))))",
        "(1 1)",
    );
}

/// R7RS 4.3.1 again, for the definitions a macro produces indirectly.
/// `define-values` and `define-record-type` both expand to a `begin` of
/// definitions, so testing only the top level of the desugared body saw no
/// definition and let the names escape into the enclosing body.
#[test]
fn a_let_syntax_body_keeps_definitions_a_macro_wrapped_in_begin() {
    assert_program_eval_to(
        "(define aa 'outer)
         (let-syntax ((noop (syntax-rules () ((_ x) x))))
           (define-values (aa bb) (values 1 2))
           (noop aa))
         aa",
        "outer",
    );
}

/// R7RS 5.3.2: a syntax definition inside a body is local to that body. It
/// used to install itself in the enclosing environment, and then — once a body
/// with bindings got an environment of its own — only when the body's lambda
/// happened to bind nothing, which made the leak depend on the formals list.
#[test]
fn an_internal_define_syntax_is_local_to_its_body() {
    assert_program_eval_to(
        "(define (g y) (define-syntax m2 (syntax-rules () ((_ v) (list 'withargs v)))) (m2 y))
         (define (f) (define-syntax m (syntax-rules () ((_ v) (list 'noargs v)))) (m 1))
         (list (g 1) (f))",
        "((withargs 1) (noargs 1))",
    );
    assert_program_eval_error(
        "(define (g y) (define-syntax m2 (syntax-rules () ((_ v) v))) (m2 y))
         (g 1)
         (m2 3)",
    );
}

/// A `let-syntax` transformer's free identifier denotes the binding that
/// encloses the *form*, even when the program also defines that name at top
/// level. Larceny's `base` is what found this: the suite happens to define
/// its own `f`, so the same two assertions family 23 covers still failed
/// there after they passed in isolation — `(g 1)` answered `"1"`, the
/// suite's `number->string` wrapper, rather than 2.
///
/// The cause is one IR below hygiene. A template's free identifiers are
/// linked back to the macro's definition environment by *name*, for the sake
/// of a template that calls a helper private to the library defining it —
/// and the name-only view of an environment deliberately hides local
/// variables, so it could not tell this `f` from a global one and aliased it
/// to the global. The link is asked with the macro's definition scopes now,
/// and skips any name something lexical shadows.
#[test]
fn a_transformer_free_reference_prefers_the_enclosing_binding_over_a_global() {
    assert_program_eval_to(
        "(define (f n) (number->string n))
         (let ((f (lambda (x) (+ x 1))))
           (let-syntax ((f (syntax-rules () ((f x) x)))
                        (g (syntax-rules () ((g x) (f x)))))
             (list (f 1) (g 1))))",
        "(1 2)",
    );
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

/// R7RS 4.3.1: a `let-syntax` body is a body — its definitions are local to
/// it, not spliced into the enclosing one — and the macro names it binds are
/// visible in the transformers only under `letrec-syntax`. Fixed 2026-08-25.
///
/// Three separate mistakes, and the middle one is the interesting one. `defs`
/// binds `x` through a *macro*, so reading the body's source forms saw no
/// definition and let it escape. `scope`'s `(f 1)` went to the outer variable
/// because a keyword bound unscoped could never outrank one; and its `(g 1)`
/// reached a sibling keyword that, under `let-syntax`, is not in scope in the
/// transformers at all. `rec-scope` is the same body under `letrec-syntax`,
/// where the sibling *is* in scope, and pins that the two forms still differ.
#[test]
fn let_syntax_body_definitions_and_transformer_scope() {
    assert_program_eval_to(
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
         (list (defs) (scope) (rec-scope))",
        "((13 70) (1 2) (1 1))",
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

// ---------------------------------------------------------------------------
// Family 27 — `error` refused a message that is not a string
// ---------------------------------------------------------------------------

/// R7RS 6.11 says the message *should* be a string — advice, not a
/// requirement — and the R6RS habit of `(error 'who "what")` runs through
/// SRFI reference implementations, the bundled SRFI 41 among them: all 53 of
/// its diagnostics were being replaced by a complaint about the argument
/// (found by review of the SRFI 41 bundle, 2026-08-25). chibi and Gauche
/// report the non-string as the message; so do we now, on both backends and
/// in both the primitive and each backend's `error` intercept.
#[test]
fn error_accepts_a_message_that_is_not_a_string() {
    assert_program_eval_to(
        "(import (scheme stream))
         (list (guard (e (#t (error-object-message e))) (error 'foo \"bar\" 1))
               (guard (e (#t (error-object-irritants e))) (error 'foo \"bar\" 1))
               (guard (e (#t (error-object-message e))) (stream-car 5))
               (guard (e (#t (error-object-message e))) (error \"plain\" 2))
               (guard (e (#t (error-object-irritants e))) (error \"plain\" 2)))",
        "(\"foo\" (\"bar\" 1) \"stream-car\" \"plain\" (2))",
    );
}

// ---------------------------------------------------------------------------
// Family 28 — an exception handler runs after the unwind, not in the raise's
//             dynamic extent
// ---------------------------------------------------------------------------

/// R7RS 6.11: a handler is called "in the dynamic environment of the call to
/// `raise`, except that the current exception handler is the outer one".
/// Both backends unwind to the handler's own wind depth *first* on the
/// `raise`/`raise-continuable` path, so the handler runs outside the extent —
/// and the after-thunk runs twice.
///
/// Found 2026-08-25 while attempting family 22, whose visible symptom this
/// produces on the non-continuable path: a `guard` cannot re-enter an extent
/// that was left before its handler ran. Measured, not deduced — R7RS 7.3's
/// reference `guard`, which explicitly jumps a continuation back to the raise
/// point, still gives family 22's wrong answer on the VM, because that
/// continuation is captured after the unwind. Neither half fixes it alone;
/// {TRIAGE} family 22 records the order.
///
/// Two neighbours, deliberately not re-pinned here: the tree-walker's `error`
/// path runs its handler *before* the unwind — the opposite ordering, and
/// already pinned in `backend_divergence.rs` — and the wind machinery the fix
/// will lean on has a VM defect of its own, also pinned there
/// (`continuation_within_its_own_wind_reruns_the_thunks_on_the_vm`). Plain
/// re-entry is covered by `compliance/control.rs`'s
/// `test_dynamic_wind_with_callcc_reentry` (R7RS §6.10's own example) rather
/// than re-derived here.
///
/// **When the log converges on `(in handler out)` — chibi's and Gauche's —
/// replace the assertion with `assert_program_eval_to` and update {TRIAGE}
/// families 22 and 28 together with PRD §6's entry.**
#[test]
fn an_exception_handler_runs_in_the_raises_dynamic_extent() {
    let program = "(define v '())
                   (define (log x) (set! v (cons x v)))
                   (define answer
                     (with-exception-handler
                       (lambda (e) (log 'handler) 'handled)
                       (lambda ()
                         (dynamic-wind (lambda () (log 'in))
                                       (lambda () (raise-continuable 'x))
                                       (lambda () (log 'out))))))
                   (list answer (reverse v))";
    assert_eq!(
        eval_program(program),
        "(handled (in out handler out))",
        "expected the pinned wrong answer; if this is now (handled (in handler out)) \
         the defect is fixed — see {TRIAGE} families 22 and 28"
    );
}

// ---------------------------------------------------------------------------
// Family 33 — a template's `quote` resolved at the use site
// ---------------------------------------------------------------------------

/// `m`'s template writes `(quote d)`; a `let-syntax` binding `quote` around
/// the *call* has nothing to do with it. Patina used to bind a `let-syntax`
/// keyword unscoped as well as scoped, which made it reachable from every
/// reference of that spelling — including one another macro introduced —
/// and since the captured expansion introduces `quote` again, the capture
/// repeated until the stack went. chibi answers `hello`.
///
/// Fixed 2026-08-26: the keyword is bound only under the body's scopes, plus
/// the binder's own scopes when the `let-syntax` itself came out of a
/// template (chibi's §4.3 `(m k)`, where binder and reference are
/// `bound-identifier=?`).
#[test]
fn a_templates_quote_is_not_captured_by_a_use_site_let_syntax() {
    assert_program_eval_to(
        "(define-syntax m (syntax-rules () ((m d) (quote d))))
         (let-syntax ((quote (syntax-rules () ((_ x) 'captured))))
           (m hello))",
        "hello",
    );
}

/// The import half, and the literal form. SRFI 101 exports its own `quote`,
/// which builds random-access lists. Its own template `(get-cached 'datum)`
/// must reach the `quote` SRFI 101 was written against — it used to reach
/// the exported one and expand without end — and a *literal* `'(1 2)` in
/// another library's template must be a pair, not whatever the program's
/// `quote` builds. Both are the definition site's `quote`; only the user's
/// own `'(1 2)` is the program's.
///
/// Fixed 2026-08-26: the relinker rewrites a `quote` head it used to skip,
/// and the template compiler compiles the `quote` of a literal datum as a
/// reference rather than emitting it verbatim with the datum.
#[test]
fn a_templates_quote_is_the_definition_sites_quote_under_an_imported_one() {
    assert_program_eval_to(
        "(define-library (probe lit)
           (import (scheme base))
           (export lit)
           (begin (define-syntax lit (syntax-rules () ((_) '(1 2))))))
         (import (except (scheme base) quote car cons list list?)
                 (prefix (scheme base) r7:)
                 (srfi 101)
                 (probe lit))
         (r7:list (let ((f (lambda () '(x)))) (eq? (f) (f)))
                  (car '(1 2))
                  (r7:pair? (lit)))",
        "(#t 1 #t)",
    );
}

// ---------------------------------------------------------------------------
// Family 34 — VM: quasiquote built its result with the use site's `list`
// ---------------------------------------------------------------------------

/// A quasiquote denotes the structure it writes, whatever `list`, `append`
/// and `list->vector` mean where it appears. The VM's expansion called them
/// by name, so under SRFI 101 — whose `list` builds random-access lists —
/// `` `(1 ,x 3) `` was one too, and `` `#(1 ,x) `` failed inside
/// `list->vector`. The tree-walker builds the structure directly and was
/// right all along. The last element pins that the rebinding is real.
///
/// Fixed 2026-08-26: the expansion references the registry's primitives as
/// values, so nothing the program imports or defines can redirect them.
#[test]
fn quasiquote_builds_pairs_whatever_list_means_at_the_use_site() {
    assert_program_eval_to(
        "(import (except (scheme base) quote car cons list list? append)
                 (prefix (scheme base) r7:)
                 (srfi 101))
         (define x 2)
         (r7:list (r7:pair? `(1 ,x 3))
                  (r7:equal? `(1 ,@(r7:list 7 8) 3) (r7:list 1 7 8 3))
                  (r7:vector? `#(1 ,x))
                  (r7:pair? (list 1 2)))",
        "(#t #t #t #f)",
    );
}

// ---------------------------------------------------------------------------
// Family 35 — relinking rewrote the user's code inside a macro call
// ---------------------------------------------------------------------------

/// `both`'s template uses `(scheme base)`'s `list`; the program's `list` is
/// SRFI 101's. The template's reference is relinked to the definition
/// site's `list` — that is referential transparency — but the `(list 1 2)`
/// the user wrote *as the argument* means the program's, and it used to be
/// rewritten too, because the relinker matched by spelling. So `v` is a
/// pair and its second element is not.
///
/// Fixed 2026-08-26: the relinker renames only identifiers carrying the
/// expansion's own scope, which the expander puts on what a template
/// introduces and on nothing that came in through a pattern variable.
#[test]
fn relinking_leaves_the_users_code_inside_a_macro_call_alone() {
    assert_program_eval_to(
        "(define-library (probe both)
           (import (scheme base))
           (export both)
           (begin (define-syntax both (syntax-rules () ((_ e) (list 'template e))))))
         (import (except (scheme base) quote car cons list list?)
                 (prefix (scheme base) r7:)
                 (srfi 101)
                 (probe both))
         (define v (both (list 1 2)))
         (r7:list (r7:pair? v) (r7:pair? (r7:cadr v)))",
        "(#t #f)",
    );
}

/// The review of the first fix found what the new binding rule had left
/// out, all fixed 2026-08-26 and pinned here: a `let-syntax` puts its scope
/// on its body *as written* and — for `letrec-syntax` — on its transformers,
/// which is what lets an introduced binder be bound at its own scopes plus
/// that one; and a user's symbol that reached a transformer through a
/// pattern variable stands in the transformer's definition context.
///
/// `gen6`'s keyword is referenced from inside its own transformer, which
/// the unscoped binding used to satisfy by name; `gen7`'s from a transformer
/// in the body. Both unbound after the first fix; `done6` and `v` in chibi.
#[test]
fn a_generated_keyword_is_reachable_from_a_transformer_in_its_scope() {
    assert_program_eval_to(
        "(define-syntax gen6
           (syntax-rules ()
             ((_ name) (letrec-syntax ((name (syntax-rules () ((_) 'done6) ((_ x . r) (name . r)))))
                         (name 1)))))
         (define-syntax gen7
           (syntax-rules ()
             ((_ name) (let-syntax ((name (syntax-rules () ((_) 'v))))
                         (let-syntax ((g (syntax-rules () ((_) (name))))) (g))))))
         (list (gen6 foo) (gen7 bar))",
        "(done6 v)",
    );
}

/// Under `let-syntax` a transformer does not see its siblings, and that
/// holds when the whole form came out of a template: `a`'s `(b)` is the
/// outer `b`, whether that is bound by `define-syntax` or by an enclosing
/// `let-syntax`. The first fix bound the generated keyword at the
/// template's scopes alone, which every reference from that expansion
/// carries — the sibling's transformer included.
#[test]
fn a_template_generated_let_syntax_keeps_siblings_out_of_its_transformers() {
    assert_program_eval_to(
        "(define-syntax b (syntax-rules () ((_) 'outer-b)))
         (define-syntax m
           (syntax-rules ()
             ((_) (let-syntax ((a (syntax-rules () ((_) (b))))
                               (b (syntax-rules () ((_) 'sibling-b))))
                    (a)))))
         (list (m)
               (let-syntax ((b2 (syntax-rules () ((_) 'outer-b2))))
                 (let-syntax ((m2 (syntax-rules ()
                                    ((_) (let-syntax ((a (syntax-rules () ((_) (b2))))
                                                      (b2 (syntax-rules () ((_) 'sibling-b2))))
                                           (a))))))
                   (m2))))",
        "(outer-b outer-b2)",
    );
}

/// A user's `(k)` passed into a template that wraps it in a `let-syntax`
/// binding `k` means the user's `k` — the generated keyword's binding
/// carries the template's scope, which the user's reference never does.
/// Pre-existing: the keyword used to be bound at the body's scopes alone,
/// which every symbol in the body resolves with.
#[test]
fn a_users_symbol_is_not_captured_by_a_template_generated_keyword() {
    assert_program_eval_to(
        "(define (k) 'users-k)
         (define-syntax gen
           (syntax-rules ()
             ((_ body) (let-syntax ((k (syntax-rules () ((_) 'captured)))) body))))
         (gen (k))",
        "users-k",
    );
}

/// Inside a quasiquote, `(quote b)` is two symbols of data and an `unquote`
/// within it is still evaluated. The first fix rewrote such a head to the
/// relinked `quote` at any depth (a `quote.N` symbol in the data), and the
/// template compiler had always inserted the quoted datum verbatim, so its
/// `,(helper 7)` — a library-private reference — was never relinked.
#[test]
fn a_quote_inside_a_quasiquote_template_is_data_with_its_unquotes_evaluated() {
    assert_program_eval_to(
        "(define-library (probe qq)
           (import (scheme base))
           (export qq qq2 qq3)
           (begin
             (define (helper x) (* 6 x))
             (define-syntax qq (syntax-rules () ((_ e) `(a (quote b) ,e))))
             (define-syntax qq2 (syntax-rules () ((_ e) `(a '(b ,(helper 7)) ,e))))
             (define-syntax qq3 (syntax-rules () ((_ e) `#(a 'b ,e))))))
         (import (except (scheme base) quote car cons list list?)
                 (prefix (scheme base) r7:)
                 (srfi 101)
                 (probe qq))
         (r7:list (qq 1) (qq2 1) (qq3 1))",
        "((a 'b 1) (a '(b 42) 1) #(a 'b 1))",
    );
}

/// A `quote` that reached a template through a pattern variable and was
/// then relinked is still `quote`: the relinker classifies a head by what
/// it resolves to, not its spelling, so the datum after a `quote.N` head is
/// left alone. Pre-existing — it walked the datum and renamed its `list`.
#[test]
fn a_relinked_quote_head_still_protects_its_datum() {
    assert_program_eval_to(
        "(define-library (probe wq)
           (import (scheme base))
           (export outer)
           (begin
             (define-syntax with-q (syntax-rules () ((_ q) (q (list 1)))))
             (define-syntax outer (syntax-rules () ((_) (with-q quote))))))
         (import (except (scheme base) quote car cons list list?)
                 (prefix (scheme base) r7:)
                 (srfi 101)
                 (probe wq))
         (outer)",
        "(list 1)",
    );
}

/// Symbols under an ellipsis escape are references like any other: a
/// library-private `helper` inside `(... (helper 1))` resolves where the
/// macro was written, and a generated macro's `(tag x ...)` reaches the
/// library's `tag`. The escape compiler used to emit every non-pattern
/// symbol as a bare literal, so nothing under `(... …)` was hygienic; the
/// first occurrence is what the by-spelling relinker had covered by
/// accident, the second never worked.
#[test]
fn references_under_an_ellipsis_escape_resolve_at_the_definition_site() {
    assert_program_eval_to(
        "(define-library (probe esc)
           (import (scheme base))
           (export mk def-tagger)
           (begin
             (define (helper x) (* 5 x))
             (define (tag . xs) (cons 'tagged xs))
             (define-syntax mk
               (syntax-rules ()
                 ((_) (begin (helper 0)
                             (define-syntax g (syntax-rules () ((_) (... (helper 1)))))))))
             (define-syntax def-tagger
               (syntax-rules ()
                 ((_ name) (define-syntax name (... (syntax-rules () ((_ x ...) (tag x ...))))))))))
         (import (scheme base) (probe esc))
         (mk)
         (def-tagger t)
         (list (g) (t 1 2))",
        "(5 (tagged 1 2))",
    );
}

/// A vector object embedded in code — `(eval (list 'outer vec) env)` — is
/// the object the expansion mutates, not a copy. The scope flip walks
/// vectors now (so a quasiquoted `#(,(helper x))` can be relinked) and its
/// first version copied every vector it walked; it copies only one whose
/// elements changed.
#[test]
fn a_vector_object_in_evaluated_code_keeps_its_identity_through_expansion() {
    assert_program_eval_to(
        "(import (scheme base) (scheme eval) (scheme repl))
         (define-syntax inner (syntax-rules () ((_ x) (vector-set! x 0 'changed))))
         (define-syntax outer (syntax-rules () ((_ x) (inner x))))
         (define vec (vector 1 2))
         (eval (list 'outer vec) (interaction-environment))
         vec",
        "#(changed 2)",
    );
}

// ---------------------------------------------------------------------------
// Family 36 — tree-walker: a local variable captures a template's reference
// ---------------------------------------------------------------------------

/// `mk`'s template references `list`; a use-site `(let ((list 1)) …)` has no
/// bearing on it, and the VM agrees. The tree-walker binds and looks up
/// locals by name (`application.rs` / `step.rs`), so the template's `list`
/// finds the variable. Surfaced by the review of #132 while auditing the
/// relinker's contract, which skips a name when the two environments'
/// global views agree and leaves the rest to scope-aware resolution — which
/// the tree-walker does not do for locals.
#[test]
fn a_use_site_local_does_not_capture_a_templates_reference() {
    assert_divergence(
        "(define-syntax mk (syntax-rules () ((_ a b) (list a b))))
         (let ((list 1)) (mk 1 2))",
        On::Vm,
        "(1 2)",
        ErrorClass::AtRuntime,
        "scheme_tests/reports/larceny_triage.md, family 36",
    );
}

// ---------------------------------------------------------------------------
// Family 37 — a macro-introduced variable binding does not rename its scope
// ---------------------------------------------------------------------------

/// R7RS §4.3.2: "if a macro transformer inserts a binding for an identifier
/// (variable or keyword), the identifier will in effect be renamed
/// throughout its scope". Patina does that for a `let-syntax` keyword —
/// family 33's second round put the form's scope on its body as written —
/// but `lambda` and `let` still bind through `with_shadowed_names`, at
/// `current_scopes + fresh`, which a reference the same template introduced
/// never carries. So the template's own `(p 1)` resolves to the outer
/// keyword instead of the parameter it just bound.
///
/// Asserted as-is: both backends agree, and chibi answers
/// `(inner-keyword 101 201)`. **When this converges, that is the fix
/// landing** — replace the expectation with chibi's and delete this note.
/// Recorded 2026-08-26 by the cleanup review of #132, which found the
/// `let-syntax` fix had been placed at the call site rather than in a shared
/// "enter a binding form" step; see the triage doc for the shape that would.
#[test]
fn a_macro_introduced_variable_binding_renames_its_scope() {
    assert_program_eval_to(
        "(define-syntax p (syntax-rules () ((_ x) 'outer-macro)))
         (define-syntax genls
           (syntax-rules ()
             ((_) (let-syntax ((p (syntax-rules () ((_ y) 'inner-keyword))))
                    (p 1)))))
         (define-syntax genlam
           (syntax-rules () ((_) ((lambda (p) (p 1)) (lambda (y) (+ y 100))))))
         (define-syntax genlet
           (syntax-rules () ((_) (let ((p (lambda (y) (+ y 200)))) (p 1)))))
         (list (genls) (genlam) (genlet))",
        "(inner-keyword 101 201)",
    );
}

// ---------------------------------------------------------------------------
// Family 38 — a scoped write cannot reach the binding its own read can
// ---------------------------------------------------------------------------

/// `Environment::get_with_scopes` resolves by subset — the largest binding
/// scope set contained in the reference's. `set_with_scopes` demanded an
/// *exact* match, so a reference could read a binding it could not write and
/// the write fell through to the root's by-name `set`. The `set!` here is
/// introduced by the *inner* macro, so it carries that macro's definition
/// scopes on top of the binder's — a strict superset, ordinary under
/// set-of-scopes and fatal under exact matching.
///
/// Fixed 2026-08-27, and only after two other things were: writes resolve
/// the way reads do, which was unsafe while a source-written parameter lived
/// in *two* cells — a by-name one and a scoped one — because the write
/// reached the scoped twin and left the by-name read stale. Binding such a
/// parameter once, with `define_scoped_definition`, removes the twin; that
/// function's own doc had named this freeze as the reason it exists.
#[test]
fn an_introduced_macro_can_assign_to_an_introduced_binding() {
    assert_program_eval_to(
        "(define-syntax gen
           (syntax-rules ()
             ((_ mac)
              (let ((v 1))
                (define-syntax mac (syntax-rules () ((_) (set! v (+ v 10)))))
                (mac)
                v))))
         (gen bump)",
        "11",
    );
}

/// The same defect through a *source-written* binder, which is the shape
/// that made the naive fix unsafe: with two cells, `(f 1)` answered `1`
/// instead of `101` with no error at all.
#[test]
fn an_introduced_macro_can_assign_to_a_source_written_binder() {
    assert_program_eval_to(
        "(define (f x)
           (define-syntax bump (syntax-rules () ((_) (set! x (+ x 100)))))
           (bump)
           x)
         (f 1)",
        "101",
    );
}

// ---------------------------------------------------------------------------
// Family 39 — a binder is scoped by where it stands, not by one fresh scope
// ---------------------------------------------------------------------------

/// `m1` is defined inside the outer `let`, so its template's `x` means that
/// binding wherever the macro is used — R7RS §4.3.2 referential
/// transparency. `m2`, defined one `let` deeper, means the middle one.
///
/// The answer was right before this fix and produced for the wrong reason:
/// each `let` bound its variable at a *single* fresh scope, so `{outer}` and
/// `{middle}` were unordered, neither was more specific, and set-of-scopes
/// resolution could not choose — Flatt's rule calls such a reference
/// ambiguous and Racket raises an error. The winner came from the candidate
/// walk visiting inner environments first, which is lexical nesting and
/// outside the model.
///
/// Fixed 2026-08-27: a parameter written in source is bound at the scopes it
/// *stands in* — every scope enclosing the form, plus the one minted for it.
/// Nested binders then form a chain, each strictly containing the last, and a
/// chain is always decidable. `PATINA_AMBIGUITY_STRICT=1` accepts this
/// program on both backends now, and the whole test suite with it.
#[test]
fn a_binder_is_scoped_by_where_it_stands() {
    assert_program_eval_to(
        "(let ((x 'outer))
           (let-syntax ((m1 (syntax-rules () ((m1) x))))
             (let ((x 'middle))
               (let-syntax ((m2 (syntax-rules () ((m2) x))))
                 (let ((x 'inner))
                   (list (m1) (m2)))))))",
        "(outer middle)",
    );
}
