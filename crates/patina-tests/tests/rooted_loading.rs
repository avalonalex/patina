//! A library's load collects, on both backends (#677).
//!
//! No library load collected while it ran: `ParsedLibrary` held its unrun
//! body under a holder's guard, the environment being built was reachable
//! from no root until the library was registered, and the VM's `with_globals`
//! held the environment it set aside the same way. Now the registry's
//! loading stack roots each load's environment and the body forms it has not
//! yet run, and the VM keeps what `with_globals` sets aside on a traced
//! stack. A load the backend runs at its top level, outermost, collects
//! inside its body forms (point D) and where a library's load ends (point B),
//! which also lets a run of `define-library` forms collect (#614's program
//! 2). A load under a guard, such as one a running program asks for, still
//! defers.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_interpreter::{Backend, Interpreter};

fn collections<B: Backend>(interp: &Interpreter<B>) -> u64 {
    interp.global_env().heap().borrow().gc_collections()
}

/// Collections while `program` is evaluated.
fn collected_during<B: Backend>(interp: &Interpreter<B>, program: &str) -> u64 {
    let before = collections(interp);
    interp.eval_program_owned(program).unwrap();
    collections(interp) - before
}

/// #614's program 2, without the body: a run of `define-library` forms and
/// nothing else, so no code runs that could poll, and only point B can
/// collect. On `main` the first collection waited for the first ordinary
/// form after the run.
#[test]
fn a_run_of_define_library_forms_collects() {
    let program = "(define-library (rooted steady) (import (scheme base)))\n".repeat(400);
    let vm = collected_during(&vm_interpreter(), &program);
    let tree_walker = collected_during(&tree_walker_interpreter(), &program);
    assert!(
        vm > 0 && tree_walker > 0,
        "collections during the run: VM {vm}, tree-walker {tree_walker}"
    );
}

/// A library whose body collects partway through: `early` is bound in the
/// library's environment before the collection, and `later` is a body form
/// not yet run when it happens. Each is rooted by the load's registry entry
/// alone.
const COLLECTING_LIBRARY: &str = "(define-library (rooted collecting)
  (export early later)
  (import (scheme base) (patina debug))
  (begin
    (define early (list 'early (vector 1 2 3) \"early\"))
    (gc)
    (define later '(later #(4 5 6) \"later\"))))";

/// Collections during the library's load, then `early`, `later` and a global
/// the program bound before it, read back. On the VM the global environment
/// is set aside while the body runs, and only the machine's traced stack
/// reaches it.
fn collecting_body<B: Backend>(interp: Interpreter<B>) -> (u64, String) {
    interp
        .eval_program_owned("(define kept-global (list 'global (vector 7) \"global\"))")
        .unwrap();
    let during = collected_during(&interp, COLLECTING_LIBRARY);
    let value = interp
        .eval_program_owned("(import (rooted collecting)) (list early later kept-global)")
        .unwrap();
    (during, interp.display_tagged(&value))
}

#[test]
fn a_library_body_collects_while_it_loads() {
    let expected = "((early #(1 2 3) \"early\") (later #(4 5 6) \"later\") \
                    (global #(7) \"global\"))";
    let answers = [
        collecting_body(vm_interpreter()),
        collecting_body(tree_walker_interpreter()),
    ];
    assert!(
        answers.iter().all(|(during, _)| *during > 0),
        "collections during the load (VM, tree-walker): {answers:?}"
    );
    for value in answers.map(|(_, value)| value) {
        assert_eq!(value, expected);
    }
}

/// A library file whose body collects, imported by an inline library:
/// loading the importer loads the dependency first, and the dependency's
/// `(gc)` runs while the importer's environment holds only some of its
/// imports and none of its body has run.
const DEPENDENCY: &str = "(define-library (rooted dependency)
  (export made)
  (import (scheme base) (patina debug))
  (begin
    (define made (list 'made (vector 8 9)))
    (gc)))";

const IMPORTER: &str = "(define-library (rooted importer)
  (export both)
  (import (scheme base) (rooted dependency))
  (begin
    (define mine (list 'mine \"importer\"))
    (define both (list made mine))))";

fn collecting_dependency<B: Backend + SearchPath>(interp: Interpreter<B>) -> (u64, String) {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("rooted")).unwrap();
    std::fs::write(dir.path().join("rooted/dependency.sld"), DEPENDENCY).unwrap();
    interp.backend().add_search_path(dir.path().to_path_buf());
    let during = collected_during(&interp, IMPORTER);
    let value = interp
        .eval_program_owned("(import (rooted importer)) both")
        .unwrap();
    (during, interp.display_tagged(&value))
}

/// Both backends name the method alike, but not through `Backend`.
trait SearchPath {
    fn add_search_path(&self, path: std::path::PathBuf);
}

impl SearchPath for patina_vm::VmBackend {
    fn add_search_path(&self, path: std::path::PathBuf) {
        self.add_library_search_path(path);
    }
}

impl SearchPath for patina_interpreter::TreeWalker {
    fn add_search_path(&self, path: std::path::PathBuf) {
        self.add_library_search_path(path);
    }
}

#[test]
fn a_library_collects_while_its_dependency_loads() {
    let expected = "((made #(8 9)) (mine \"importer\"))";
    let answers = [
        collecting_dependency(vm_interpreter()),
        collecting_dependency(tree_walker_interpreter()),
    ];
    assert!(
        answers.iter().all(|(during, _)| *during > 0),
        "collections during the load (VM, tree-walker): {answers:?}"
    );
    for value in answers.map(|(_, value)| value) {
        assert_eq!(value, expected);
    }
}
