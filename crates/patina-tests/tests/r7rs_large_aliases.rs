//! Tests for the R7RS-large `(scheme ...)` alias libraries.
//!
//! R7RS-large names its libraries `(scheme list)`, `(scheme sort)`, and so on;
//! each is an existing SRFI under its standard-track name. The alias libraries
//! in `lib/scheme/` are pure re-exports of the corresponding `(srfi n)`, so
//! these tests check two things: that each alias loads, and that a binding
//! reached through the alias is the *same* binding as through the SRFI.

use patina_interpreter::{Interpreter, TreeWalkInterpreter};
use patina_primitives::primitives::io::datum_writer::format_write_tagged;
use patina_runtime::Backend;
use patina_tree_walker::Evaluator;
use patina_vm::VmBackend;

/// (alias library, backing SRFI) pairs, per PRD/phase2/R7RS_LARGE_STATUS.md.
const ALIASES: &[(&str, u32)] = &[
    ("list", 1),         // Red
    ("box", 111),        // Red
    ("set", 113),        // Red
    ("comparator", 128), // Red
    ("sort", 132),       // Red
    ("vector", 133),     // Red
    ("generator", 158),  // Tangerine
    ("hash-table", 125), // Red
    ("charset", 14),     // Red
    ("stream", 41),      // Red
    ("list-queue", 117), // Red
    ("lseq", 127),       // Red
    ("ilist", 116),      // Red
    ("ideque", 134),     // Red
    ("flonum", 144),     // Tangerine
    ("text", 135),       // Red
    ("ephemeron", 124),  // Red
    ("rlist", 101),      // Red — renames, see RENAMING_ALIASES
];

#[test]
fn test_all_alias_libraries_load() {
    let eval = Evaluator::new();
    for (name, srfi) in ALIASES {
        let lib_name = vec!["scheme".to_string(), name.to_string()];
        let lib = eval
            .load_library(&lib_name)
            .unwrap_or_else(|e| panic!("failed to load (scheme {name}) [SRFI {srfi}]: {e}"));
        assert_eq!(lib.name, lib_name);
    }
}

/// An alias must re-export exactly what its backing SRFI exports -- no more,
/// no fewer. A mismatch means the hand-listed export set has drifted from the
/// source library, which is the failure mode this whole approach invites.
/// The aliases that *rename* rather than re-export.
///
/// Almost every `(scheme …)` alias is a pure re-export of its SRFI, so the two
/// export the same names and the tests below compare them directly. An alias
/// whose SRFI deliberately shadows `(scheme base)` cannot do that — R7RS-large
/// renames those so both libraries can be imported together — so it needs its
/// own shape of check. Keeping the list here rather than testing the name in
/// each place means the next one is a row, not a third `if`.
const RENAMING_ALIASES: &[&str] = &["rlist"];

#[test]
fn test_alias_exports_match_backing_srfi() {
    let eval = Evaluator::new();
    for (name, srfi) in ALIASES {
        let alias = eval
            .load_library(&["scheme".to_string(), name.to_string()])
            .unwrap_or_else(|e| panic!("(scheme {name}): {e}"));
        let source = eval
            .load_library(&["srfi".to_string(), srfi.to_string()])
            .unwrap_or_else(|e| panic!("(srfi {srfi}): {e}"));

        let mut a: Vec<_> = alias.exports.keys().cloned().collect();
        let mut s: Vec<_> = source.exports.keys().cloned().collect();
        a.sort();
        s.sort();

        // `(scheme rlist)` renames rather than re-exports, and is the only one
        // that does. SRFI 101's own names shadow `(scheme base)` — it exports
        // `cons`, `car`, `list?` — so R7RS-large gives every one an `r` prefix
        // to let both libraries be imported together.
        //
        // Comparing name sets is therefore the wrong check, but comparing
        // *counts* is far too weak: the alias is a hand-written 48-entry
        // mapping, and its characteristic mistake is a wrong target — writing
        // `(rename cddar rcdddar)` keeps both counts at 48 while shipping two
        // names for one accessor and none for another. So reconstruct the
        // expected names from the rule and compare those.
        if RENAMING_ALIASES.contains(name) {
            let expected: Vec<String> = s
                .iter()
                // chibi's port exports one name R7RS-large does not have.
                .filter(|k| k.as_str() != "length<=?")
                .map(|k| match k.as_str() {
                    "make-list" => "make-rlist".to_string(),
                    "random-access-list->linear-access-list" => "rlist->list".to_string(),
                    "linear-access-list->random-access-list" => "list->rlist".to_string(),
                    other => format!("r{other}"),
                })
                .collect();
            let mut expected_sorted = expected.clone();
            expected_sorted.sort();
            let missing: Vec<_> = expected_sorted.iter().filter(|k| !a.contains(k)).collect();
            let extra: Vec<_> = a.iter().filter(|k| !expected_sorted.contains(k)).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "(scheme rlist) is not `r` + each (srfi 101) name: missing {missing:?}, \
                 extra {extra:?}"
            );
            continue;
        }

        let missing: Vec<_> = s.iter().filter(|k| !a.contains(k)).collect();
        let extra: Vec<_> = a.iter().filter(|k| !s.contains(k)).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "(scheme {name}) drifted from (srfi {srfi}): missing {missing:?}, extra {extra:?}"
        );
    }
}

/// Evaluate on both backends and return the answer they agree on.
///
/// Both, because these are library-resolution tests and the VM is the default
/// backend: a `(scheme …)` alias that failed to resolve, or resolved to
/// different bindings, only there would have gone unnoticed while
/// `R7RS_LARGE_STATUS.md` recorded the library as shipped on both. This ran on
/// the tree-walker alone until 2026-08-25.
fn eval_to_string(src: &str) -> String {
    let tw = TreeWalkInterpreter::new_tree_walker();
    let tw_value = tw
        .eval_program(src)
        .unwrap_or_else(|e| panic!("[tree-walker] eval failed for {src}: {e:?}"));
    let tw_out = format_write_tagged(tw_value, tw.backend().global_env().heap());

    let vm = Interpreter::new(VmBackend::new());
    let vm_value = vm
        .eval_program(src)
        .unwrap_or_else(|e| panic!("[vm] eval failed for {src}: {e:?}"));
    let vm_out = format_write_tagged(vm_value, vm.backend().global_env().heap());

    assert_eq!(
        tw_out, vm_out,
        "backends disagree for: {src}\n  tree-walker: {tw_out}\n  vm: {vm_out}"
    );
    vm_out
}

/// Headline forms reached through the alias name, not the SRFI name.
#[test]
fn test_alias_bindings_are_usable() {
    let cases = [
        ("(import (scheme list)) (fold + 0 '(1 2 3 4))", "10"),
        (
            "(import (scheme list)) (length (delete-duplicates '(1 2 1 3 2)))",
            "3",
        ),
        ("(import (scheme box)) (unbox (box 42))", "42"),
        (
            "(import (scheme set) (scheme comparator)) \
             (set-size (set (make-default-comparator) 1 2 3 2))",
            "3",
        ),
        (
            "(import (scheme comparator)) (comparator? (make-default-comparator))",
            "#t",
        ),
        ("(import (scheme sort)) (car (list-sort < '(3 1 2)))", "1"),
        (
            "(import (scheme vector)) (vector-count even? #(1 2 3 4))",
            "2",
        ),
        (
            "(import (scheme generator)) (length (generator->list (make-iota-generator 4)))",
            "4",
        ),
        // Reaching a binding *through* the alias, not just checking the name
        // set: a lazy prefix of an infinite stream (the shape chibi's SRFI 41
        // cannot take — see lib/srfi/PROVENANCE.md) and `stream-match`, whose
        // `_` the alias re-exports for the macro's sake.
        (
            "(import (scheme stream)) \
             (stream->list 2 (stream-filter (lambda (n) (< n 2)) (stream-from 0)))",
            "(0 1)",
        ),
        (
            "(import (scheme stream)) (stream-match (stream 1 2) ((a b) (+ a b)) (_ 'no))",
            "3",
        ),
        (
            "(import (scheme charset)) (char-set-contains? (char-set #\\a #\\b) #\\a)",
            "#t",
        ),
        (
            "(import (scheme list-queue)) \
             (let ((q (list-queue 1 2))) (list-queue-add-back! q 3) (list-queue-list q))",
            "(1 2 3)",
        ),
        // n-ary on purpose: the one-list paths are indistinguishable from
        // SRFI 1's, while the multi-list ones go through `%cars+cdrs` and a
        // continuation invoked with two values — which is where this
        // library's one live defect was (`ievery` answered #t without ever
        // calling its predicate).
        (
            "(import (scheme ilist)) \
             (list (ilist->list (iappend (imap (lambda (x) (* x x)) (ilist 1 2)) (ilist 9))) \
                   (ievery < (ilist 5 6) (ilist 1 2)) \
                   (ievery < (ilist 1 2) (ilist 5 6)) \
                   (ilist->list (imap + (ilist 1 2) (ilist 10 20))))",
            "((1 4 9) #f #t (11 22))",
        ),
        // Both ends and the rebalance, not just construction: the
        // implementation is a banker's deque over two streams, so the
        // interesting behaviour is what happens when one chain runs empty and
        // the other has to be split — which is what `ideque-remove-front` on a
        // deque built entirely from the back reaches.
        (
            "(import (scheme ideque) (scheme base)) \
             (define built-from-back \
               (ideque-add-back (ideque-add-back (ideque-add-back (ideque) 1) 2) 3)) \
             (list (ideque->list built-from-back) \
                   (ideque-front built-from-back) \
                   (ideque-back built-from-back) \
                   (ideque->list (ideque-remove-front built-from-back)) \
                   (ideque= = (ideque 1 2 3) built-from-back) \
                   (ideque= = (ideque 1 2) built-from-back))",
            "((1 2 3) 1 3 (2 3) #t #f)",
        ),
        // The only cargo-test reach into `(scheme rlist)`, so it covers what a
        // lane cannot: `rquote`, a *macro* exported through two renames
        // (`ra:quote` -> `quote` -> `rquote`) and the likeliest thing to break
        // silently; the higher-order arms; the update operations; and one of
        // the 28 compound accessors, whose rename targets are hand-written.
        (
            "(import (scheme base) (scheme rlist)) \
             (define l (rlist 1 2 3 4 5)) \
             (list (rlist->list (rquote (a b c))) \
                   (rlist->list (rmap + l (rlist 10 20 30 40 50))) \
                   (rlist->list (rlist-set l 0 'x)) \
                   (rcadr l) \
                   (rlist->list (rreverse (rlist-tail l 3))) \
                   (rlength (make-rlist 3 'z)) \
                   (rlist->list (list->rlist '(7 8))))",
            "((a b c) (11 22 33 44 55) (x 2 3 4 5) 2 (5 4) 3 (7 8))",
        ),
        // An arithmetic op, a rounding op and the two things this bundle had
        // to fix. The two `fl/` probes are chosen to be independent: the first
        // fails if the divisor's sign is ignored, the second if the
        // *numerator's* is, so neither can pass for the other's reason. Then
        // `flnumerator` at an infinity, which the R7RS `numerator` the
        // reference implementation delegates to will not take.
        (
            "(import (scheme flonum) (scheme base)) \
             (list (fl+ 1.0 2.0) (flfloor 2.7) (fl/ -0.0) (fl/ -1.0 -0.0) \
                   (flnumerator +inf.0) (fldenominator -inf.0) (flsign-bit -1.0))",
            "(3.0 2.0 -inf.0 +inf.0 +inf.0 1.0 1)",
        ),
        // A text is not a string, so the conversions are the interesting part
        // — and `textual-` procedures accept both, which is the distinction
        // this case pins along with the kernel's own indexing.
        (
            "(import (scheme text) (scheme base)) \
             (define t (string->text \"hello\")) \
             (list (text? t) (text? \"hello\") (textual? \"hello\") \
                   (text-length t) (textual->string (subtext t 1 3)) \
                   (textual->string (textual-upcase t)) \
                   (textual->string (textual-append t (text #\\!))))",
            "(#t #f #t 5 \"el\" \"HELLO\" \"hello!\")",
        ),
        // Generator-backed on purpose: a plain list exercises paths
        // indistinguishable from SRFI 1's `take`, and it was exactly the
        // generator path that carried the `lseq-append` defect chibi's copy
        // shipped with.
        (
            "(import (scheme lseq) (scheme base)) \
             (define (gen . xs) \
               (let ((l xs)) \
                 (generator->lseq \
                   (lambda () (if (null? l) (eof-object) \
                                  (let ((x (car l))) (set! l (cdr l)) x)))))) \
             (lseq-realize (lseq-append (gen 1 2 3) (gen 4 5)))",
            "(1 2 3 4 5)",
        ),
    ];
    for (src, expected) in cases {
        assert_eq!(eval_to_string(src), expected, "for: {src}");
    }
}

/// The four `PATINA LOCAL EDIT`s in SRFI 135's body, which neither suite
/// reaches.
///
/// All four are upstream defects — chibi ships the same code and still has
/// them — and all four turn on the argument being a *text* rather than a
/// string, which is why an interface suite that mostly passes strings misses
/// them. The shape that triggers the first three is an ASCII cased character
/// *before* a character above U+007F: the scanner starts on an all-ASCII fast
/// path, switches to the slow one at the cased character, and the slow path
/// was the broken copy.
#[test]
fn text_case_and_replicate_local_edits() {
    // `subtext` returns a text; `string-upcase` and `string-caser` take
    // strings. Without the conversion these raised a type error.
    assert_eq!(
        eval_to_string(
            "(import (scheme text) (scheme base)) \
             (textual->string (textual-upcase (string->text \"a\\xdf;\")))"
        ),
        "\"ASS\""
    );
    assert_eq!(
        eval_to_string(
            "(import (scheme text) (scheme base)) \
             (textual->string (textual-downcase (string->text \"A\\xdf;\")))"
        ),
        "\"a\u{df}\""
    );

    // `textual-foldcase` on a text applied `textual-downcase` instead of the
    // caser it was handed, so folding degraded to downcasing. A text and a
    // string must fold alike, and a medial sigma must not fold to a final one.
    assert_eq!(
        eval_to_string(
            "(import (scheme text) (scheme base)) \
             (list (textual->string (textual-foldcase (string->text \"\\xdf;\"))) \
                   (textual->string (textual-foldcase \"\\xdf;\")) \
                   (textual->string (textual-foldcase (string->text \"\\x39e;\\x3a3;\"))))"
        ),
        "(\"ss\" \"ss\" \"\u{3be}\u{3c3}\")"
    );

    // The degenerate slice returned the *string* "" where SRFI 135 says text.
    assert_eq!(
        eval_to_string(
            "(import (scheme text) (scheme base)) \
             (list (text? (textual-replicate \"abc\" 0 0)) \
                   (text-length (textual-replicate \"abc\" 0 0)) \
                   (textual->string (textual-replicate \"abc\" 2 5)))"
        ),
        "(#t 0 \"cab\")"
    );
}

/// The alias and the SRFI must denote the same binding, not two copies.
///
/// Checked for every alias, not just one. Comparing export *name sets*, which
/// is what `test_alias_exports_match_backing_srfi` does, cannot tell a
/// re-export from a re-implementation: a `(scheme ideque)` that defined its own
/// 48 procedures would match name for name and pass. Importing both libraries
/// into one program is what distinguishes them — two distinct bindings for one
/// name is a conflict, and one binding reached twice is not.
#[test]
fn test_alias_and_srfi_share_bindings() {
    for (name, srfi) in ALIASES {
        // `(srfi 101)`'s `quote` shadows the special form, and importing it
        // alongside anything that quotes trips triage family 33 — including
        // this program's own `'ok`. The pair is checked below by name instead.
        if RENAMING_ALIASES.contains(name) {
            continue;
        }
        let src = format!("(import (scheme {name}) (srfi {srfi})) 'ok");
        assert_eq!(
            eval_to_string(&src),
            "ok",
            "(scheme {name}) and (srfi {srfi}) must be the same bindings, not two copies"
        );
    }
    // `(scheme rlist)` renames, so it needs its own shape — but it still has
    // to be the *same* bindings, which means reaching one library's
    // constructor and the other's operation in one expression. `only` is what
    // makes that possible: importing `(srfi 101)` wholesale would rebind
    // `quote` and trip triage family 33 on this program's own literals.
    //
    // A `(scheme rlist)` that re-implemented its 48 procedures would answer
    // this with a type error, not `(1 2 3 4)` — `rappend` would be walking a
    // kons record type that `list` never built.
    assert_eq!(
        eval_to_string(
            "(import (except (scheme base) length list) (scheme rlist) \
                     (only (srfi 101) list length)) \
             (rlist->list (rappend (rlist 1 2) (list 3 4)))"
        ),
        "(1 2 3 4)"
    );

    // One that also *uses* a shared binding, so the check is not only that the
    // imports coexist.
    assert_eq!(
        eval_to_string("(import (scheme box) (srfi 111)) (unbox (box 7))"),
        "7",
        "importing an alias and its backing SRFI together must not conflict"
    );
}
