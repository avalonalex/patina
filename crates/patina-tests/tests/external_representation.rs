//! What `display` and `write` print for values R7RS gives no external
//! representation: error objects, procedures, continuations, ports.
//!
//! The standard lets an implementation print these however it likes, so none
//! of the spellings here is required — but *something informative* is, and
//! three of them were `#<unknown>` until issue #181. The writer reached them
//! through a chain of type probes, and a heap variant nobody had added a probe
//! for fell off the end of it: an error object, a continuation, and — on the
//! VM, where a `lambda` compiles to a `VmClosure` rather than the
//! tree-walker's `Procedure` — every user-defined procedure. The same miss had
//! already been fixed twice by adding one more probe (procedures in record
//! fields, then environment specifiers), so the writer now dispatches on the
//! heap variant instead, and an unhandled one is a compile error.
//!
//! Every test runs on both backends. That is the point of several of them:
//! the two allocate different heap objects for the same Scheme value, so a
//! spelling that agrees is a property of the language and not of the backend
//! a program happened to run on.
//!
//! An error object is the one value here with parts, and the tests say where
//! its irritants are rendered as much as how: by the writer that assigns
//! datum labels, so that a circular irritant terminates. `debug_format`, the
//! REPL's printer, stops at the message for the opposite reason — it has no
//! cycle detection at all.

mod common;
use common::{
    assert_program_eval_to, eval_program, eval_program_tree_walker, eval_program_vm,
    try_eval_program_tree_walker, try_eval_program_vm,
};

// ─── Error objects (issue #181) ──────────────────────────────────────────────

/// The repro from the issue. `display` said `#<unknown>` on both backends.
#[test]
fn test_an_error_object_displays_its_message_and_irritants() {
    assert_program_eval_to(
        r#"(guard (e (#t e)) (error "boom" 1 2))"#,
        "#<error-object: boom 1 2>",
    );
}

/// No irritants, nothing trailing the message.
#[test]
fn test_an_error_object_with_no_irritants_shows_only_its_message() {
    assert_program_eval_to(
        r#"(guard (e (#t e)) (error "boom"))"#,
        "#<error-object: boom>",
    );
}

/// The irritants are nested values, so they follow the ambient mode the way a
/// list's elements do: `write` distinguishes a string from a symbol from a
/// character, `display` does not. Only the irritants change — the message is
/// prose about the error, not a datum, and stays unquoted in both.
#[test]
fn test_the_irritants_follow_the_ambient_write_or_display_mode() {
    let capture = |writer: &str| {
        format!(
            r#"(import (scheme write))
               (let ((p (open-output-string)))
                 ({writer} (guard (e (#t e)) (error "bad key:" "foo" 'bar #\c)) p)
                 (get-output-string p))"#
        )
    };
    // The harness `write`s the captured string back, so the expectations carry
    // that second round of escaping.
    assert_program_eval_to(
        &capture("display"),
        r##""#<error-object: bad key: foo bar c>""##,
    );
    assert_program_eval_to(
        &capture("write"),
        r##""#<error-object: bad key: \"foo\" bar #\\c>""##,
    );
}

/// An error the runtime raised itself, not one `error` built, prints the same
/// way — it is the same heap object.
///
/// The `car` diagnostic's exact wording is deliberately not pinned: it is not
/// this file's property, and pinning it would turn any rewording of that
/// message into a failure here — reported, confusingly, as "backends
/// disagree" rather than as this assertion.
#[test]
fn test_a_runtime_error_object_displays_its_message() {
    let printed = eval_program("(guard (e (#t e)) (car '()))");
    assert!(
        printed.starts_with("#<error-object: ") && printed.ends_with('>'),
        "a runtime-raised error printed as {printed}"
    );
    assert!(
        printed.contains("pair"),
        "the message did not survive into the printed form: {printed}"
    );
    assert_program_eval_to("(guard (e (#t (error-object? e))) (car '()))", "#t");
}

/// Printing the message is not a substitute for the accessors, and must not
/// disturb them: the irritants are still there, still separate, still in
/// order. R7RS §6.11 makes these the supported way to read an error object.
#[test]
fn test_the_accessors_still_see_message_and_irritants() {
    assert_program_eval_to(
        r#"(list (guard (e (#t (error-object-message e))) (error "boom" 1 2))
                 (guard (e (#t (error-object-irritants e))) (error "boom" 1 2))
                 (guard (e (#t (error-object? e))) (error "boom" 1 2)))"#,
        r#"("boom" (1 2) #t)"#,
    );
}

/// A circular irritant gets a datum label, like a circular value anywhere
/// else. This is what makes printing the irritants safe at all: the error
/// object is rendered by the writer that already assigns labels, not by the
/// leaf formatter, which cannot recurse. Without that, `(error "cycle" xs)`
/// would be one call away from a non-terminating `display`.
#[test]
fn test_a_circular_irritant_gets_a_datum_label() {
    assert_program_eval_to(
        r#"(define xs (list 1 2))
           (set-cdr! (cdr xs) xs)
           (guard (e (#t e)) (error "cycle" xs))"#,
        "#<error-object: cycle #0=(1 2 . #0#)>",
    );
}

/// The error object nests both ways: inside a list, and around a compound
/// irritant. Neither is a special case in the writer — it recurses.
#[test]
fn test_an_error_object_nests_in_both_directions() {
    assert_program_eval_to(
        r#"(list (guard (e (#t e)) (error "nested" (list 1 (vector 2 3)))))"#,
        "(#<error-object: nested (1 #(2 3))>)",
    );
}

/// `write-shared` labels a shared error object, because the writer now
/// *descends* into one. Its contract (R7RS §6.13.3) is to label all shared
/// structure, and a value the writer descends into but refuses to label is
/// re-emitted in full at every occurrence — here `e1` four times, and
/// 2^N times at N levels.
#[test]
fn test_write_shared_labels_a_shared_error_object() {
    assert_program_eval_to(
        r#"(import (scheme write))
           (define e1 (guard (c (#t c)) (error "a" 1)))
           (define e2 (guard (c (#t c)) (error "b" e1 e1)))
           (define e3 (guard (c (#t c)) (error "c" e2 e2)))
           (let ((p (open-output-string))) (write-shared e3 p) (get-output-string p))"#,
        r##""#<error-object: c #1=#<error-object: b #0=#<error-object: a 1> #0#> #1#>""##,
    );
}

/// Plain `write` labels only what is *circular*, so the same shared structure
/// is written out twice — the treatment a shared pair gets. The contrast with
/// the test above is the whole difference between the two procedures.
#[test]
fn test_plain_write_does_not_label_merely_shared_error_objects() {
    assert_program_eval_to(
        r#"(import (scheme write))
           (define e1 (guard (c (#t c)) (error "a" 1)))
           (define e2 (guard (c (#t c)) (error "b" e1 e1)))
           (let ((p (open-output-string))) (write e2 p) (get-output-string p))"#,
        r##""#<error-object: b #<error-object: a 1> #<error-object: a 1>>""##,
    );
}

/// `write-simple` renders the irritants with no labels at all. R7RS §6.13.3
/// lets it diverge on a cycle, which is why the circular case is not tested
/// here — but the ordinary case still has to come out right.
#[test]
fn test_write_simple_renders_the_irritants_without_labels() {
    assert_program_eval_to(
        r#"(import (scheme write))
           (let ((p (open-output-string)))
             (write-simple (guard (e (#t e)) (error "boom" (list 1 2) "x")) p)
             (get-output-string p))"#,
        r##""#<error-object: boom (1 2) \"x\">""##,
    );
}

// ─── The uncaught-error diagnostic ───────────────────────────────────────────

/// Where the gap cost the most: an error nothing handles ends the program, and
/// on the VM the message that ended it was `unhandled exception: #<unknown>`.
///
/// The two backends still word this differently, and the test says so rather
/// than settling for a substring both happen to contain. The VM formats the
/// raised object with the datum writer, so it now carries the irritants; the
/// tree-walker's `error` never builds the heap object at all — it raises an
/// `EvalError` whose `Display` drops the `irritants_display` it computed. The
/// wording is not the property under test, but neither diagnostic may fall
/// back to `#<unknown>`, and that part holds on both.
#[test]
fn test_an_uncaught_error_names_its_message() {
    for (backend, result, expected) in [
        (
            "tree-walker",
            try_eval_program_tree_walker(r#"(error "boom" 1 2)"#),
            "Scheme exception (Error): boom",
        ),
        (
            "vm",
            try_eval_program_vm(r#"(error "boom" 1 2)"#),
            "unhandled exception: #<error-object: boom 1 2>",
        ),
    ] {
        let message = result.expect_err("nothing handles the error");
        assert!(
            message.contains(expected),
            "[{backend}] expected the diagnostic to contain {expected:?}, got: {message}"
        );
        assert!(
            !message.contains("#<unknown>"),
            "[{backend}] diagnostic fell back to #<unknown>: {message}"
        );
    }
}

/// Re-raising a caught error object takes *both* backends through the writer,
/// so this is the shape that pinned the gap on the tree-walker too. It is what
/// the third-party compat harness hit, where a library's own error handling
/// reported `unhandled exception: unhandled exception: #<unknown>`.
#[test]
fn test_a_re_raised_error_object_names_its_message() {
    let code = r#"(raise (guard (e (#t e)) (error "boom" 1 2)))"#;
    for (backend, result) in [
        ("tree-walker", try_eval_program_tree_walker(code)),
        ("vm", try_eval_program_vm(code)),
    ] {
        let message = result.expect_err("nothing handles the re-raise");
        assert!(
            message.contains("#<error-object: boom 1 2>"),
            "[{backend}] uncaught error object was not named: {message}"
        );
    }
}

// ─── Procedures and continuations ────────────────────────────────────────────

/// The VM printed `#<unknown>` for every `lambda` a program defined: its
/// closures are a heap variant the probe chain never learned about, while the
/// tree-walker's are the one variant it did. A primitive printed correctly on
/// both, which is why this went unnoticed.
#[test]
fn test_a_user_defined_procedure_prints_as_a_procedure() {
    assert_program_eval_to("(lambda (x) x)", "#<procedure>");
    assert_program_eval_to("(define (f x) x) f", "#<procedure>");
    assert_program_eval_to(
        "(import (scheme case-lambda)) (case-lambda ((x) x))",
        "#<procedure>",
    );
}

/// A primitive prints the same as a `lambda`. Nothing in the printed form says
/// which is which — R7RS gives a program no way to tell them apart either.
#[test]
fn test_a_primitive_prints_as_a_procedure() {
    assert_program_eval_to("car", "#<procedure>");
}

/// One spelling on both backends, though they capture into different things:
/// the tree-walker into a heap continuation, the VM into a store it reaches by
/// handle. A program cannot tell which backend it is on, so its output must
/// not either.
#[test]
fn test_a_continuation_prints_as_a_continuation() {
    assert_program_eval_to(
        "(call-with-current-continuation (lambda (k) k))",
        "#<continuation>",
    );
}

/// A *delimited* continuation is a third heap variant again — the one an
/// abort hands its prompt's handler — and prints as the same thing. Anything
/// else would let a program read the capture's flavour out of its output.
#[test]
fn test_a_delimited_continuation_prints_as_a_continuation() {
    assert_program_eval_to(
        "(define t (make-continuation-prompt-tag))
         (call-with-continuation-prompt
           (lambda () (abort-current-continuation t 1))
           t
           (lambda (v k) k))",
        "#<continuation>",
    );
}

// ─── The property behind the individual spellings ────────────────────────────

/// No value a program can hold prints as `#<unknown>`.
///
/// The individual tests above each pin one spelling; this one pins the
/// property they exist for, over every value shape this file can reach. It is
/// the check that would have caught all three misses at once, and the one
/// that keeps catching a new heap variant reaching user output — though the
/// exhaustive dispatch in the writer is what makes that unlikely enough to be
/// a backstop rather than the defence.
///
/// The count is asserted, not just the absence of `#<unknown>`: without it an
/// empty capture passes, and a capture can go empty for reasons that have
/// nothing to do with the property (a port that never received anything, a
/// `for-each` that stopped early).
///
/// Each backend is checked on its own rather than through the both-backends
/// helper, because the property is per-backend and one of these renderings
/// legitimately differs: a prompt tag prints an allocation-order id
/// (`#<prompt-tag:prompt/2>` on one, `/3` on the other). Requiring agreement
/// would exclude the tag from the sweep to protect an assertion the sweep is
/// not making.
#[test]
fn test_nothing_reachable_prints_as_unknown() {
    // One line per value, so a spelling that came out empty is a length
    // mismatch rather than a silent pass.
    let values = [
        r#"(guard (e (#t e)) (error "boom" 1 2))"#,
        "(guard (e (#t e)) (car '()))",
        "(lambda (x) x)",
        "car",
        "(case-lambda ((x) x))",
        "(call-with-current-continuation (lambda (k) k))",
        "(call-with-continuation-prompt
           (lambda () (abort-current-continuation tag 1)) tag (lambda (v k) k))",
        "tag",
        "(make-parameter 1)",
        "(delay 1)",
        "(make-point 1)",
        "<point>",
        r#"(open-input-string "x")"#,
        "(current-output-port)",
        "(environment '(scheme base))",
        r#"(string->symbol "sym")"#,
        "(bytevector 1 2)",
        "3.5",
        "1/2",
        "(* 1000000000000 1000000000000)",
    ];
    let code = format!(
        r#"(import (scheme write) (scheme lazy) (scheme case-lambda) (scheme eval))
           (define-record-type <point> (make-point x) point? (x point-x))
           (define tag (make-continuation-prompt-tag))
           (define p (open-output-string))
           (for-each
             (lambda (v) (write v p) (newline p))
             (list {}))
           (get-output-string p)"#,
        values.join("\n")
    );

    for (backend, printed) in [
        ("tree-walker", eval_program_tree_walker(&code)),
        ("vm", eval_program_vm(&code)),
    ] {
        let lines: Vec<&str> = printed.trim_matches('"').split("\\n").collect();
        // `split` leaves a trailing empty piece after the final newline.
        let lines = &lines[..lines.len() - 1];
        assert_eq!(
            lines.len(),
            values.len(),
            "[{backend}] expected one rendering per value, got: {printed}"
        );
        for (source, rendering) in values.iter().zip(lines) {
            assert!(
                !rendering.is_empty() && !rendering.contains("#<unknown>"),
                "[{backend}] {source} printed as {rendering:?}"
            );
        }
    }
}
