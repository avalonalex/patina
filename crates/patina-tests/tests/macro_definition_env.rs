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
        .get(name)
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
