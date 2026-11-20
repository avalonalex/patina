//! A macro's template must denote what its names meant where the macro was
//! *defined*, not where it is used — R7RS referential transparency.
//!
//! Patina previously dropped the definition environment entirely. A free
//! identifier in a template was emitted as a bare name and resolved in the
//! importing program, so an exported macro could only reference bindings its
//! caller happened to have. Library-private helpers were the common casualty:
//!
//! ```scheme
//! (define-library (t proc)
//!   (import (scheme base))
//!   (export m)
//!   (begin
//!     (define (helper x) (* 5 x))
//!     (define-syntax m (syntax-rules () ((m x) (helper x))))))
//! ```
//! `(m 4)` from another module => "unbound variable: helper".
//!
//! This looked narrower than it was. Referencing an *imported* binding appeared
//! to work, but only because every Rust primitive is installed into globals
//! unconditionally — a Scheme-defined import failed exactly the same way. The
//! standard library escaped notice because its macro templates happen to expand
//! to primitives only (`parameterize` inlines `list`/`map`/`for-each` rather
//! than calling its own `%set-params!` helper).
//!
//! Found via `(chibi test)`, whose `test` macro expands into an internal
//! `test-vars` macro — the most-depended library in the ecosystem.

// The bare-value `eval_*` forms (#605), deprecated until stage 5e removes them.
#![allow(deprecated)]

mod common;

use patina_tree_walker::Evaluator;
use std::fs;
use tempfile::TempDir;

/// Write a set of `(name, source)` libraries under `<tmp>/t/` and return an
/// evaluator that can find them.
fn eval_with_libs(libs: &[(&str, &str)]) -> (Evaluator, TempDir) {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("t");
    fs::create_dir(&dir).unwrap();
    for (file, src) in libs {
        fs::write(dir.join(file), src).unwrap();
    }
    let eval = Evaluator::new();
    eval.add_library_search_path(temp.path().to_path_buf());
    (eval, temp)
}

/// Load `(t <lib>)` and read one of its exported bindings as a fixnum.
fn exported_fixnum(eval: &Evaluator, lib: &str, name: &str) -> i64 {
    let library = eval
        .load_library(&["t".to_string(), lib.to_string()])
        .unwrap_or_else(|e| panic!("failed to load (t {lib}): {e}"));
    library
        .env
        .get(&eval.heap, name)
        .unwrap_or_else(|| panic!("(t {lib}) does not define {name}"))
        .as_fixnum()
        .unwrap_or_else(|| panic!("{name} is not a fixnum"))
}

#[test]
fn test_macro_reaches_library_private_procedure() {
    let (eval, _t) = eval_with_libs(&[
        (
            "proc.sld",
            r#"(define-library (t proc)
                 (import (scheme base))
                 (export m)
                 (begin
                   (define (helper x) (* 5 x))
                   (define-syntax m (syntax-rules () ((m x) (helper x))))))"#,
        ),
        (
            "useproc.sld",
            r#"(define-library (t useproc)
                 (import (scheme base) (t proc))
                 (export r)
                 (begin (define r (m 4))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "useproc", "r"), 20);
}

#[test]
fn test_macro_reaches_library_private_variable() {
    let (eval, _t) = eval_with_libs(&[
        (
            "var.sld",
            r#"(define-library (t var)
                 (import (scheme base))
                 (export mv)
                 (begin
                   (define secret 99)
                   (define-syntax mv (syntax-rules () ((mv) secret)))))"#,
        ),
        (
            "usevar.sld",
            r#"(define-library (t usevar)
                 (import (scheme base) (t var))
                 (export r)
                 (begin (define r (mv))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "usevar", "r"), 99);
}

/// The `(chibi test)` shape: an exported macro expanding into a *private* macro.
#[test]
fn test_macro_reaches_library_private_macro() {
    let (eval, _t) = eval_with_libs(&[
        (
            "mac.sld",
            r#"(define-library (t mac)
                 (import (scheme base))
                 (export outer)
                 (begin
                   (define-syntax outer (syntax-rules () ((outer x) (inner x))))
                   (define-syntax inner (syntax-rules () ((inner x) (* 2 x))))))"#,
        ),
        (
            "usemac.sld",
            r#"(define-library (t usemac)
                 (import (scheme base) (t mac))
                 (export r)
                 (begin (define r (outer 21))))"#,
        ),
    ]);
    // Also confirms a forward reference works: `inner` is defined after `outer`.
    assert_eq!(exported_fixnum(&eval, "usemac", "r"), 42);
}

/// A Scheme-defined import — the case that disproved "imports already work".
#[test]
fn test_macro_reaches_binding_imported_by_defining_library_only() {
    let (eval, _t) = eval_with_libs(&[
        (
            "dep.sld",
            r#"(define-library (t dep)
                 (import (scheme base))
                 (export dep-fn)
                 (begin (define (dep-fn x) (* 100 x))))"#,
        ),
        (
            "user.sld",
            r#"(define-library (t user)
                 (import (scheme base) (t dep))
                 (export md)
                 (begin (define-syntax md (syntax-rules () ((md x) (dep-fn x))))))"#,
        ),
        (
            // Deliberately does NOT import (t dep).
            "useuser.sld",
            r#"(define-library (t useuser)
                 (import (scheme base) (t user))
                 (export r)
                 (begin (define r (md 3))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "useuser", "r"), 300);
}

/// Relinking must apply only to names the *template* mentions. An identifier
/// arriving by pattern-variable substitution is the caller's code and must keep
/// resolving at the use site — even when the defining library happens to bind
/// the same name. Regression: `log` is a `(scheme base)` primitive, so a user's
/// own `log` was being hijacked and rewritten to the primitive.
#[test]
fn test_substituted_identifiers_still_resolve_at_the_use_site() {
    let (eval, _t) = eval_with_libs(&[
        (
            "wrap.sld",
            r#"(define-library (t wrap)
                 (import (scheme base))
                 (export capture)
                 (begin (define-syntax capture (syntax-rules () ((capture e) (list e))))))"#,
        ),
        (
            "uselog.sld",
            r#"(define-library (t uselog)
                 (import (scheme base) (t wrap))
                 (export r)
                 (begin
                   ;; `log` shadows the (scheme base) primitive here.
                   (define log 7)
                   (define r (car (capture log)))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "uselog", "r"), 7);
}

/// The definition binding wins over a same-named binding at the use site.
///
/// R7RS 4.3.2: a free identifier in a template refers to the binding in scope
/// where the macro was written. `(t amb)` and `(t useamb)` each define
/// `shared`; `pick` must reach the one it was written next to.
///
/// Patina briefly did the opposite — the first cut of the definition-environment
/// fix only relinked names the use site could not resolve at all, which left
/// this case silently capturing. Fixed while the cost of changing was still low.
#[test]
fn test_definition_binding_wins_over_use_site() {
    let (eval, _t) = eval_with_libs(&[
        (
            "amb.sld",
            r#"(define-library (t amb)
                 (import (scheme base))
                 (export pick)
                 (begin
                   (define (shared) 1)
                   (define-syntax pick (syntax-rules () ((pick) (shared))))))"#,
        ),
        (
            "useamb.sld",
            r#"(define-library (t useamb)
                 (import (scheme base) (t amb))
                 (export r)
                 (begin
                   (define (shared) 2)
                   (define r (pick))))"#,
        ),
    ]);
    assert_eq!(
        exported_fixnum(&eval, "useamb", "r"),
        1,
        "template's `shared` must be the one defined alongside the macro"
    );
}

// ─── Cases the first cut of the alias mechanism got wrong ────────────────────
//
// The rewrite that links template identifiers to the definition environment
// initially walked the raw expansion as a flat pair tree. That is the wrong
// shape for syntax in three ways, each of which corrupted working programs.

/// Quoted data denotes itself. Rewriting a name inside `quote` turned
/// `'(helper x)` into `'(helper.0 x)` — a corrupted datum, not a reference.
#[test]
fn test_quoted_data_is_not_rewritten() {
    let (eval, _t) = eval_with_libs(&[
        (
            "q.sld",
            r#"(define-library (t q)
                 (import (scheme base))
                 (export mq)
                 (begin
                   (define (helper x) x)
                   (define-syntax mq (syntax-rules () ((mq x) '(helper x))))))"#,
        ),
        (
            "useq.sld",
            r#"(define-library (t useq)
                 (import (scheme base) (t q))
                 (export r)
                 (begin (define r (length (mq 1)))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "useq", "r"), 2);
}

/// Vector elements are evaluated inside quasiquote, so the walk has to descend
/// into vectors. A pair-only walk silently left the reference unlinked.
#[test]
fn test_quasiquoted_vector_elements_are_rewritten() {
    let (eval, _t) = eval_with_libs(&[
        (
            "v.sld",
            r#"(define-library (t v)
                 (import (scheme base))
                 (export mvec)
                 (begin
                   (define (secret x) (* x 2))
                   (define-syntax mvec (syntax-rules () ((mvec e) `#(,(secret e)))))))"#,
        ),
        (
            "usev.sld",
            r#"(define-library (t usev)
                 (import (scheme base) (t v))
                 (export r)
                 (begin (define r (vector-ref (mvec 5) 0))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "usev", "r"), 10);
}

/// Writes must follow the alias as reads do. `get` was taught to and `set` was
/// not, so a template that mutated a library-private binding failed — while the
/// field documentation claimed the opposite.
#[test]
fn test_set_through_a_template_reference_is_live() {
    let (eval, _t) = eval_with_libs(&[
        (
            "c.sld",
            r#"(define-library (t c)
                 (import (scheme base))
                 (export bump)
                 (begin
                   (define counter 0)
                   (define-syntax bump
                     (syntax-rules () ((bump) (begin (set! counter (+ counter 1)) counter))))))"#,
        ),
        (
            "usec.sld",
            r#"(define-library (t usec)
                 (import (scheme base) (t c))
                 (export r)
                 (begin
                   (bump)
                   (bump)
                   (define r (bump))))"#,
        ),
    ]);
    // 3, not 1: the alias resolves afresh each time rather than snapshotting.
    assert_eq!(exported_fixnum(&eval, "usec", "r"), 3);
}

/// Aliases must land in the environment the code is resolved in. `self.env` at
/// desugar time can be a transient child made for a `let-syntax` body, which is
/// dropped before the program runs — so the alias went with it.
#[test]
fn test_expansion_inside_a_let_syntax_body_resolves() {
    let (eval, _t) = eval_with_libs(&[
        (
            "p.sld",
            r#"(define-library (t p)
                 (import (scheme base))
                 (export mp)
                 (begin
                   (define (helper x) (* 5 x))
                   (define-syntax mp (syntax-rules () ((mp x) (helper x))))))"#,
        ),
        (
            "usep.sld",
            r#"(define-library (t usep)
                 (import (scheme base) (t p))
                 (export r)
                 (begin
                   (define r
                     (let-syntax ((ignore (syntax-rules () ((ignore) 0))))
                       (mp 5)))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "usep", "r"), 25);
}

/// A tail is not a form. `rewrite_refs` used to recurse on each cdr as though
/// it were, so in `(pass quote (helper 1 2))` the tail `(quote (helper 1 2))`
/// read as a quote form and everything after the argument `quote` was left
/// unrelinked — `helper` stayed a bare name and was resolved, unsuccessfully,
/// at the use site.
///
/// Nothing about `quote` as an *argument* is exotic: `(chibi match)` reaches it
/// for any pattern with a quoted tail, since `(x . 'bad)` reads as
/// `(x quote bad)` and is threaded through `match-two` as three arguments.
#[test]
fn test_quote_as_an_argument_does_not_stop_relinking() {
    let (eval, _t) = eval_with_libs(&[
        (
            "q.sld",
            r#"(define-library (t q)
                 (import (scheme base))
                 (export mq)
                 (begin
                   (define (helper x) (* 5 x))
                   (define-syntax pass (syntax-rules () ((pass a b) b)))
                   (define-syntax mq
                     (syntax-rules () ((mq x) (pass quote (helper x)))))))"#,
        ),
        (
            "useq.sld",
            r#"(define-library (t useq)
                 (import (scheme base) (t q))
                 (export r)
                 (begin (define r (mq 6))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "useq", "r"), 30);
}

/// The same defect one layer down, which is the shape `(chibi match)` actually
/// hits: the identifier is substituted into the *pattern* of a macro that a
/// template defines with `let-syntax`, and the relinked reference rides along
/// as another argument. This is `match-check-ellipsis`'s trick in miniature.
#[test]
fn test_quote_substituted_into_a_nested_macro_pattern() {
    let (eval, _t) = eval_with_libs(&[
        (
            "n.sld",
            r#"(define-library (t n)
                 (import (scheme base))
                 (export mn)
                 (begin
                   (define (helper x) (* 5 x))
                   (define-syntax check
                     (syntax-rules ()
                       ((check id sk fk)
                        (let-syntax ((inner (syntax-rules ()
                                              ((inner (foo id) s f) s)
                                              ((inner other s f) f))))
                          (inner (a b c) sk fk)))))
                   (define-syntax mn
                     (syntax-rules ()
                       ((mn id x) (check id 0 (helper x)))))))"#,
        ),
        (
            "usen.sld",
            r#"(define-library (t usen)
                 (import (scheme base) (t n))
                 (export r s)
                 (begin
                   (define r (mn quote 7))
                   (define s (mn zzz 7))))"#,
        ),
    ]);
    // Both spellings must relink; only the `quote` one used to fail.
    assert_eq!(exported_fixnum(&eval, "usen", "r"), 35);
    assert_eq!(exported_fixnum(&eval, "usen", "s"), 35);
}

/// `` `(a . ,e) `` reads as `(quasiquote (a unquote e))`: the unquote sits in
/// the spine's *interior*, in cdr position. The spine walk that fixed
/// `(f quote x)` treated it as an ordinary argument and rewrote `e` a level
/// too deep, so the helper inside it was never relinked — the dual of the case
/// that walk was written for (audit C1).
#[test]
fn test_dotted_quasiquote_relinks_its_unquoted_expression() {
    let (eval, _t) = eval_with_libs(&[
        (
            "d.sld",
            r#"(define-library (t d)
                 (import (scheme base))
                 (export mq-dotted mq-nested mq-proper)
                 (begin
                   (define (helper x) (* 5 x))
                   (define-syntax mq-proper
                     (syntax-rules () ((mq-proper x) `(a ,(helper x)))))
                   (define-syntax mq-dotted
                     (syntax-rules () ((mq-dotted x) `(a . ,(helper x)))))
                   (define-syntax mq-nested
                     (syntax-rules () ((mq-nested x) `((b . ,(helper x)) c))))))"#,
        ),
        (
            "used.sld",
            r#"(define-library (t used)
                 (import (scheme base) (t d))
                 (export proper dotted nested)
                 (begin
                   (define proper (car (cdr (mq-proper 2))))
                   (define dotted (cdr (mq-dotted 2)))
                   (define nested (cdr (car (mq-nested 2))))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "used", "proper"), 10);
    assert_eq!(exported_fixnum(&eval, "used", "dotted"), 10);
    assert_eq!(exported_fixnum(&eval, "used", "nested"), 10);
}

/// The guard that makes the above safe: outside a template, `(f unquote x)` is
/// an ordinary call whose second argument is spelled `unquote`, and its third
/// must still be relinked at depth 0. Symmetric with
/// `test_quote_as_an_argument_does_not_stop_relinking`.
#[test]
fn test_unquote_as_an_argument_does_not_change_depth() {
    let (eval, _t) = eval_with_libs(&[
        (
            "u.sld",
            r#"(define-library (t u)
                 (import (scheme base))
                 (export mu)
                 (begin
                   (define (helper x) (* 5 x))
                   (define-syntax pass (syntax-rules () ((pass a b) b)))
                   (define-syntax mu
                     (syntax-rules () ((mu x) (pass unquote (helper x)))))))"#,
        ),
        (
            "useu.sld",
            r#"(define-library (t useu)
                 (import (scheme base) (t u))
                 (export r)
                 (begin (define r (mu 6))))"#,
        ),
    ]);
    assert_eq!(exported_fixnum(&eval, "useu", "r"), 30);
}

/// A macro's definition environment outlives a collection: an exported
/// macro expanded by a program after the heap has been collected still
/// reaches the library-private procedure its template names, on both
/// backends.
///
/// Every other test here works inside library loads, from
/// `Evaluator::load_library` outside any loop, which deferred collection for
/// as long as the library's unevaluated body existed (`ParsedLibrary`'s
/// `GcDeferGuard::holding`), so under the per-PR stress lane this was the one
/// that collected (`scripts/run_gc_stress_tests.sh`, #626). Since #677 those
/// loads collect inside their bodies under stress as well. `(gc)` makes this
/// one collect without stress too.
#[test]
fn test_macro_reaches_its_library_after_a_collection() {
    let program = r#"(define-library (t collected)
                       (import (scheme base))
                       (export m)
                       (begin
                         (define (helper x) (* 5 x))
                         (define-syntax m (syntax-rules () ((m x) (helper x))))))
                     (import (scheme base) (t collected) (patina debug))
                     (define (churn n acc)
                       (if (= n 0) (length acc) (churn (- n 1) (cons n acc))))
                     (churn 1000 '())
                     (gc)
                     (churn 1000 '())
                     (list (m 4) (m 5))"#;
    assert_eq!(common::eval_program(program), "(20 25)");
}

// ─── One alias per target (#611) ────────────────────────────────────────────
//
// Relinking gives a template's reference to a private helper an alias in the
// program. It used to mint one per expansion, which nothing removes, so each
// top-level `guard` — whose helper `%guard-aux` is private — left three
// aliases and their symbols behind. One target now has one alias
// (`Environment::relink_alias`).

/// A library whose macros name one exported helper and one private one.
const PRIVATE_HELPER: &str = "(define-library (t m)
  (export via-public public-helper via-private)
  (import (scheme base))
  (begin
    (define (public-helper x) x)
    (define (private-helper x) x)
    (define-syntax via-public (syntax-rules () ((_ x) (public-helper x))))
    (define-syntax via-private (syntax-rules () ((_ x) (private-helper x))))))
";

/// The interned symbols after `forms` (top-level forms, `n` times), on each
/// backend.
fn symbols_after(prelude: &str, form: &str, n: usize) -> [String; 2] {
    let program = format!(
        "{prelude}{}(cdr (assq 'symbols (gc-stats)))",
        format!("{form}\n").repeat(n)
    );
    [
        common::eval_program_vm(&program),
        common::eval_program_tree_walker(&program),
    ]
}

#[test]
fn repeated_top_level_expansions_add_no_symbols() {
    let prelude =
        format!("{PRIVATE_HELPER}(import (scheme base) (scheme write) (patina debug) (t m))\n");
    for form in [
        "(guard (e (#t 0)) (raise 'x))",
        "(guard (e ((string? e) 1) ((number? e) 2) (#t 0)) (raise 'x))",
        "(via-private 1)",
        "(via-public 1)",
    ] {
        assert_eq!(
            symbols_after(&prelude, form, 1000),
            symbols_after(&prelude, form, 3000),
            "{form}: [VM, tree-walker] after 1,000 and 3,000 forms"
        );
    }
}

#[test]
fn repeated_evals_of_a_guard_form_add_no_symbols() {
    let prelude = "(import (scheme base) (scheme eval) (scheme repl) (patina debug))\n";
    let symbols = |n: usize| {
        symbols_after(
            prelude,
            &format!(
                "(let loop ((i 0)) (when (< i {n}) \
                 (eval '(guard (e (#t 0)) (raise 'x)) (interaction-environment)) \
                 (loop (+ i 1))))"
            ),
            1,
        )
    };
    assert_eq!(symbols(1000), symbols(3000), "[VM, tree-walker]");
}

/// A library whose macros read and assign its private `secret`, use its
/// private keyword `priv` as a value, and tell the spelling a template's
/// `secret` has in an expansion: `(spell-secret)` hands it to a quoting
/// macro, and a relinked identifier used as data keeps the spelling
/// relinking gave it. A test that needs that spelling reads it so, and
/// never computes it.
const LEAK: &str = "(define-library (t leak)
  (export get-secret bump-secret! spell-secret priv-as-a-value)
  (import (scheme base))
  (begin
    (define secret 42)
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax spell-secret (syntax-rules () ((_) (quote-it secret))))
    (define-syntax get-secret (syntax-rules () ((_) secret)))
    (define-syntax bump-secret! (syntax-rules () ((_) (set! secret (+ secret 1)))))
    (define-syntax priv (syntax-rules () ((_ x) x)))
    (define-syntax priv-as-a-value (syntax-rules () ((_) (list priv))))))
(import (scheme base) (t leak))
";

/// The spelling relinking gives a template's `secret` is no name the code
/// already compiled reads: `h` and `b` expanded `get-secret` and
/// `bump-secret!` before the program defined that spelling, and still read
/// and assign the library's variable, as does an expansion after the
/// definition; the program's definition is the program's own. chibi 0.12
/// and Gauche 0.9.15 answer the same for a name the program defines.
/// Before #837's second PR `h` read by name, and answered `mine`.
fn defining_the_relinked_spelling<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> [String; 2] {
    interp
        .eval_program(&format!(
            "{LEAK}(define (h) (get-secret)) (define (b) (bump-secret!))"
        ))
        .unwrap();
    let spelling = interp.display_tagged(interp.eval_str("(spell-secret)").unwrap());
    assert!(
        spelling.starts_with("secret") && spelling != "secret",
        "the spelling relinking gave `secret`, read from an expansion: {spelling}"
    );
    interp
        .eval_program(&format!("(define {spelling} 'mine)"))
        .unwrap();
    // Each answer is shown before the next evaluation, which may collect.
    let after = interp.display_tagged(
        interp
            .eval_str("(let* ((bumped (b)) (read (h))) (list read (get-secret)))")
            .unwrap(),
    );
    let own = interp.display_tagged(interp.eval_str(&spelling).unwrap());
    [after, own]
}

#[test]
fn a_definition_of_a_relinked_spelling_captures_nothing() {
    let expected = ["(43 43)".to_string(), "mine".to_string()];
    assert_eq!(
        defining_the_relinked_spelling(common::vm_interpreter()),
        expected,
        "VM"
    );
    assert_eq!(
        defining_the_relinked_spelling(common::tree_walker_interpreter()),
        expected,
        "tree-walker"
    );
}

/// A library whose macros read its private `secret`, expand its private
/// keyword `priv`, and tell the spelling a template's `secret` and `priv`
/// have in an expansion.
const NO_NAME: &str = "(define-library (t noname)
  (export get-secret spell-secret use-priv spell-priv)
  (import (scheme base))
  (begin
    (define secret 42)
    (define-syntax priv (syntax-rules () ((_ x) (list 'priv x))))
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax get-secret (syntax-rules () ((_) secret)))
    (define-syntax spell-secret (syntax-rules () ((_) (quote-it secret))))
    (define-syntax use-priv (syntax-rules () ((_ x) (priv x))))
    (define-syntax spell-priv (syntax-rules () ((_) (quote-it priv))))))
(import (scheme base) (t noname))
";

/// Expansions of `(t noname)`'s macros at top level and in a procedure.
const NO_NAME_EXPANSIONS: &str = "(define (h) (get-secret))
(use-priv 1)
(get-secret)
(define (k) (use-priv 2))
";

/// What `source` answers, or `error` with its message.
fn answer_or_error<B: patina_runtime::Backend>(
    interp: &patina_interpreter::Interpreter<B>,
    source: &str,
) -> Result<String, String>
where
    B::Error: std::fmt::Display,
{
    match interp.eval_str_owned(source) {
        Ok(value) => Ok(interp.display_tagged(&value)),
        Err(e) => Err(e.to_string()),
    }
}

/// The spelling relinking gives a template identifier is no name in a
/// program's text (#837, #873). The program reads each spelling from an
/// expansion and writes it: as a reference, as an assignment's target, as
/// the head of a form, for the library's private variable and for its
/// private keyword. Each is an error about a name that is not defined, and
/// the library's variable and keyword are what they were. chibi 0.12 and
/// Gauche 0.9.15 answer the same for a name the program wrote and bound
/// nowhere. Until the switch the spelling was an alias in the program's
/// namespace: the reference read 42, the assignment wrote the library's
/// variable, the form expanded the library's private macro, and assigning
/// the keyword's spelling was refused as an assignment to a macro.
fn writing_the_relinked_spellings<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> Vec<String>
where
    B::Error: std::fmt::Display,
{
    interp
        .eval_program_owned(&format!("{NO_NAME}{NO_NAME_EXPANSIONS}"))
        .expect("the library and the expansions");
    let spelling_of = |speller: &str, written: &str| {
        let spelling = answer_or_error(&interp, speller).expect("spells");
        assert!(
            spelling.starts_with(written) && spelling != written,
            "the spelling relinking gave `{written}`, read from an expansion: {spelling}"
        );
        spelling
    };
    let secret = spelling_of("(spell-secret)", "secret");
    let keyword = spelling_of("(spell-priv)", "priv");
    let refused = |source: String| match answer_or_error(&interp, &source) {
        Ok(answer) => format!("answered {answer}"),
        Err(message) => {
            assert!(
                !message.contains("is a macro"),
                "{source} was refused as a keyword: {message}"
            );
            "refused".to_string()
        }
    };
    vec![
        refused(secret.clone()),
        refused(format!("(set! {secret} 'hacked)")),
        answer_or_error(&interp, "(list (h) (get-secret))").expect("reads"),
        refused(format!("({keyword} 2)")),
        refused(format!("(set! {keyword} 0)")),
        answer_or_error(&interp, "(use-priv 3)").expect("expands"),
    ]
}

#[test]
fn a_relinked_spelling_is_no_name_in_the_programs_text() {
    let expected = [
        "refused", "refused", "(42 42)", "refused", "refused", "(priv 3)",
    ];
    assert_eq!(
        writing_the_relinked_spellings(common::vm_interpreter()),
        expected,
        "VM"
    );
    assert_eq!(
        writing_the_relinked_spellings(common::tree_walker_interpreter()),
        expected,
        "tree-walker"
    );
}

/// After expansions of `(t noname)`'s macros, at top level and in a
/// procedure: whether the spelling relinking gave the private `secret` or
/// the private `priv` is a name of the program's, and every name the
/// program's namespace lists that starts as one of them would.
fn names_relinking_left<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> (bool, bool, Vec<String>)
where
    B::Error: std::fmt::Display,
{
    interp
        .eval_program_owned(&format!("{NO_NAME}{NO_NAME_EXPANSIONS}"))
        .expect("the library and the expansions");
    let secret = answer_or_error(&interp, "(spell-secret)").expect("spells");
    let keyword = answer_or_error(&interp, "(spell-priv)").expect("spells");
    assert!(
        secret.starts_with("secret") && secret != "secret",
        "{secret}"
    );
    assert!(
        keyword.starts_with("priv") && keyword != "priv",
        "{keyword}"
    );
    let env = interp.global_env();
    let heap = interp.backend().heap();
    let mut listed: Vec<String> = env
        .local_names(heap)
        .into_iter()
        .chain(env.get_all_names(heap))
        .chain(env.weak_entry_names_for_tests())
        .filter(|name| name.starts_with("secret.") || name.starts_with("priv."))
        .collect();
    listed.sort();
    listed.dedup();
    (
        is_a_name(&interp, &secret),
        is_a_name(&interp, &keyword),
        listed,
    )
}

/// A relinked reference mints no spelling in the program (#837's
/// acceptance): after expansions at top level and in a procedure, the
/// spelling the expansions gave the library's private variable and its
/// private keyword is nothing `get` answers and no slot, and the program's
/// namespace lists no name that starts `secret.` or `priv.`. Before the
/// switch each spelling was an alias, which `get` answered.
#[test]
fn a_relinked_reference_mints_no_spelling() {
    let none = (false, false, Vec::<String>::new());
    assert_eq!(names_relinking_left(common::vm_interpreter()), none, "VM");
    assert_eq!(
        names_relinking_left(common::tree_walker_interpreter()),
        none,
        "tree-walker"
    );
}

/// A relinked expansion leaves the program's names as they were: the
/// namespace's `local_names`, `bindings` and `get_all_names`, its
/// placeholders, the count of binding records and what the namespace has
/// charged for its tables, before the first expansions of `(t noname)`'s
/// macros, which define nothing, and after them. Before the switch the
/// expansions installed two aliases, and the namespace charged for the
/// table that held them.
fn a_relinked_expansion_leaves<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> [(NamesAndRecords, Option<usize>); 2] {
    interp.eval_program_owned(NO_NAME).expect("the library");
    let read = |interp: &patina_interpreter::Interpreter<B>| {
        interp.eval_program_owned("(gc)").expect("collects");
        (
            names_and_records(interp),
            interp.global_env().charged_bytes(),
        )
    };
    // `(patina debug)`'s import is part of the baseline.
    interp
        .eval_program_owned("(import (patina debug))")
        .expect("the import");
    let before = read(&interp);
    interp
        .eval_program_owned(
            "(get-secret) (list (use-priv 1)) ((lambda () (list (get-secret) (use-priv 2))))",
        )
        .expect("the expansions");
    [before, read(&interp)]
}

#[test]
fn a_relinked_expansion_leaves_the_programs_names_alone() {
    let [vm_before, vm_after] = a_relinked_expansion_leaves(common::vm_interpreter());
    let [tw_before, tw_after] = a_relinked_expansion_leaves(common::tree_walker_interpreter());
    assert_eq!(
        [vm_after, tw_after],
        [vm_before, tw_before],
        "[VM, tree-walker]"
    );
}

/// Only an identifier of the expansion is the relinked one. The program
/// defines a macro of its own whose template writes the very spelling an
/// expansion gave the library's private `secret`, and uses it in the same
/// form as the library's macro, whose expansion's entry for that spelling
/// is then in the form's table. The table answers an identifier only if it
/// carries the scope of the expansion that relinked it, and the program's
/// macro's does not: its reference is unbound, and after the program
/// defines the spelling, the program's own, while the library's macro
/// reads 42 throughout. Before the switch the spelling was an alias in the
/// program's namespace, and the program's macro read 42 through it.
fn a_programs_template_writing_the_spelling<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> [String; 2]
where
    B::Error: std::fmt::Display,
{
    interp
        .eval_program_owned(&format!("{NO_NAME}{NO_NAME_EXPANSIONS}"))
        .expect("the library and the expansions");
    let spelling = answer_or_error(&interp, "(spell-secret)").expect("spells");
    assert!(spelling.starts_with("secret") && spelling != "secret");
    interp
        .eval_program_owned(&format!(
            "(define-syntax mine (syntax-rules () ((_) {spelling})))"
        ))
        .expect("the program's macro");
    let unbound = answer_or_error(
        &interp,
        "(list (get-secret) (guard (e (#t 'unbound)) (mine)) (get-secret))",
    )
    .expect("one form");
    interp
        .eval_program_owned(&format!("(define {spelling} 'own)"))
        .expect("the program's definition");
    let own =
        answer_or_error(&interp, "(list (get-secret) (mine) (get-secret) (h))").expect("one form");
    [unbound, own]
}

#[test]
fn only_an_identifier_of_the_expansion_is_the_relinked_one() {
    let expected = ["(42 unbound 42)".to_string(), "(42 own 42 42)".to_string()];
    assert_eq!(
        a_programs_template_writing_the_spelling(common::vm_interpreter()),
        expected,
        "VM"
    );
    assert_eq!(
        a_programs_template_writing_the_spelling(common::tree_walker_interpreter()),
        expected,
        "tree-walker"
    );
}

/// How many names a collision library's template writes that are spelled
/// as relinking might spell its private `secret`.
const COLLISION_RANGE: usize = 400;

/// What the names a collision library's template writes are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WrittenNames {
    /// Bound nowhere.
    Unbound,
    /// The library's own variables, which the program does not import: each
    /// is relinked, as the private `secret` is.
    Private,
    /// The library's variables, which the program imports: each is one
    /// binding on both sides, and is left as the template wrote it.
    Shared,
}

/// Set in the child [`alone_in_a_process`] runs.
const ALONE: &str = "PATINA_MDE_ALONE_IN_A_PROCESS";

/// Whether this process is the one the test `test` has to itself. If it is
/// not, the test is run in a child that is, alone and on one thread, the
/// child must pass, and the caller has nothing left to do.
///
/// For a test that writes a range of names around a number it read and
/// needs the next library's number to fall in the range: the numbers come
/// from counters the whole process shares, and the other tests of this
/// file, on their threads, take thousands between two forms of one test.
fn alone_in_a_process(test: &str) -> bool {
    if std::env::var_os(ALONE).is_some() {
        return true;
    }
    let child = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .env(ALONE, "1")
        .args(["--exact", test, "--test-threads=1"])
        .output()
        .expect("run the child");
    assert!(
        child.status.success(),
        "{test}, alone in a process: {}\n{}{}",
        child.status,
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    false
}

/// The number a leaked spelling of `secret` starts with after its dot: the
/// part that tells one library's spelling from another's. Read only to
/// write a range of names around it and to check the range was wide enough;
/// what follows it is not read.
fn number_after_secret(spelling: &str) -> usize {
    let digits: String = spelling
        .strip_prefix("secret.")
        .unwrap_or_else(|| panic!("{spelling} is no spelling of `secret`"))
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits
        .parse()
        .unwrap_or_else(|_| panic!("{spelling} has no number after `secret.`"))
}

/// A library whose one template writes its private `secret` beside every
/// name `secret.k` for `k` in a range that starts where a library defined
/// just before it had its spelling, so that one of them is the spelling
/// relinking would give the private `secret` if nothing kept it clear of
/// the template's own names. `names` says what those names are to the
/// library and to the program. The program reads each `secret.k` in a
/// procedure of its own, under a `guard`. Answers what the template's
/// `secret` read and what each `secret.k` read, having checked from the
/// library's own leaked spelling that its number fell in the range and
/// that it is one of the names the template writes.
fn a_template_writing_the_relinked_spelling<B: patina_runtime::Backend>(
    interp: &patina_interpreter::Interpreter<B>,
    names_are: WrittenNames,
) -> (String, Vec<String>)
where
    B::Error: std::fmt::Display,
{
    interp
        .eval_program_owned(
            "(define-library (t first)
  (export spell-first)
  (import (scheme base))
  (begin
    (define secret 'first)
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax spell-first (syntax-rules () ((_) (quote-it secret))))))
(import (scheme base) (t first))",
        )
        .expect("the first library");
    let first = number_after_secret(&answer_or_error(interp, "(spell-first)").expect("spells"));
    let names: Vec<String> = (first..=first + COLLISION_RANGE)
        .map(|k| format!("secret.{k}"))
        .collect();
    let (exports, definitions) = if names_are == WrittenNames::Unbound {
        (String::new(), String::new())
    } else {
        (
            names.join(" "),
            names
                .iter()
                .map(|name| format!("(define {name} '{name})"))
                .collect::<Vec<_>>()
                .join("\n    "),
        )
    };
    let import = if names_are == WrittenNames::Shared {
        "(t collide)"
    } else {
        "(only (t collide) both spell-second)"
    };
    let thunks = names
        .iter()
        .map(|name| format!("(lambda () {name})"))
        .collect::<Vec<_>>()
        .join(" ");
    interp
        .eval_program_owned(&format!(
            "(define-library (t collide)
  (export both spell-second {exports})
  (import (scheme base))
  (begin
    (define secret 42)
    {definitions}
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax spell-second (syntax-rules () ((_) (quote-it secret))))
    (define-syntax both (syntax-rules () ((_) (list secret (list {thunks})))))))
(import {import})
(define read (both))"
        ))
        .expect("the collision library");
    let second_spelling = answer_or_error(interp, "(spell-second)").expect("spells");
    let second = number_after_secret(&second_spelling);
    assert!(
        (first..=first + COLLISION_RANGE).contains(&second),
        "the range missed the library's spelling: {second} is not in {first}..={}",
        first + COLLISION_RANGE
    );
    // The number alone falling in the range is not enough: were the
    // spelling to gain anything after it, no name the template writes
    // would meet it, and the test would pass for that reason.
    assert!(
        names.contains(&second_spelling),
        "the library's spelling {second_spelling} is none of the names the template writes"
    );
    let private = answer_or_error(interp, "(car read)").expect("the template's secret");
    let written = (0..names.len())
        .map(|k| {
            answer_or_error(
                interp,
                &format!("(guard (e (#t 'unbound)) ((list-ref (cadr read) {k})))"),
            )
            .expect("a guarded read")
        })
        .collect();
    (private, written)
}

/// A template that writes a name spelled as relinking would spell its
/// private `secret`, a name bound nowhere, reads nothing by it: the private
/// `secret` reads the library's 42 and every `secret.k` is unbound, as
/// chibi 0.12 and Gauche 0.9.15 answer for a template's unbound name. The
/// template's own text carries its expansion's scope as the relinked
/// identifier does, so relinking spells the private one clear of the names
/// the template writes (`relink_spelling`'s `taken`). Until the switch the
/// spelling was an alias, a name, and the `secret.k` that met it read 42.
#[test]
fn a_template_that_writes_the_relinked_spelling_unbound_reads_nothing() {
    if !alone_in_a_process("a_template_that_writes_the_relinked_spelling_unbound_reads_nothing") {
        return;
    }
    fn check<B: patina_runtime::Backend>(interp: patina_interpreter::Interpreter<B>, backend: &str)
    where
        B::Error: std::fmt::Display,
    {
        let (private, written) =
            a_template_writing_the_relinked_spelling(&interp, WrittenNames::Unbound);
        assert_eq!(private, "42", "{backend}");
        let bound: Vec<(usize, &String)> = written
            .iter()
            .enumerate()
            .filter(|(_, read)| *read != "unbound")
            .collect();
        assert!(
            bound.is_empty(),
            "{backend}: names the template wrote, by their place in the range, read {bound:?}"
        );
        assert_eq!(written.len(), COLLISION_RANGE + 1, "{backend}");
    }
    check(common::vm_interpreter(), "VM");
    check(common::tree_walker_interpreter(), "tree-walker");
}

/// A template that writes names spelled as relinking would spell its
/// private `secret`, each a variable of the library's own, reads its own
/// value by each and the library's 42 by `secret`, as chibi 0.12 and
/// Gauche 0.9.15 answer (`expansion/relinked-names.scm`'s first row is the
/// same over forty names, which meet the spelling only by chance). Twice:
/// with the names imported by the program, so that each is one binding on
/// both sides and is left as written, where relinking has to spell the
/// private `secret` clear of what the template writes; and with the names
/// private to the library, so that each is relinked too, where it also has
/// to spell each mention clear of what it has given another in the same
/// expansion. Before the switch the alias's mint skipped a spelling the
/// use site bound, and the first passed for that reason.
#[test]
fn a_template_that_writes_the_relinked_spelling_reads_its_own() {
    if !alone_in_a_process("a_template_that_writes_the_relinked_spelling_reads_its_own") {
        return;
    }
    fn check<B: patina_runtime::Backend>(
        interp: patina_interpreter::Interpreter<B>,
        names_are: WrittenNames,
        backend: &str,
    ) where
        B::Error: std::fmt::Display,
    {
        let (private, written) = a_template_writing_the_relinked_spelling(&interp, names_are);
        assert_eq!(private, "42", "{backend}, {names_are:?}");
        assert_eq!(written.len(), COLLISION_RANGE + 1, "{backend}");
        let wrong: Vec<(usize, &String)> = written
            .iter()
            .enumerate()
            .filter(|(_, read)| !read.starts_with("secret."))
            .collect();
        assert!(
            wrong.is_empty(),
            "{backend}, {names_are:?}: names that did not read their own value, by their place \
             in the range: {wrong:?}"
        );
        let mut distinct = written.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            written.len(),
            "{backend}, {names_are:?}: two names read one value"
        );
    }
    for names_are in [WrittenNames::Shared, WrittenNames::Private] {
        check(common::vm_interpreter(), names_are, "VM");
        check(common::tree_walker_interpreter(), names_are, "tree-walker");
    }
}

/// A generated macro carries the spelling relinking gave its template's
/// `secret`, and the program then defines that spelling. A procedure that
/// expanded the macro before the definition, an expansion after it and a
/// procedure compiled after it all read the library's variable: the
/// generated macro carries the record with the identifier, and nothing
/// reads the spelling by name. chibi 0.12 and Gauche 0.9.15 answer
/// `(42 42 42)` for a name the program defines. Before #837's second PR a
/// carried identifier was read by name, and all three answered `mine`.
fn defining_a_carried_spelling<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> [String; 2] {
    interp
        .eval_program(
            "(define-library (t gm)
  (export def-getter)
  (import (scheme base))
  (begin
    (define secret 42)
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax def-getter
      (syntax-rules ()
        ((_ name spell)
         (begin
           (define-syntax name (syntax-rules () ((_) secret)))
           (define-syntax spell (syntax-rules () ((_) (quote-it secret))))))))))
(import (scheme base) (t gm))
(def-getter get spell)
(define (early) (get))",
        )
        .unwrap();
    let spelling = interp.display_tagged(interp.eval_str("(spell)").unwrap());
    assert!(
        spelling.starts_with("secret") && spelling != "secret",
        "the spelling the generated macro carries, read from an expansion: {spelling}"
    );
    interp
        .eval_program(&format!("(define {spelling} 'mine) (define (late) (get))"))
        .unwrap();
    // Each answer is shown before the next evaluation, which may collect.
    let read = interp.display_tagged(interp.eval_str("(list (early) (get) (late))").unwrap());
    let own = interp.display_tagged(interp.eval_str(&spelling).unwrap());
    [read, own]
}

#[test]
fn a_definition_of_a_carried_spelling_captures_no_later_expansion() {
    let expected = ["(42 42 42)".to_string(), "mine".to_string()];
    assert_eq!(
        defining_a_carried_spelling(common::vm_interpreter()),
        expected,
        "VM"
    );
    assert_eq!(
        defining_a_carried_spelling(common::tree_walker_interpreter()),
        expected,
        "tree-walker"
    );
}

/// A generated definer run twice keeps two keywords. `(t defdef)`'s
/// `def-definer` makes a definer in the program whose template defines a
/// keyword spelled as the library's private `priv`, at top level, and a
/// macro that uses it. The binder was relinked with its references when
/// the definer was generated, so each run of the definer binds it at its
/// own scopes, and each generated macro expands the keyword of its own
/// run: `(one 3)` and `(two 4)`, with the library's own macro still
/// `lib-priv`. Gauche 0.9.15 answers the same. chibi 0.12 answers
/// `((one 1) (one 2))` and `((two 3) (two 4) (two 5))`: its top-level
/// `define-syntax` lands on the library's own keyword, so every user of
/// the spelling expands the newest definition. Before #837's second PR
/// the carried binder was bound by name, the second run redefined the
/// first's keyword, and Patina answered `(two 3)` too.
#[test]
fn a_generated_definer_run_twice_keeps_two_keywords() {
    let program = "(define-library (t defdef)
  (export def-definer use-priv)
  (import (scheme base))
  (begin
    (define-syntax priv (syntax-rules () ((_ x) (list 'lib-priv x))))
    (define-syntax use-priv (syntax-rules () ((_ x) (priv x))))
    (define-syntax def-definer
      (syntax-rules ()
        ((_ d)
         (define-syntax d
           (syntax-rules ()
             ((_ n tag)
              (begin (define-syntax priv (syntax-rules () ((_ y) (list 'tag y))))
                     (define-syntax n (syntax-rules () ((_ y) (priv y)))))))))))))
(import (scheme base) (t defdef))
(def-definer d)
(d g one)
(define first (list (g 1) (use-priv 2)))
(d g2 two)
(define (later) (list (g 3) (g2 4) (use-priv 5)))
(list first (later))";
    let expected = "(((one 1) (lib-priv 2)) ((one 3) (two 4) (lib-priv 5)))".to_string();
    assert_eq!(
        on_both_backends(program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// A generated macro that carries three relinks, one of each kind of
/// target: `read`, which `(t three)`'s `make-reader` makes in the program,
/// mentions the library's private variable `secret` (a namespace's slot),
/// the `count` the library's definer introduced beside the plain `count`
/// (an introduced definition), and `helper`, a keyword private to a
/// splice (a scoped keyword). The program has a `secret`, a `count` and a
/// `helper` of its own. `read` is expanded in forms after the one that
/// made it, at top level and in a procedure, past collections and an
/// assignment by the library, and reads all three of the library's. Its
/// macro object holds the three records, each under the spelling its
/// template wrote. chibi 0.12 answers the same with the library as a file
/// and without the collections (2026-11-17); Gauche 0.9.15 has no `(srfi
/// 188)`. Run in a release `gc-check` build too, where a record no link
/// may name is refused at every one of the three.
fn three_carried_relinks<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> (String, Vec<String>) {
    let answer = interp
        .eval_program(
            "(define-library (t three)
  (import (scheme base) (srfi 188))
  (export make-reader bump! plain-count)
  (begin
    (define secret 42)
    (define count 100)
    (define (plain-count) count)
    (define (bump!) (set! secret (+ secret 1)))
    (define-syntax def-state
      (syntax-rules ()
        ((_ maker)
         (begin
           (define count 0)
           (splicing-let-syntax ((helper (syntax-rules () ((_ x) (list 'helped x)))))
             (define-syntax maker
               (syntax-rules ()
                 ((_ name)
                  (define-syntax name
                    (syntax-rules ()
                      ((_) (begin (set! count (+ count 1))
                                  (helper (list secret count))))))))))))))
    (def-state make-reader)))
(import (scheme base) (patina debug) (t three))
(define secret 'program)
(define count 'program)
(define-syntax helper (syntax-rules () ((_ x) 'the-programs)))
(make-reader read)
(gc) (gc)
(define first (read))
(define (later) (read))
(bump!)
(gc)
(let* ((second (later)) (third (read)))
  (list first second third (plain-count) secret count (helper 0)))",
        )
        .unwrap();
    let answer = interp.display_tagged(answer);
    let global = interp.global_env();
    let mut written = patina_core::NoGcScope::with_cx(interp.backend().heap(), |cx| {
        let read = cx.lookup(&global, "read").expect("the generated macro");
        cx.macro_relinks(read)
            .iter()
            .map(|carried| carried.written.to_string())
            .collect::<Vec<_>>()
    });
    written.sort();
    (answer, written)
}

#[test]
fn a_generated_macro_carries_a_relink_of_each_kind() {
    let expected = (
        "((helped (42 1)) (helped (43 2)) (helped (43 3)) 100 program program the-programs)"
            .to_string(),
        vec![
            "count".to_string(),
            "helper".to_string(),
            "secret".to_string(),
        ],
    );
    assert_eq!(
        three_carried_relinks(common::vm_interpreter()),
        expected,
        "VM"
    );
    assert_eq!(
        three_carried_relinks(common::tree_walker_interpreter()),
        expected,
        "tree-walker"
    );
}

/// A desugar-time error that names a relinked identifier names it as its
/// template wrote it: the library's private keyword `priv` used as a
/// value is refused as `priv`, where the message read `priv.N`, the
/// spelling relinking gave it.
#[test]
fn a_relinked_keyword_used_as_a_value_is_named_as_written() {
    let program = format!("{LEAK}(priv-as-a-value)");
    for (backend, message) in [
        ("VM", common::try_eval_program_vm(&program).unwrap_err()),
        (
            "tree-walker",
            common::try_eval_program_tree_walker(&program).unwrap_err(),
        ),
    ] {
        assert!(
            message.contains("invalid use of syntax as a value: `priv` is a macro"),
            "{backend}: {message}"
        );
    }
}

/// Two expansions of a macro are given one spelling for its private name
/// (the spelling is a function of the macro's environment, the name and
/// the mention), so the spelling cannot keep them apart: here the outer
/// expansion's `let` binds `helper` and the inner expansion, nested in its
/// body, mentions the library's `helper` free. The renamed occurrences keep
/// their scopes, which do keep them apart. chibi 0.12 and Gauche 0.9.15 both
/// answer `(lib (lib 0))` and `(lib (lib (lib 0)))`, measured 2026-10-08;
/// with the spelling alone, both backends answered `(lib (local 0))`.
#[test]
fn nested_expansions_sharing_a_spelling_do_not_capture_each_other() {
    let program = "(define-library (t n)
  (export m)
  (import (scheme base))
  (begin
    (define helper 'lib)
    (define-syntax m
      (syntax-rules ()
        ((_ e) (list helper (let ((helper 'local)) e)))))))
(import (scheme base) (t n))
(list (m (m 0)) (m (m (m 0))))";
    let expected = "((lib (lib 0)) (lib (lib (lib 0))))";
    assert_eq!(common::eval_program_vm(program), expected, "VM");
    assert_eq!(
        common::eval_program_tree_walker(program),
        expected,
        "tree-walker"
    );
}

// ─── Early and relinked references across forms (#837) ──────────────────────
//
// 4b.10 replaced the spelling an early-bound template reference was renamed
// to (`f.0`, an import alias) with a link to the binding's record, and its
// second PR does the same for relinked references. These pin what programs
// answer through both, before and after each step.

/// Run `program` on each backend: `[VM, tree-walker]`.
fn on_both_backends(program: &str) -> [String; 2] {
    [
        common::eval_program_vm(program),
        common::eval_program_tree_walker(program),
    ]
}

/// #837's `def-getter-maker`: a library macro defines a macro in the
/// program whose template names the library's private `secret`. The
/// generated macro is expanded two collections and a program definition of
/// `secret` later, in another form, and still reads the library's binding,
/// which `bump!` has assigned since. Run under the stress lane and in a
/// release `gc-check` build too, where a freed record or cell panics.
#[test]
fn a_generated_macros_private_reference_survives_a_collection_and_a_program_definition() {
    let program = "(define-library (t getter)
  (export def-getter-maker bump!)
  (import (scheme base))
  (begin
    (define secret 42)
    (define (bump!) (set! secret (+ secret 1)))
    (define-syntax def-getter-maker
      (syntax-rules ()
        ((_ name) (define-syntax name (syntax-rules () ((_) secret))))))))
(import (scheme base) (patina debug) (t getter))
(def-getter-maker get)
(gc) (gc)
(define secret 'program)
(bump!)
(list (get) secret)";
    let expected = "(43 program)".to_string();
    assert_eq!(
        on_both_backends(program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// Three levels: a library macro generates a macro that generates a macro
/// whose template names the library's private `secret`. The innermost is
/// made in a later form than the middle one, after the program has defined
/// a `secret` of its own, and again inside a body; each reads the library's
/// binding, at top level, in a procedure under a local `secret`, and in the
/// body. chibi 0.12 (the library as a file) and Gauche 0.9.15 answer the
/// same (2026-11-16). One of #837's scoreboard rows, which the suite file
/// `expansion/relinked-names.scm` holds the rest of: it passed before the
/// second PR and must after.
#[test]
fn a_macro_generated_through_three_levels_reaches_the_private_variable() {
    let program = "(define-library (t nest)
  (export gen3 bump!)
  (import (scheme base))
  (begin
    (define secret 42)
    (define (bump!) (set! secret (+ secret 1)))
    (define-syntax gen3
      (syntax-rules ()
        ((_ g2)
         (define-syntax g2
           (syntax-rules ()
             ((_ g1)
              (define-syntax g1
                (syntax-rules () ((_) secret)))))))))))
(import (scheme base) (t nest))
(gen3 mid)
(define secret 'program)
(mid inner)
(bump!)
(define first (list (inner) secret))
(define (later) (let ((secret 'local)) (inner)))
(bump!)
(list first (later) (let () (mid local-inner) (local-inner)))";
    let expected = "((43 program) 44 44)".to_string();
    assert_eq!(
        on_both_backends(program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// Two libraries: `(t genb)` runs `(t gena)`'s generator and exports the
/// macro it made, and has a private `secret` of its own. The exported
/// macro reads the first library's `secret`, from a third root that
/// defines another, before and after the first library assigns it, as a
/// macro the program makes with the same generator does. chibi 0.12 (the
/// libraries as files) and Gauche 0.9.15 answer the same (2026-11-16). A
/// scoreboard row of #837's second PR.
#[test]
fn a_macro_generated_across_two_libraries_reaches_the_first() {
    let program = "(define-library (t gena)
  (export mk bump!)
  (import (scheme base))
  (begin
    (define secret 42)
    (define (bump!) (set! secret (+ secret 1)))
    (define-syntax mk
      (syntax-rules ()
        ((_ name) (define-syntax name (syntax-rules () ((_) secret))))))))
(define-library (t genb)
  (export get-b)
  (import (scheme base) (t gena))
  (begin
    (define secret 'genb-own)
    (mk get-b)))
(import (scheme base) (t genb) (t gena))
(define secret 'program)
(define first (list (get-b) secret))
(bump!)
(mk get-p)
(list first (list (get-b) (get-p)))";
    let expected = "((42 program) (43 43))".to_string();
    assert_eq!(
        on_both_backends(program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
    assert_eq!(
        [
            a_middle_library_defining_the_carried_spelling(common::vm_interpreter()),
            a_middle_library_defining_the_carried_spelling(common::tree_walker_interpreter()),
        ],
        ["(42 middle)", "(42 middle)"],
        "[VM, tree-walker]"
    );
}

/// The same where the middle library itself defines the spelling the
/// generated macro carries. `get-b` is made in `(t genb2)` by `(t gena2)`'s
/// generator, so its template's `secret` was relinked there and the macro
/// carries the identifier with `(t gena2)`'s record. `(t genb2)` also
/// defines a variable under that very spelling, which the test reads from
/// an expansion and writes into the library's text. Expanded in the
/// program, `get-b` reads `(t gena2)`'s 42: the carried identifier means
/// its record, and its spelling being a name of the macro's own
/// environment is a definition of that library's, which `get-own` reads.
/// Relinked by that name it would read `middle`.
fn a_middle_library_defining_the_carried_spelling<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> String
where
    B::Error: std::fmt::Display,
{
    interp
        .eval_program_owned(
            "(define-library (t gena2)
  (export mk spell)
  (import (scheme base))
  (begin
    (define secret 42)
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax spell (syntax-rules () ((_) (quote-it secret))))
    (define-syntax mk
      (syntax-rules ()
        ((_ name) (define-syntax name (syntax-rules () ((_) secret))))))))
(import (scheme base) (t gena2))",
        )
        .expect("the first library");
    let spelling = answer_or_error(&interp, "(spell)").expect("spells");
    assert!(spelling.starts_with("secret") && spelling != "secret");
    interp
        .eval_program_owned(&format!(
            "(define-library (t genb2)
  (export get-b get-own)
  (import (scheme base) (t gena2))
  (begin
    (define {spelling} 'middle)
    (define (get-own) {spelling})
    (mk get-b)))
(import (t genb2))"
        ))
        .expect("the middle library");
    answer_or_error(&interp, "(list (get-b) (get-own))").expect("reads")
}

/// A redefinition: `get-old`, a macro the first `(t redef)`'s generator
/// made, and `h-old`, a procedure that expanded its `m`, each name the
/// first library's private `secret`, and still read it after the library
/// is redefined, imported again and two collections run, while a macro
/// made then and a fresh expansion of `m` read the new one's, which the
/// new `bump!` has assigned. Neither oracle redefines a library so (Gauche
/// 0.9.15 keeps the one module and answers the new library's value
/// throughout, `new` and then `(bumped new)` for all four; chibi 0.12
/// takes no `define-library` in a script), so it is a Rust test and not a
/// suite row. A scoreboard row of #837's second PR, run under the stress
/// lane and in a release `gc-check` build too, where a freed record or
/// cell panics.
#[test]
fn generated_macros_keep_the_library_they_were_made_from_across_a_redefinition() {
    let library = |value: &str| {
        format!(
            "(define-library (t redef-gen)
  (export mk m bump!)
  (import (scheme base))
  (begin
    (define secret '{value})
    (define (bump!) (set! secret (list 'bumped secret)))
    (define-syntax m (syntax-rules () ((_) secret)))
    (define-syntax mk
      (syntax-rules ()
        ((_ name) (define-syntax name (syntax-rules () ((_) secret))))))))
"
        )
    };
    let program = format!(
        "{}(import (scheme base) (patina debug) (t redef-gen))
(mk get-old)
(define (h-old) (m))
{}(define before (list (get-old) (h-old) (m)))
(import (t redef-gen))
(gc) (gc)
(mk get-new)
(bump!)
(list before (list (get-old) (h-old) (get-new) (m)))",
        library("old"),
        library("new")
    );
    let expected = "((old old old) (old old (bumped new) (bumped new)))".to_string();
    assert_eq!(
        on_both_backends(&program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// A definer of introduced state, across forms and collections: each run
/// of `(t intro)`'s `def-counter` introduces a `count` of its own beside
/// the library's plain `count`, and the getter and bumper it generates
/// name that one. The program has a `count` too. A procedure compiled
/// before the program's definition, and procedures compiled after it and
/// after collections, read and assign each run's own state; a run of the
/// definer in the program has another. Every write and the reads after it
/// are sequenced. chibi 0.12 (the library as a file) and Gauche 0.9.15
/// answer the same without the collections (2026-11-17). A scoreboard row
/// of #837's second PR, the second target that holds a relinked reference
/// to an introduced definition (`expansion/relinked-names.scm` has the
/// first): it passed before the switch and must after.
#[test]
fn a_definers_introduced_state_is_read_and_assigned_from_procedures_compiled_later() {
    let program = "(define-library (t intro)
  (export get1 bump1! get2 bump2! plain-count def-counter)
  (import (scheme base))
  (begin
    (define count 100)
    (define (plain-count) count)
    (define-syntax def-counter
      (syntax-rules ()
        ((_ getter bumper)
         (begin
           (define count 0)
           (define-syntax getter (syntax-rules () ((_) count)))
           (define-syntax bumper (syntax-rules () ((_) (set! count (+ count 1)))))))))
    (def-counter get1 bump1!)
    (def-counter get2 bump2!)))
(import (scheme base) (patina debug) (t intro))
(define (early) (bump1!) (get1))
(define count 'program)
(gc) (gc)
(define (later) (bump2!) (list (get1) (get2)))
(gc)
(define first (let* ((a (early)) (b (later))) (list a b)))
(def-counter get3 bump3!)
(gc) (gc)
(define (later3) (bump3!) (bump1!) (list (get1) (get2) (get3) (plain-count) count))
(let* ((c (later3)) (d (early))) (list first c d))";
    let expected = "((1 (1 1)) (2 1 1 100 program) 3)".to_string();
    assert_eq!(
        on_both_backends(program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// Private keywords of a splice, across forms and collections: `(t
/// splice)` keeps `helper` under a `splicing-let-syntax` and the recursive
/// `step` under a `splicing-letrec-syntax`, each bound at the splice's
/// scopes in the library's namespace and named by nothing a program can
/// write. Its own macro `answer` and the macros its generators make in the
/// program expand them, in the form after two collections and in a
/// procedure under a `let-syntax` of the spelling, past the program's own
/// `helper`. chibi 0.12 answers the same with the library as a file and
/// without the collections (2026-11-17); Gauche 0.9.15 has no `(srfi
/// 188)`. A scoreboard row of #837's second PR, the second target that
/// holds a relinked scoped keyword (`expansion/splicing-libraries.scm` has
/// the first): it passed before the switch and must after.
#[test]
fn a_splices_private_keywords_are_expanded_from_later_forms() {
    let program = "(define-library (t splice)
  (import (scheme base) (srfi 188))
  (export answer make-answer make-countdown)
  (begin
    (splicing-let-syntax ((helper (syntax-rules () ((_ x) (list 'helped x)))))
      (define-syntax answer (syntax-rules () ((_ x) (helper x))))
      (define-syntax make-answer
        (syntax-rules ()
          ((_ name) (define-syntax name (syntax-rules () ((_ x) (helper x))))))))
    (splicing-letrec-syntax
        ((step (syntax-rules () ((_ 0) 70) ((_ n) (step 0)))))
      (define-syntax make-countdown
        (syntax-rules ()
          ((_ name) (define-syntax name (syntax-rules () ((_) (step 1))))))))))
(import (scheme base) (patina debug) (t splice))
(define-syntax helper (syntax-rules () ((_ x) 'the-programs)))
(make-answer generated)
(make-countdown countdown)
(gc) (gc)
(define first (list (answer 1) (generated 2) (countdown) (helper 3)))
(define (later)
  (let-syntax ((step (syntax-rules () ((_ n) 'local))))
    (list (answer 4) (generated 5) (countdown) (step 6))))
(gc)
(list first (later))";
    let expected =
        "(((helped 1) (helped 2) 70 the-programs) ((helped 4) (helped 5) 70 local))".to_string();
    assert_eq!(
        on_both_backends(program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// The library `call-f` comes from: an exported `f` its template calls.
const CALL_F: &str = "(define-library (t lib-f)
  (export call-f f)
  (import (scheme base))
  (begin
    (define (f) 'lib-f)
    (define-syntax call-f (syntax-rules () ((_) (f))))))
";

/// A use-site binding of the template reference's spelling never captures
/// it (Larceny triage family 36): inside `(let ((f …)) (call-f))` the
/// expansion's `f` is the library's, at top level and inside a procedure.
#[test]
fn a_template_reference_reads_the_library_inside_a_frame_binding_of_its_spelling() {
    let program = format!(
        "{CALL_F}(import (scheme base) (t lib-f))
(define (h) (let ((f (lambda () 'local))) (call-f)))
(list (let ((f 'local)) (call-f)) (h))"
    );
    let expected = "(lib-f lib-f)".to_string();
    assert_eq!(
        on_both_backends(&program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// An unquoted expression of a template is ordinary code, early-bound like
/// the rest (#445): `h`, compiled before the program defines `f`, still
/// calls the library's. It guards the desugarer's emission of an unquoted
/// expression and the quasiquote lowering's copy of it (`quasiquote_lower`):
/// either dropping what the reference was bound to would bring back #438's
/// capture there. Settling does not walk this form, which introduces no
/// definition; the next test's does.
#[test]
fn an_unquoted_template_reference_stays_linked() {
    let program = "(define-library (t lib-q)
  (export quasi-f f)
  (import (scheme base))
  (begin
    (define (f) 'lib-f)
    (define-syntax quasi-f (syntax-rules () ((_) `(,(f)))))))
(import (scheme base) (t lib-q))
(define (h) (quasi-f))
(define (f) 'mine)
(list (h) (f))";
    let expected = "((lib-f) mine)".to_string();
    assert_eq!(
        on_both_backends(program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// The same, in a form whose expansion also introduces a definition
/// (`tmp`) and binds `f` early, so that settling rebuilds it node by node
/// (`Desugarer::settle_early_bindings`), the quasiquote's unquoted parts
/// through `QuasiTemplate::map_unquoted`: a rebuild there that dropped
/// what the reference was bound to would bring back #438's capture.
#[test]
fn an_unquoted_template_reference_in_a_form_settling_rebuilds_stays_linked() {
    let program = "(define-library (t lib-qd)
  (export quasi-def f)
  (import (scheme base))
  (begin
    (define (f) 'lib-f)
    (define-syntax quasi-def
      (syntax-rules ()
        ((_ name) (begin (define tmp 0) (define (name) `(,(f) ,tmp))))))))
(import (scheme base) (t lib-qd))
(quasi-def h)
(define (f) 'mine)
(list (h) (f))";
    let expected = "((lib-f 0) mine)".to_string();
    assert_eq!(
        on_both_backends(program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// A procedure that expanded a library macro keeps the binding it was
/// expanded against when the library is redefined and imported again and
/// two collections run (4b's start decision 12): `h` answers the old `f`,
/// a fresh expansion the new one. Run under the stress lane and in a
/// release `gc-check` build too, where a freed record or cell panics.
#[test]
fn an_early_link_keeps_the_old_library_binding_across_a_redefinition() {
    let library = |answer: &str| {
        format!(
            "(define-library (t redef)
  (export m f)
  (import (scheme base))
  (begin
    (define (f) '{answer})
    (define-syntax m (syntax-rules () ((_ x) (list (f) x))))))
"
        )
    };
    let program = format!(
        "{}(import (scheme base) (patina debug) (t redef))
(define (h) (m 1))
{}(import (t redef))
(gc) (gc)
(list (h) (m 2) (f))",
        library("old-f"),
        library("new-f")
    );
    let expected = "((old-f 1) (new-f 2) new-f)".to_string();
    assert_eq!(
        on_both_backends(&program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

// ─── The same four, for a relinked reference (#837) ─────────────────────────
//
// The tests above are of a template reference to a name both sides import,
// which is bound early. These are their twins for a reference to a name
// private to the library, which is relinked: the identifier is renamed in
// the expansion and means its record in the form's table. Each passed
// before the switch too, through the alias.

/// The library `call-f` and its kin come from here: `f` is private.
const PRIVATE_F: &str = "(define-library (t priv-f)
  (export call-f quasi-f quasi-def)
  (import (scheme base))
  (begin
    (define (f) 'lib-f)
    (define-syntax call-f (syntax-rules () ((_) (f))))
    (define-syntax quasi-f (syntax-rules () ((_) `(,(f)))))
    (define-syntax quasi-def
      (syntax-rules ()
        ((_ name) (begin (define tmp 0) (define (name) `(,(f) ,tmp))))))))
(import (scheme base) (t priv-f))
(define (f) 'mine)
";

/// A use-site binding of the relinked reference's written spelling never
/// captures it: inside `(let ((f …)) (call-f))` the expansion's `f` is the
/// library's, at top level and inside a procedure.
#[test]
fn a_relinked_reference_reads_the_library_inside_a_frame_binding_of_its_spelling() {
    let program = format!(
        "{PRIVATE_F}(define (h) (let ((f (lambda () 'local))) (call-f)))
(list (let ((f 'local)) (call-f)) (h))"
    );
    let expected = "(lib-f lib-f)".to_string();
    assert_eq!(
        on_both_backends(&program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// An unquoted expression of a template is ordinary code, relinked like
/// the rest: the desugarer's emission of an unquoted expression and the
/// quasiquote lowering's copy of it both keep the link.
#[test]
fn an_unquoted_relinked_reference_stays_linked() {
    let program = format!("{PRIVATE_F}(define (h) (quasi-f))\n(list (h) (f))");
    let expected = "((lib-f) mine)".to_string();
    assert_eq!(
        on_both_backends(&program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// The same, in a form whose expansion also introduces a definition
/// (`tmp`), so that settling walks it and rebuilds it node by node: the
/// relinked reference in the unquote, which is no reference to a
/// definition the form introduced, keeps its link through the rebuild.
#[test]
fn an_unquoted_relinked_reference_in_a_form_settling_rebuilds_stays_linked() {
    let program = format!("{PRIVATE_F}(quasi-def h)\n(list (h) (f))");
    let expected = "((lib-f 0) mine)".to_string();
    assert_eq!(
        on_both_backends(&program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// A procedure that expanded a library macro over a private procedure
/// keeps the record it was relinked to when the library is redefined and
/// imported again and two collections run: `h` answers the old `f`, a
/// fresh expansion the new one. Under the stress lane and in a release
/// `gc-check` build a freed record or cell panics.
#[test]
fn a_relinked_link_keeps_the_old_library_binding_across_a_redefinition() {
    let library = |answer: &str| {
        format!(
            "(define-library (t redef-priv)
  (export m)
  (import (scheme base))
  (begin
    (define (f) '{answer})
    (define-syntax m (syntax-rules () ((_ x) (list (f) x))))))
"
        )
    };
    let program = format!(
        "{}(import (scheme base) (patina debug) (t redef-priv))
(define (h) (m 1))
{}(import (t redef-priv))
(gc) (gc)
(list (h) (m 2))",
        library("old-f"),
        library("new-f")
    );
    let expected = "((old-f 1) (new-f 2))".to_string();
    assert_eq!(
        on_both_backends(&program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// A library with a private `settled`, a getter of it, and macros that
/// expand to a definition of that spelling beside references to it.
const SETTLE: &str = "(define-library (t settle)
  (export beside beside-two get-settled m-produced)
  (import (scheme base))
  (begin
    (define settled 42)
    (define-syntax get-settled (syntax-rules () ((_) settled)))
    (define-syntax beside
      (syntax-rules ()
        ((_ v) (begin (define settled 'introduced) (define v settled)))))
    (define-syntax beside-two
      (syntax-rules ()
        ((_ v) (begin (define settled 'introduced)
                      (define v (list settled (get-settled) settled))))))
    (define (ev? n) 'lib-ev)
    (define (od? n) 'lib-od)
    (define-syntax m-produced
      (syntax-rules ()
        ((_ x)
         (begin
           (define (ev? n) (if (= n 0) 'even (od? (- n 1))))
           (define (od? n) (if (= n 0) 'odd (ev? (- n 1))))
           (ev? x)))))))
(import (scheme base) (t settle))
";

/// A relinked reference beside a definition its own expansion produces
/// into a body means that definition. When the body is desugared nothing
/// selects the reference (the body's scan drops the names an expansion
/// introduced), so the form's table answers it and it is emitted as a link
/// to the *library's* binding; settling takes the link back at the form's
/// end, where the form is known to have introduced a definition of that
/// spelling at scopes the reference carries, and gives the node the
/// spelling the definition has. chibi 0.12 and Gauche 0.9.15 answer each
/// program the same.
///
/// - beside a reference, with the library's own getter after it;
/// - a forward reference between two produced procedures, in a `let` body
///   and in a procedure's;
/// - in a `lambda` body;
/// - two expansions' mentions of one written name in one produced form:
///   `beside-two`'s own `settled`, twice, which are the definition's, and
///   the `settled` its use of `get-settled` expands to, which is given the
///   same spelling by another expansion and is the library's. Settling
///   tells them apart by the scopes each carries.
#[test]
fn a_relinked_reference_beside_a_definition_its_expansion_produces_into_a_body() {
    for (program, expected) in [
        (
            "(define (f) (beside w) (list w (get-settled)))\n(f)",
            "(introduced 42)",
        ),
        (
            "(define (g) (m-produced 4))\n(list (let () (m-produced 5)) (g))",
            "(odd even)",
        ),
        ("((lambda (k) (m-produced k)) 3)", "odd"),
        (
            "(define (f) (beside-two w) (list w (get-settled)))\n(f)",
            "((introduced 42 introduced) 42)",
        ),
    ] {
        let program = format!("{SETTLE}{program}");
        assert_eq!(
            on_both_backends(&program),
            [expected.to_string(), expected.to_string()],
            "[VM, tree-walker] {program}"
        );
    }
}

/// The same templates at top level, where settling has nothing to do: the
/// produced definition is declared at its scopes before the reference
/// beside it is resolved, so the reference is selected by its scopes and
/// the table is never asked.
#[test]
fn a_relinked_reference_beside_a_definition_its_expansion_produces_at_top_level() {
    for (program, expected) in [
        ("(beside v)\n(list v (get-settled))", "(introduced 42)"),
        (
            "(beside-two v)\n(list v (get-settled))",
            "((introduced 42 introduced) 42)",
        ),
    ] {
        let program = format!("{SETTLE}{program}");
        assert_eq!(
            on_both_backends(&program),
            [expected.to_string(), expected.to_string()],
            "[VM, tree-walker] {program}"
        );
    }
}

/// A template's reference to its library's private name, and an
/// assignment to it, where its own expansion produces a definition of that
/// name into a body and runs them before the definition: each means the
/// body's definition, not yet run, and the library's binding is untouched.
/// The VM reads and writes the body's variable before its definition has
/// run, `#<unspecified>`, as chibi 0.12 answers `#<undef>`; the tree-walker
/// raises its own undefined-variable error for each, as it does for a body
/// nothing expanded (guarded here). Gauche 0.9.15 answers `(1
/// written-by-body lib-hidden)`. Before #837's switch the tree-walker
/// reached the library's binding by the alias's name: the read answered
/// `lib-hidden`, and the assignment wrote the library's variable, which
/// then read `written-by-body`.
#[test]
fn an_expansions_own_definition_reached_before_it_runs_is_not_the_librarys() {
    let program = "(define-library (t early)
  (export read-before set-before lib-hidden)
  (import (scheme base))
  (begin
    (define hidden 'lib-hidden)
    (define (lib-hidden) hidden)
    (define-syntax read-before
      (syntax-rules ()
        ((_ v) (begin (define (get) hidden) (define x (get)) (define hidden v) x))))
    (define-syntax set-before
      (syntax-rules ()
        ((_ v)
         (begin (define (put!) (set! hidden v)) (define x (put!)) (define hidden v) x))))))
(import (scheme base) (t early))
(define (f) (read-before 1))
(define (g) (set-before 'written-by-body))
(let* ((read (guard (e (#t 'error)) (f)))
       (assigned (guard (e (#t 'error)) (g)))
       (library (lib-hidden)))
  (list read assigned library))";
    assert_eq!(
        on_both_backends(program),
        [
            "(#<unspecified> #<unspecified> lib-hidden)".to_string(),
            "(error error lib-hidden)".to_string()
        ],
        "[VM, tree-walker]"
    );
}

/// A library definer whose template defines its library's private name,
/// run twice where a definition is an expression (a `when` at top level),
/// keeps two bindings, and the spelling relinking gave the name is no name
/// to the program. Gauche 0.9.15 answers as asserted; chibi 0.12 refuses a
/// definition in an expression context. Before #837's switch the VM
/// answered `(a b lib-hidden …)` too: each run defined the alias it was
/// given by name, and the alias was minted again after each definition.
/// The switch gives both runs one spelling, which the VM bound by name in
/// that position until #884 bound such a definition at its scopes: the
/// switch's commits alone answered `(b b lib-hidden b)` on the VM.
#[test]
fn a_definer_run_twice_in_an_expression_keeps_two_bindings() {
    let program = "(define-library (t pair)
  (export def-pair lib-hidden spell-hidden)
  (import (scheme base))
  (begin
    (define hidden 'lib-hidden)
    (define (lib-hidden) hidden)
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax spell-hidden (syntax-rules () ((_) (quote-it hidden))))
    (define-syntax def-pair
      (syntax-rules () ((_ getter v) (begin (define hidden v) (define (getter) hidden)))))))
(import (scheme base) (scheme eval) (scheme repl) (t pair))
(when #t (def-pair get-a 'a))
(when #t (def-pair get-b 'b))
(list (get-a) (get-b) (lib-hidden)
      (guard (e (#t 'no-such-name)) (eval (spell-hidden) (interaction-environment))))";
    let expected = "(a b lib-hidden no-such-name)";
    assert_eq!(common::eval_program_vm(program), expected, "VM");
    assert_eq!(
        common::eval_program_tree_walker(program),
        expected,
        "tree-walker"
    );
}

/// A library whose macro expands to a body headed by a private definer,
/// a keyword the program cannot name and has a procedure of its own
/// spelled like.
const SCAN: &str = "(define-library (t scan)
  (export with-private get-shown spell-definer)
  (import (scheme base))
  (begin
    (define shown 'lib-shown)
    (define-syntax get-shown (syntax-rules () ((_) shown)))
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax spell-definer (syntax-rules () ((_) (quote-it def-private))))
    (define-syntax def-private
      (syntax-rules () ((_ name value) (define name value))))
    (define-syntax with-private
      (syntax-rules ()
        ((_ name value body)
         (let () (def-private name value) body))))))
(import (scheme base) (t scan))
(define (shown) 'the-programs)
(define (def-private . args) 'the-programs)
";

/// A body is scanned for the definitions its macro uses produce before it
/// is desugared, and the scan expands those uses. Here the body is one a
/// library macro's expansion wrote, and its first form is headed by a
/// keyword private to the library, which that expansion relinked: the
/// scan reads the enclosing form's table beneath its own, finds the
/// keyword, expands it and sees the definition, so the body's reference
/// to the defined name is bound to it. At top level and in a procedure.
#[test]
fn a_body_scan_sees_the_enclosing_expansions_relinked_keywords() {
    let program = format!(
        "{SCAN}(list (with-private x 1 (list x (get-shown)))
      ((lambda () (with-private y 2 (+ y 1))))
      (def-private 1 2))"
    );
    let expected = "((1 lib-shown) 3 the-programs)".to_string();
    assert_eq!(
        on_both_backends(&program),
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// What a body scan's expansions relink goes in a table of the scan's own
/// and is dropped with it: after forms whose bodies were scanned, the
/// spelling the private definer was given is no name of the program's, and
/// the program's names and records are what they were. Until the switch
/// the scan's relinking installed its aliases in a frame of its own, which
/// it dropped, and the real desugar's in the program's namespace, which
/// kept them.
#[test]
fn a_body_scan_leaves_no_relinked_name_behind() {
    fn check<B: patina_runtime::Backend>(interp: patina_interpreter::Interpreter<B>, backend: &str)
    where
        B::Error: std::fmt::Display,
    {
        interp.eval_program_owned(SCAN).expect("the library");
        let before = names_and_records(&interp);
        let answer = answer_or_error(
            &interp,
            "(list (with-private x 1 x) ((lambda () (with-private y 2 (+ y 1)))))",
        )
        .expect("the bodies");
        assert_eq!(answer, "(1 3)", "{backend}");
        assert_eq!(names_and_records(&interp), before, "{backend}");
        let spelling = answer_or_error(&interp, "(spell-definer)").expect("spells");
        assert!(
            spelling.starts_with("def-private") && spelling != "def-private",
            "{backend}: {spelling}"
        );
        assert!(!is_a_name(&interp, &spelling), "{backend}: {spelling}");
    }
    check(common::vm_interpreter(), "VM");
    check(common::tree_walker_interpreter(), "tree-walker");
}

// ─── The switch: an early-bound reference links, and mints nothing (#837) ───
//
// Until 4b.10 an early-bound template reference was renamed to an alias,
// `f.N`, a second record over the import's cell in a slot of the use
// site's namespace: a name a program could read, and define over to change
// code compiled before it. It links the binding's record now and leaves no
// name behind (#837's question 3), as chibi 0.12 and Gauche 0.9.15 leave
// none. Each test below failed before the switch.

/// `CALL_F` imported, then a procedure and a bare form that expand
/// `call-f`, each binding its `f` early.
fn expand_call_f<B: patina_runtime::Backend>(interp: &patina_interpreter::Interpreter<B>) {
    interp
        .eval_program_owned(&format!(
            "{CALL_F}(import (scheme base) (t lib-f))\n(define (h) (call-f))\n(call-f)\n(h)"
        ))
        .expect("the library, the import and the expansions");
}

/// Every `f.k`, `k` below 100,000, that the use site's namespace answers by
/// name or holds a slot for.
fn spellings_of_f<B: patina_runtime::Backend>(
    interp: &patina_interpreter::Interpreter<B>,
) -> Vec<String> {
    let env = interp.global_env();
    let heap = interp.backend().heap();
    (0..100_000)
        .map(|k| format!("f.{k}"))
        .filter(|spelling| env.get(heap, spelling).is_some() || env.local_slot(spelling).is_some())
        .collect()
}

/// After the expansions, no `f.k` is a name of the program's namespace: no
/// slot, and nothing `get` answers. The spelling's number comes from one
/// counter every alias in the process shares, so the scan is wide rather
/// than a guess at it. Before the switch the alias was there, on both
/// backends.
#[test]
fn an_early_bound_reference_mints_no_spelling() {
    let vm = common::vm_interpreter();
    expand_call_f(&vm);
    let tree_walker = common::tree_walker_interpreter();
    expand_call_f(&tree_walker);
    assert_eq!(
        [spellings_of_f(&vm), spellings_of_f(&tree_walker)],
        [Vec::<String>::new(), Vec::new()],
        "[VM, tree-walker]"
    );
}

/// What the program's namespace lists and counts: its names three ways, its
/// placeholders, and the binding records the live namespaces hold.
type NamesAndRecords = (
    Vec<String>,
    Vec<(String, patina_core::TaggedValue)>,
    Vec<String>,
    Vec<String>,
    usize,
);

fn names_and_records<B: patina_runtime::Backend>(
    interp: &patina_interpreter::Interpreter<B>,
) -> NamesAndRecords {
    let env = interp.global_env();
    let heap = interp.backend().heap();
    (
        env.local_names(heap),
        env.bindings(heap),
        env.get_all_names(heap),
        env.weak_entry_names_for_tests(),
        heap.borrow().binding_records(),
    )
}

/// An early-bound expansion leaves the program's names as they were: the
/// namespace's `local_names`, `bindings` and `get_all_names`, its
/// placeholders, and the count of binding records, before and after a form
/// that expands `call-f` and defines nothing. The enumerations hid the alias
/// before the switch (`visible_slots`' filter, deleted with it), so what
/// failed there is the count: the alias was a record more, in a slot.
fn an_expansion_leaves<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> (NamesAndRecords, NamesAndRecords) {
    interp
        .eval_program_owned(&format!("{CALL_F}(import (scheme base) (t lib-f))"))
        .expect("the library and the import");
    let before = names_and_records(&interp);
    interp
        .eval_program_owned("(call-f) (list (call-f))")
        .expect("the expansions");
    (before, names_and_records(&interp))
}

#[test]
fn an_early_bound_expansion_leaves_the_programs_names_alone() {
    let (vm_before, vm_after) = an_expansion_leaves(common::vm_interpreter());
    let (tw_before, tw_after) = an_expansion_leaves(common::tree_walker_interpreter());
    assert_eq!(
        [vm_after, tw_after],
        [vm_before, tw_before],
        "[VM, tree-walker]"
    );
}

/// A program's definition of a spelling an alias once had leaves code
/// compiled before it alone (§9.1): `h`, compiled before, still calls the
/// library's `f`. The definitions cover each spelling the namespace answers
/// (before the switch, the alias `h` was compiled against) and `f.0` to
/// `f.9`; before the switch the definition re-pointed the alias's record
/// under `h`, and `h` answered `mine`, where chibi 0.12 and Gauche 0.9.15
/// answer `lib-f`. The suite file's last two rows
/// (`expansion/template-references.scm`) are the same in Scheme.
fn defining_a_minted_spelling<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> String {
    expand_call_f(&interp);
    let spellings: Vec<String> = spellings_of_f(&interp)
        .into_iter()
        .chain((0..10).map(|k| format!("f.{k}")))
        .collect();
    for spelling in &spellings {
        interp
            .eval_program_owned(&format!("(define ({spelling}) 'mine)"))
            .expect("a definition");
    }
    let answer = interp.eval_str_owned("(h)").expect("calls h");
    interp.display_tagged(&answer)
}

#[test]
fn a_minted_spelling_defined_by_the_program_leaves_earlier_code_alone() {
    assert_eq!(
        [
            defining_a_minted_spelling(common::vm_interpreter()),
            defining_a_minted_spelling(common::tree_walker_interpreter()),
        ],
        ["lib-f", "lib-f"],
        "[VM, tree-walker]"
    );
}

// ─── A relinked keyword leaves nothing behind (#837, question 4 (a)) ────────
//
// A library macro whose template names a private macro (`priv`) relinks it:
// the identifier travels with its target's record in the form's table, and
// in a generated macro's relinks. Until #837's second PR the expansion left
// an alias in the program, `priv.N`, over the private keyword's record,
// which kept the keyword's macro and so the library's namespace for good.
// Nothing is installed now, and each test below failed before the switch.

/// A library whose exported `m` expands to its private macro `priv`, and
/// whose `spell-priv` tells the spelling a template's `priv` has in an
/// expansion (a relinked identifier used as data keeps it).
const KEYWORD_LIB: &str = "(define-library (t kw)
  (export m spell-priv)
  (import (scheme base))
  (begin
    (define-syntax priv (syntax-rules () ((_ x) (+ x 1))))
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax spell-priv (syntax-rules () ((_) (quote-it priv))))
    (define-syntax m (syntax-rules () ((_ x) (priv x))))))
";

/// Whether `spelling` is a name of the program's namespace: something
/// `get` answers, or a slot.
fn is_a_name<B: patina_runtime::Backend>(
    interp: &patina_interpreter::Interpreter<B>,
    spelling: &str,
) -> bool {
    let env = interp.global_env();
    env.get(interp.backend().heap(), spelling).is_some() || env.local_slot(spelling).is_some()
}

/// The spelling relinking gives `(t kw)`'s `priv`, read from an expansion.
fn spelling_of_priv<B: patina_runtime::Backend>(
    interp: &patina_interpreter::Interpreter<B>,
) -> String {
    let spelling = interp.display_tagged(interp.eval_str_owned("(spell-priv)").expect("spells"));
    assert!(
        spelling.starts_with("priv") && spelling != "priv",
        "the spelling relinking gave `priv`, read from an expansion: {spelling}"
    );
    spelling
}

/// `(m 1)`'s answer, whether the spelling its `priv` was given is a name of
/// the program's afterwards, and the program's names and records before the
/// expansion and after it.
fn a_relinked_keyword_after_an_expansion<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> (String, bool, NamesAndRecords, NamesAndRecords) {
    interp
        .eval_program_owned(&format!("{KEYWORD_LIB}(import (scheme base) (t kw))"))
        .expect("the library and the import");
    let before = names_and_records(&interp);
    let answer = interp.eval_program_owned("(m 1)").expect("the expansion");
    let answer = interp.display_tagged(&answer);
    let after = names_and_records(&interp);
    let spelling = spelling_of_priv(&interp);
    (answer, is_a_name(&interp, &spelling), before, after)
}

/// An expansion of `m` leaves no alias for `priv` in the program: the
/// spelling the expansion gave it is nothing `get` answers and no slot, and
/// the namespace's names, its placeholders and the count of binding records
/// are what they were before the expansion. Before the switch the spelling
/// was an alias, which `get` answered with the private macro.
#[test]
fn a_relinked_keyword_leaves_no_alias() {
    for (backend, (answer, named, before, after)) in [
        (
            "VM",
            a_relinked_keyword_after_an_expansion(common::vm_interpreter()),
        ),
        (
            "tree-walker",
            a_relinked_keyword_after_an_expansion(common::tree_walker_interpreter()),
        ),
    ] {
        assert_eq!(answer, "2", "{backend}");
        assert!(!named, "{backend}: the relinked spelling is a name");
        assert_eq!(after, before, "{backend}: the program's names and records");
    }
}

/// Whether the library namespace `m` was defined in is alive after the
/// library `lib` (`(t kw)`) is redefined and imported again and two
/// collections run, with `(m 1)` expanded before the redefinition or not,
/// and whether the spelling that expansion gave `priv` is a name of the
/// program's then. `setup` is what the first program defines before `lib`,
/// and is not redefined. The namespace is watched through `m`'s macro,
/// whose definition environment it is, by a `Weak` taken before the
/// redefinition.
///
/// The expansion, the redefinition and the reading are separate top-level
/// forms: a form's pool roots the records it pooled, a relinked keyword's
/// among them, until the form is loaded (the VM) or has run (the
/// tree-walker), so a reading inside the expanding form would say nothing
/// of what is kept afterwards.
fn the_old_library_namespace_after_a_redefinition<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
    setup: &str,
    lib: &str,
    expand: bool,
) -> (bool, bool) {
    interp
        .eval_program_owned(&format!(
            "{setup}{lib}(import (scheme base) (patina debug) (t kw))\n{}",
            if expand { "(m 1)" } else { "" }
        ))
        .expect("the library and the import");
    let spelling = expand.then(|| spelling_of_priv(&interp));
    let old = {
        let env = interp.global_env();
        let heap = interp.backend().heap();
        let m = env.get(heap, "m").expect("the program imports m");
        let heap = heap.borrow();
        let compiled = heap.get_macro(m).expect("m is a macro");
        std::rc::Rc::downgrade(
            compiled
                .definition_env
                .as_ref()
                .expect("m's library namespace"),
        )
    };
    interp
        .eval_program_owned(&format!("{lib}(import (t kw))\n(gc) (gc)"))
        .expect("the redefinition, the import and the collections");
    let named = spelling.is_some_and(|spelling| is_a_name(&interp, &spelling));
    (old.upgrade().is_some(), named)
}

/// A replaced library's namespace whose own keyword an expansion relinked
/// is released (#837's question 4 (a), #614's keyword variant, R-B): the
/// keyword's record was in the expanding form's pool only, the emitted code
/// names nothing of it, and nothing is left in the program under the
/// spelling. With the expansion or without, the namespace goes. Until the
/// switch the program's alias kept the record, its cell, the private macro
/// and the macro's `definition_env`, the old namespace, for good.
#[test]
fn a_relinked_keyword_keeps_no_definition_namespace() {
    let released = (false, false);
    assert_eq!(
        [
            the_old_library_namespace_after_a_redefinition(
                common::vm_interpreter(),
                "",
                KEYWORD_LIB,
                true
            ),
            the_old_library_namespace_after_a_redefinition(
                common::tree_walker_interpreter(),
                "",
                KEYWORD_LIB,
                true
            ),
            the_old_library_namespace_after_a_redefinition(
                common::vm_interpreter(),
                "",
                KEYWORD_LIB,
                false
            ),
            the_old_library_namespace_after_a_redefinition(
                common::tree_walker_interpreter(),
                "",
                KEYWORD_LIB,
                false
            ),
        ],
        [released; 4],
        "[VM and tree-walker with the expansion, VM and tree-walker without]"
    );
}

/// A library like [`KEYWORD_LIB`] whose `priv` is imported from another
/// library, [`PRIV_LIB`], which is defined once.
const IMPORTED_KEYWORD_LIB: &str = "(define-library (t kw)
  (export m spell-priv)
  (import (scheme base) (t kw-priv))
  (begin
    (define-syntax quote-it (syntax-rules () ((_ x) 'x)))
    (define-syntax spell-priv (syntax-rules () ((_) (quote-it priv))))
    (define-syntax m (syntax-rules () ((_ x) (priv x))))))
";

/// The library [`IMPORTED_KEYWORD_LIB`] imports `priv` from.
const PRIV_LIB: &str = "(define-library (t kw-priv)
  (export priv)
  (import (scheme base))
  (begin
    (define-syntax priv (syntax-rules () ((_ x) (+ x 1))))))
";

/// A replaced library whose relinked keyword it imports is released too,
/// and nothing stays in the program for it. The expansion's table entry
/// was `(t kw)`'s record for `priv`, an import record over `(t kw-priv)`'s
/// cell, whose macro's definition environment is `(t kw-priv)`; nothing it
/// reached led back to the old `(t kw)`, and nothing holds it now. Until
/// the switch the old `(t kw)` went and the alias stayed, with a symbol, a
/// pair and two table entries for each redefinition (#614's `relinkki`).
#[test]
fn a_relinked_imported_keyword_leaves_nothing_of_the_replaced_library() {
    let released = (false, false);
    assert_eq!(
        [
            the_old_library_namespace_after_a_redefinition(
                common::vm_interpreter(),
                PRIV_LIB,
                IMPORTED_KEYWORD_LIB,
                true
            ),
            the_old_library_namespace_after_a_redefinition(
                common::tree_walker_interpreter(),
                PRIV_LIB,
                IMPORTED_KEYWORD_LIB,
                true
            ),
        ],
        [released; 2],
        "[VM, tree-walker]"
    );
}
