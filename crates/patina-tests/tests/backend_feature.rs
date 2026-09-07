//! Each backend names itself to `cond-expand` and to `(features)`.
//!
//! The point of the mechanism is #193: a migrated `.scm` test file expresses a
//! per-backend expectation in portable R7RS —
//! `(cond-expand (patina-vm (test-expect-fail 1)) (else))` — instead of the
//! harness carrying which backend it is. On another implementation the same
//! file takes `else`, which is what keeps it portable.
//!
//! # Why this file is shaped as a list of *shapes*
//!
//! A feature set is a property of one interpreter instance, not of the
//! process — both backends live in this test binary — so the identifier cannot
//! be a global. The first attempt (#206, reverted) instead threaded it through
//! each `Desugarer` construction site. It missed four of them, and the test
//! written alongside it *claimed* to catch a missed site while exercising one
//! construction shape out of five. Everything was green with a live
//! cross-backend divergence.
//!
//! So the identifier now lives on the `Heap` — the only per-instance thing the
//! desugarer, the library parser and the `features` primitive all already hold
//! — and this file enumerates the shapes that reach a `cond-expand`, taken
//! from the code rather than from memory:
//!
//! | shape | route |
//! |---|---|
//! | a top-level program | `Backend::eval` → `Desugarer` |
//! | `eval` | the `eval` primitive's own desugarer |
//! | a library `begin` body | the backends' library loaders |
//! | a `.sld` *declaration* | `library_parser.rs`, a different code path |
//! | inside a quasiquote | the quasiquote sub-expression desugarers |
//! | a nested binding form | child desugarers |
//! | an `include`d file | `desugar_include_tagged`, which re-enters the desugarer |
//! | `include-library-declarations` | `parse_library_declarations_file`, recursing into the parser |
//! | `(features)` | the primitive, which R7RS §4.2.1 ties to `cond-expand` |
//!
//! A missed shape does not error — `cond-expand` falls through to `else`,
//! which is a plausible-looking answer. That is what makes enumeration the
//! test rather than a spot check.

mod common;
use common::{eval_program_tree_walker, eval_program_vm};

/// `(cond-expand …)` returning this backend's name, `'neither` if it has none.
const WHICH: &str = "(cond-expand (patina-vm 'vm) (patina-tree-walker 'tw) (else 'neither))";

/// Assert both backends answer with their own name for a program built from
/// `WHICH` by `shape`.
fn each_backend_sees_itself(shape: &str, program: impl Fn(&str) -> String) {
    assert_eq!(
        eval_program_vm(&program(WHICH)),
        "vm",
        "[vm] {shape}: the backend identifier does not reach here"
    );
    assert_eq!(
        eval_program_tree_walker(&program(WHICH)),
        "tw",
        "[tree-walker] {shape}: the backend identifier does not reach here"
    );
}

#[test]
fn a_top_level_program_sees_the_backend() {
    each_backend_sees_itself("top-level program", |w| w.to_string());
}

/// Under `eval`, which builds its own desugarer. This is one of the four the
/// reverted attempt missed: it answered `vm` at top level and `neither` here.
#[test]
fn eval_sees_the_backend() {
    each_backend_sees_itself("eval", |w| {
        format!("(import (scheme eval) (scheme repl)) (eval '{w} (environment '(scheme base)))")
    });
}

/// Inside a nested binding form, which gets a *child* desugarer. The reverted
/// attempt defaulted their feature set rather than inheriting it, so a
/// top-level `cond-expand` resolved while one inside a `let` took `else`.
#[test]
fn a_nested_binding_form_sees_the_backend() {
    each_backend_sees_itself("nested in a let/lambda", |w| {
        format!("((let ((x 1)) (lambda () {w})))")
    });
}

/// Inside a quasiquote's unquote, which has its own sub-expression desugarer
/// on each backend. Both were missed before; the same file gave two answers.
#[test]
fn a_quasiquoted_subexpression_sees_the_backend() {
    each_backend_sees_itself("quasiquote unquote", |w| format!("(car `(,{w}))"));
}

/// An `include`d file, which re-enters the desugarer on the included forms.
#[test]
fn an_included_file_sees_the_backend() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let part = dir.path().join("part.scm");
    std::fs::write(&part, format!("(define included {WHICH})")).expect("write include");
    each_backend_sees_itself("included file", |_| {
        format!("(include \"{}\") included", part.display())
    });
}

/// `include-library-declarations`, which recurses back into the *library
/// parser* rather than the desugarer — a fifth route, and the one this file's
/// table named before it had a test, which is the overclaim this whole file
/// exists to avoid making.
#[test]
fn included_library_declarations_see_the_backend() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let lib = dir.path().join("probe");
    std::fs::create_dir_all(&lib).expect("mkdir");
    std::fs::write(
        lib.join("decls.scm"),
        "(cond-expand (patina-vm (begin (define from-included-decls 'incl-vm)))
                      (patina-tree-walker (begin (define from-included-decls 'incl-tw)))
                      (else (begin (define from-included-decls 'incl-neither))))",
    )
    .expect("write decls");
    std::fs::write(
        lib.join("outer.sld"),
        r#"(define-library (probe outer)
             (import (scheme base))
             (export from-included-decls)
             (include-library-declarations "decls.scm"))"#,
    )
    .expect("write library");

    for (backend, expected) in [("tree-walker", "incl-tw"), ("vm", "incl-vm")] {
        let root = dir.path().to_path_buf();
        let out = if backend == "vm" {
            let vm = common::vm_interpreter();
            vm.backend().add_library_search_path(root);
            run(&vm, "(import (probe outer)) from-included-decls")
        } else {
            let tw = common::tree_walker_interpreter();
            tw.backend().add_library_search_path(root);
            run(&tw, "(import (probe outer)) from-included-decls")
        };
        assert_eq!(
            out, expected,
            "[{backend}] include-library-declarations does not see the backend"
        );
    }
}

/// `(features)` must agree with `cond-expand` — R7RS §4.2.1 defines it as
/// exactly "a list of the feature identifiers which `cond-expand` treats as
/// true". The reverted attempt could not satisfy this at all: the primitive
/// built a fresh `default_features()` while the desugarer read its own copy,
/// so `(features)` reported the backend absent while `cond-expand` treated it
/// as present.
///
/// Asserted in both directions. Presence alone would pass if *both* names were
/// advertised on both backends, which would make the identifier useless for
/// choosing a branch.
#[test]
fn features_agrees_with_cond_expand_and_is_exclusive() {
    const PROBE: &str = "(list (and (memq 'patina-vm (features)) #t)
                               (and (memq 'patina-tree-walker (features)) #t)
                               (and (memq 'patina (features)) #t))";
    assert_eq!(eval_program_vm(PROBE), "(#t #f #t)");
    assert_eq!(eval_program_tree_walker(PROBE), "(#f #t #t)");
}

/// The identifier composes with `not`, `and` and `or`, since an expectation
/// table will want "every backend but this one".
#[test]
fn the_identifier_composes() {
    assert_eq!(
        eval_program_vm("(cond-expand ((not patina-vm) 'not-vm) (else 'is-vm))"),
        "is-vm"
    );
    assert_eq!(
        eval_program_tree_walker("(cond-expand ((not patina-vm) 'not-vm) (else 'is-vm))"),
        "not-vm"
    );
    const EITHER: &str = "(cond-expand ((or patina-vm patina-tree-walker) 'a-backend)
                                       (else 'neither))";
    assert_eq!(eval_program_vm(EITHER), "a-backend");
    assert_eq!(eval_program_tree_walker(EITHER), "a-backend");
    const BOTH: &str = "(cond-expand ((and patina patina-vm) 'vm-and-patina) (else 'no))";
    assert_eq!(eval_program_vm(BOTH), "vm-and-patina");
    assert_eq!(eval_program_tree_walker(BOTH), "no");
}

/// A library's `begin` body **and** a `.sld` *declaration*, which are two
/// different code paths — the body goes through the backends' library loaders
/// and the declaration through `library_parser.rs`.
///
/// The declaration case is the one the reverted attempt's own doc comment
/// conceded it could not reach ("`cond-expand` inside a `.sld` goes through
/// `library_parser.rs`, which builds its own registry"). Putting the registry
/// on the heap is what closed it, so a single `.sld` can no longer answer
/// `cond-expand` two ways depending on where in the file it appears.
#[test]
fn a_library_sees_the_backend_in_both_its_declarations_and_its_body() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let lib = dir.path().join("probe");
    std::fs::create_dir_all(&lib).expect("mkdir");
    std::fs::write(
        lib.join("who.sld"),
        r#"(define-library (probe who)
             (import (scheme base))
             (export from-body from-declaration)
             (cond-expand (patina-vm (begin (define from-declaration 'decl-vm)))
                          (patina-tree-walker (begin (define from-declaration 'decl-tw)))
                          (else (begin (define from-declaration 'decl-neither))))
             (begin
               (define from-body
                 (cond-expand (patina-vm 'body-vm)
                              (patina-tree-walker 'body-tw)
                              (else 'body-neither)))))"#,
    )
    .expect("write library");

    let root = dir.path().to_path_buf();
    let program = "(import (probe who)) (list from-body from-declaration)";

    let tw = common::tree_walker_interpreter();
    tw.backend().add_library_search_path(root.clone());
    assert_eq!(
        run(&tw, program),
        "(body-tw decl-tw)",
        "[tree-walker] a library's body and declarations must both see the backend"
    );

    let vm = common::vm_interpreter();
    vm.backend().add_library_search_path(root);
    assert_eq!(
        run(&vm, program),
        "(body-vm decl-vm)",
        "[vm] a library's body and declarations must both see the backend"
    );
}

fn run<B: patina_runtime::Backend>(
    interp: &patina_interpreter::Interpreter<B>,
    program: &str,
) -> String {
    let value = interp
        .eval_program(program)
        .unwrap_or_else(|e| panic!("failed to run: {e}"));
    patina_primitives::primitives::io::datum_writer::format_display_tagged(
        value,
        interp.backend().global_env().heap(),
    )
}
