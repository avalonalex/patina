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

/// An alias is an ordinary spelling, so a program can define over it. The
/// next expansion must not reach that definition: it gets a new alias.
fn defining_over_the_alias<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> (String, String) {
    interp
        .eval_program(&format!(
            "{PRIVATE_HELPER}(import (scheme base) (t m)) (define first (via-private 1))"
        ))
        .unwrap();
    let env = interp.global_env();
    let spelling = (0..100_000)
        .map(|k| format!("private-helper.{k}"))
        .find(|spelling| env.get(interp.backend().heap(), spelling).is_some())
        .expect("relinking aliased the private helper");
    interp
        .eval_program(&format!("(define ({spelling} x) 'mine)"))
        .unwrap();
    let second = interp.eval_str("(via-private 2)").unwrap();
    let own = interp.eval_str(&format!("({spelling} 0)")).unwrap();
    (interp.display_tagged(second), interp.display_tagged(own))
}

#[test]
fn a_definition_over_an_alias_does_not_capture_the_next_expansion() {
    assert_eq!(
        defining_over_the_alias(common::vm_interpreter()),
        ("2".to_string(), "mine".to_string()),
        "VM"
    );
    assert_eq!(
        defining_over_the_alias(common::tree_walker_interpreter()),
        ("2".to_string(), "mine".to_string()),
        "tree-walker"
    );
}

/// One alias per target means two expansions of a macro share a spelling for
/// its private name, so the spelling cannot keep them apart: here the outer
/// expansion's `let` binds `helper` and the inner expansion, nested in its
/// body, mentions the library's `helper` free. The renamed occurrences keep
/// their scopes, which do keep them apart. chibi 0.12 and Gauche 0.9.15 both
/// answer `(lib (lib 0))` and `(lib (lib (lib 0)))`, measured 2026-10-08;
/// with the spelling alone, both backends answered `(lib (local 0))`.
#[test]
fn nested_expansions_sharing_an_alias_do_not_capture_each_other() {
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
}

/// A redefinition: `get-old`, a macro the first `(t redef)`'s generator
/// made, and `h-old`, a procedure that expanded its `m`, each name the
/// first library's private `secret`, and still read it after the library
/// is redefined, imported again and two collections run, while a macro
/// made then and a fresh expansion of `m` read the new one's, which the
/// new `bump!` has assigned. Neither oracle redefines a library so (Gauche
/// 0.9.15 keeps the one module and answers `new` throughout; chibi 0.12
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

// ─── Keyword aliases hold their target's record (#837, R6) ──────────────────
//
// A library macro whose template names a private macro (`priv`) gets a
// relink alias in the program, `priv.N`. Since 4b.10's PR-1c the alias holds
// the private keyword's binding record, not the library's namespace.

/// A library whose exported `m` expands to its private macro `priv`.
const KEYWORD_LIB: &str = "(define-library (t kw)
  (export m)
  (import (scheme base))
  (begin
    (define-syntax priv (syntax-rules () ((_ x) (+ x 1))))
    (define-syntax m (syntax-rules () ((_ x) (priv x))))))
";

/// The program namespace's aliases for `priv`: each spelling, its home and
/// how many environments the alias entry's own trace reports.
fn aliases_of_priv<B: patina_runtime::Backend>(
    interp: &patina_interpreter::Interpreter<B>,
) -> Vec<(String, &'static str, usize)> {
    interp
        .global_env()
        .alias_homes_for_tests()
        .into_iter()
        .filter(|(spelling, _, _)| spelling.starts_with("priv."))
        .collect()
}

/// After `(m 1)`, the expansion's alias for `priv` and its answer.
fn a_keyword_alias_after_an_expansion<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
) -> (String, Vec<(&'static str, usize)>) {
    let answer = interp
        .eval_program_owned(&format!(
            "{KEYWORD_LIB}(import (scheme base) (t kw))\n(m 1)"
        ))
        .expect("the library, the import and the expansion");
    let homes = aliases_of_priv(&interp)
        .into_iter()
        .map(|(_, home, envs)| (home, envs))
        .collect();
    (interp.display_tagged(&answer), homes)
}

/// The keyword alias an expansion of `m` leaves in the program is one, holds
/// `priv`'s record, and its entry's own trace reports no environment: a
/// collection reaches the library's namespace through it, if at all, only
/// through what the record reaches (the next test). Before R6 the alias
/// named the library's namespace, an edge its trace reported.
#[test]
fn a_keyword_alias_names_no_environment_directly() {
    let expected = ("2".to_string(), vec![("keyword", 0)]);
    assert_eq!(
        [
            a_keyword_alias_after_an_expansion(common::vm_interpreter()),
            a_keyword_alias_after_an_expansion(common::tree_walker_interpreter()),
        ],
        [expected.clone(), expected],
        "[VM, tree-walker]"
    );
}

/// Whether the library namespace `m` was defined in is alive after the
/// library `lib` (`(t kw)`) is redefined and imported again and two
/// collections run, with `(m 1)` expanded before the redefinition or not,
/// and the program's aliases for `priv` then. `setup` is what the first
/// program defines before `lib`, and is not redefined. The namespace is
/// watched through `m`'s macro, whose definition environment it is, by a
/// `Weak` taken before the redefinition.
fn the_old_library_namespace_after_a_redefinition<B: patina_runtime::Backend>(
    interp: patina_interpreter::Interpreter<B>,
    setup: &str,
    lib: &str,
    expand: bool,
) -> (bool, Vec<(&'static str, usize)>) {
    interp
        .eval_program_owned(&format!(
            "{setup}{lib}(import (scheme base) (patina debug) (t kw))\n{}",
            if expand { "(m 1)" } else { "" }
        ))
        .expect("the library and the import");
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
    let homes = aliases_of_priv(&interp)
        .into_iter()
        .map(|(_, home, envs)| (home, envs))
        .collect();
    (old.upgrade().is_some(), homes)
}

/// R6 alone does not release a replaced library's namespace whose own
/// keyword an expansion relinked (#837's question 4, #614's keyword
/// variant; the next test is the imported keyword's case, which R6 does
/// release): the program's keyword alias is never
/// removed, and its record reaches its cell, the private macro, and the
/// macro's `definition_env`, the old namespace. With no expansion before the
/// redefinition there is no alias, and the namespace goes, so the alias is
/// what keeps it. This pins the edge the design forecast; it flips when
/// keyword aliases travel with the syntax, the owner's ruling (a) of
/// 2026-11-16 on question 4.
#[test]
fn a_keyword_aliass_record_still_keeps_its_macros_definition_namespace() {
    let kept = (true, vec![("keyword", 0)]);
    let released = (false, Vec::new());
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
        [kept.clone(), kept, released.clone(), released],
        "[VM and tree-walker with the expansion, VM and tree-walker without]"
    );
}

/// A library like [`KEYWORD_LIB`] whose `priv` is imported from another
/// library, [`PRIV_LIB`], which is defined once.
const IMPORTED_KEYWORD_LIB: &str = "(define-library (t kw)
  (export m)
  (import (scheme base) (t kw-priv))
  (begin
    (define-syntax m (syntax-rules () ((_ x) (priv x))))))
";

/// The library [`IMPORTED_KEYWORD_LIB`] imports `priv` from.
const PRIV_LIB: &str = "(define-library (t kw-priv)
  (export priv)
  (import (scheme base))
  (begin
    (define-syntax priv (syntax-rules () ((_ x) (+ x 1))))))
";

/// What R6 releases: a replaced library whose relinked keyword it imports.
/// The program's alias holds `(t kw)`'s record for `priv`, an import record
/// over `(t kw-priv)`'s cell, whose macro's definition environment is
/// `(t kw-priv)`; nothing it reaches leads back to the old `(t kw)`, which
/// goes while the alias stays. Before R6 the alias named the old `(t kw)`
/// and kept it, as it keeps a library whose own keyword it relinks (the
/// test above).
#[test]
fn a_keyword_alias_to_an_imported_keyword_releases_the_replaced_library() {
    let released = (false, vec![("keyword", 0)]);
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
        [released.clone(), released],
        "[VM, tree-walker]"
    );
}
