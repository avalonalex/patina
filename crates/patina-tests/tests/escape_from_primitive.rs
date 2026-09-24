//! Escaping out of a callback that a Rust primitive is running.
//!
//! A higher-order primitive re-enters the VM to run its callback. If a
//! continuation captured *outside* is invoked *inside*, the nested frames are
//! popped rather than run to completion — and until 2026-08-15 the VM then
//! wrote the primitive's result through a register base belonging to a frame
//! that no longer existed, taking the process down with `index out of bounds`
//! in `set_reg_at`. The tree-walker returned the right answer throughout, so
//! it is the expectation here.
//!
//! The primitive is also *abandoned* now rather than left running on a stack
//! it no longer owns: `ApplyContext::apply_proc` returns a non-catchable
//! `ContinuationEscape` so the primitive unwinds through its own `?`, and the
//! dispatch loop takes the value from there.
//!
//! The escape is told by the continuation, not the frames. Each re-entry
//! boundary has an id, a continuation records the boundaries it was captured
//! inside (`VmContinuation::reentry`), and one that arrives from outside a
//! boundary leaves it. Frame depth told it until 2026-09-23, and could not
//! tell every case: a continuation captured before the primitive was called
//! and one the callback captured after a tail call popped its own frame
//! restore the very same machine (#469, #472, #474), and one captured outside
//! at a deeper stack restores more frames than the callback started with,
//! which panicked (#473). #420 had closed two tail-position routes just before
//! by running the primitive before the pop. The rows are in `cps-features.scm`
//! and `prompts.scm`.
//!
//! Re-entering a callback's continuation after its primitive has *returned*
//! is a different defect: a Rust frame cannot be part of a continuation, so
//! nothing can resume the primitive. The procedures that call back into the
//! program are Scheme since #471 for that reason — `member` and `assoc` with a
//! comparator, `call-with-port` and the file variants — and the VM's `force`
//! runs a promise's thunk in a stub frame since #476. A parameter's converter
//! is a call the machine makes since #478: `make-parameter`,
//! `%parameter-convert` and `%parameter-set!` are resumable primitives
//! (`patina_primitives::Step`), which hand the call to the VM's `resume_stub`
//! frame or the tree-walker's `ResumePrimitive` continuation and are resumed
//! with its result. `eval` and `load` are resumable too since #477: each hands
//! the machine a datum to evaluate (`Step::Eval`), which the VM compiles into
//! a closure for the stub frame to call and the tree-walker runs on the
//! trampoline it is on, so no primitive left calls back into the program
//! from Rust.
//!
//! # The tree-walker's side, closed 2026-09-10
//!
//! Its callbacks run on a nested trampoline, which used to start with every
//! stack empty and to read every continuation invoke inside the callback as
//! leaving the primitive — so the rest of the *program* ran from inside the
//! callback, and the callback's value became the form's. Each trampoline now
//! carries its caller's three stacks and an identity that captured
//! continuations record (`cps_eval/types.rs`), and a jump resumes in place,
//! escapes through the primitive, or reports a callback whose primitive has
//! returned, by that identity. The rows this file held as `assert_divergence`
//! quarantines while that was open are plain rows in
//! `tests/scheme/control/cps-features.scm` now, arbitrated by chibi and
//! Gauche, and so is everything else here that needs neither a file on disk
//! nor `eval`'s environment (the "Escaping out of a callback" section there).
//! What stays: the matrix below, whose port rows need real files and whose
//! `eval` row needs `interaction-environment` to see a script's definitions,
//! which Gauche's does not; the `eval` escape, for the same reason; and the
//! port-open test, for the files.

mod common;
use common::{assert_program_eval_to, scratch_path};
use tempfile::TempDir;

/// Every re-entrant primitive reachable without a file, **left by both kinds
/// of transfer**: a full continuation escaping out, and an
/// `abort-current-continuation` reaching a prompt outside. `'x` is what each
/// carries, so it is also what the whole expression must produce.
///
/// The escape half is the original sweep. The abort half is issue #177, and
/// the reason the sweep is parameterised rather than duplicated: the two
/// leave a re-entry boundary by *different* routes, and the boundary check
/// that catches one need not catch the other. It did not — an abort lands at
/// exactly the depth a callback returning normally leaves, so the frame-depth
/// test every boundary used read it as a return. `force` was fixed first and
/// alone; `eval` kept the old failure verbatim, and a parameter *set* stored
/// the abort's value into the parameter and never ran the handler.
///
/// That is twice this file's own promise — "every re-entrant primitive" — has
/// been narrower than it sounds. `eval` and the parameter set are listed now
/// because they were missing, not only because they broke.
///
/// And once the other way: `member`, `assoc`, `call-with-port` and the file
/// variants are Scheme since #471, so their rows call the primitives still
/// under them, from `(patina internal lists)` and `(patina internal io)` —
/// the standard names would reach no boundary at all. `force`'s row reaches
/// none on either backend now: the VM runs a promise's thunk in a stub frame
/// since #476, and the tree-walker's `force` was always native CPS. It stays
/// as a check that leaving the thunk still reaches the target.
#[test]
fn test_every_re_entrant_primitive_can_be_left_by_escape_and_by_abort() {
    let dir = TempDir::new().expect("temp dir");
    let input = scratch_path(&dir, "in.txt");
    std::fs::write(&input, "seed\n").expect("seed file");
    let output = scratch_path(&dir, "out.txt");

    // `LEAVE` is what the callback evaluates. Both transfers are spelled
    // through globals so that the `eval` row — which runs its form in a fresh
    // environment — can reach them like any other.
    const PRELUDE: &str = "(import (scheme base) (scheme lazy) (scheme file) \
                           (scheme eval) (scheme repl) \
                           (rename (only (patina internal lists) member assoc) \
                                   (member prim-member) (assoc prim-assoc)) \
                           (rename (only (patina internal io) call-with-port \
                                         call-with-input-file call-with-output-file) \
                                   (call-with-port prim-call-with-port) \
                                   (call-with-input-file prim-call-with-input-file) \
                                   (call-with-output-file prim-call-with-output-file))) \
                           (define esc #f) \
                           (define t (make-continuation-prompt-tag 'p)) \
                           (define unwound 0)";

    let bodies = [
        "(prim-member 2 '(1 2 3) (lambda (a b) LEAVE))".to_string(),
        "(prim-assoc 2 '((1 . a) (2 . b)) (lambda (a b) LEAVE))".to_string(),
        "(force (delay LEAVE))".to_string(),
        "(make-parameter 1 (lambda (v) LEAVE))".to_string(),
        // A parameter *set*, which reaches its converter through a different
        // boundary than `parameterize` does — the one that stored the abort's
        // value into the parameter.
        "(let ((q (make-parameter 0 (lambda (v) (if (= v 5) LEAVE v))))) (q 5))".to_string(),
        "(eval 'LEAVE (interaction-environment))".to_string(),
        "(prim-call-with-port (open-output-string) (lambda (p) LEAVE))".to_string(),
        format!(r#"(prim-call-with-input-file "{input}" (lambda (p) LEAVE))"#),
        format!(r#"(prim-call-with-output-file "{output}" (lambda (p) LEAVE))"#),
    ];

    for (transfer, wrap, leave, value) in [
        (
            "escape",
            "(call/cc (lambda (k) (set! esc k) BODY))",
            "(esc 'x)",
            "x",
        ),
        (
            "abort",
            // The handler's answer is not the abort's value, so a callback
            // that returned `'x` cannot pass for a handled abort — which is
            // how #342 got past this sweep when the handler returned `v`.
            "(call-with-continuation-prompt (lambda () BODY) t (lambda (v k2) (list 'handled v)))",
            "(abort-current-continuation t 'x)",
            "(handled x)",
            // Both backends since 2026-09-10. Until then the tree-walker ran
            // most of these callbacks on a nested trampoline that started
            // every stack empty, so the abort found no prompt — the
            // "primitive's callback" hole its winds and handlers had since
            // before prompts. Escaping always worked there, because a full
            // continuation carries its own target rather than searching for
            // one; an abort's landing now carries its prompt's trampoline,
            // so it unwinds through the primitive the same way.
        ),
    ] {
        // In non-tail position the primitive's result has a register to go to
        // in a frame the transfer removes.
        for (position, place) in [("tail", "BODY"), ("non-tail", "(car (list BODY))")] {
            // A `dynamic-wind` between the transfer and its target makes it
            // travel, running the after thunk on the way (#342); with
            // nothing to leave, an abort cuts the machine back in place.
            for (crossing, around, unwinds) in [
                ("nothing", "LEAVE", 0),
                (
                    "a dynamic-wind",
                    "(dynamic-wind (lambda () #f) (lambda () LEAVE) \
                     (lambda () (set! unwound (+ unwound 1))))",
                    1,
                ),
            ] {
                let leave = around.replace("LEAVE", leave);
                for body in &bodies {
                    let body = place.replace("BODY", &body.replace("LEAVE", &leave));
                    let program = format!(
                        "{PRELUDE} (define r {}) (list r unwound)",
                        wrap.replace("BODY", &body)
                    );
                    assert_eq!(
                        common::eval_program(&program),
                        format!("({value} {unwinds})"),
                        "[{transfer}, {position}, crossing {crossing}] {body}"
                    );
                }
            }
        }
    }
}

/// `eval` and `load` re-enter the VM the same way a higher-order primitive
/// does, and the first attempt missed them: it detected the escape in
/// `apply_proc` alone, so `load` kept executing the remaining forms of the
/// file after the continuation had been invoked, where chibi stops at the
/// escaping form.
///
/// Both backends since 2026-09-10. The tree-walker used to escape *and then
/// continue*: the nested `eval` run had its own copy of the escape-catching
/// arm and resumed the outer continuation *inside* itself, then returned
/// into the primitive, which ran the lambda on — so `'fell-through` reached
/// the same continuation a second time, and in an SRFI 64 file every row
/// after this one ran twice. A trampoline now resumes only the continuations
/// whose chain ends in it; this one's ends in the outer run, so the escape
/// unwinds through `eval`. Rust rather than a suite row because Gauche's
/// `interaction-environment` does not see a script's top-level definitions.
#[test]
fn test_escaping_out_of_eval() {
    assert_program_eval_to(
        r#"(import (scheme base) (scheme eval) (scheme repl))
           (define kk #f)
           (define trace '())
           (define r (call/cc (lambda (k)
                       (set! kk k)
                       (eval '(kk 'from-eval) (interaction-environment))
                       (set! trace (cons 'ran-on trace))
                       'fell-through)))
           (list r (reverse trace))"#,
        "(from-eval ())",
    );
}

/// A continuation captured by one form of a loaded file and invoked from a
/// later form of the same file. `load` evaluates each form on its own
/// nested run; the earlier run has returned by the time `k` is invoked, and
/// what every implementation does is resume the rest of that form and then
/// carry on with the form *after the invoking one* — `r` is 11, the `if`
/// does not run again, `n` stays 1. The first cut of the trampoline fix
/// refused a continuation whose run had returned, and the review measured
/// this program failing where `main`, the VM, chibi and Gauche all agree.
/// Rust because it needs a file on disk.
#[test]
fn test_load_reenters_a_continuation_captured_by_an_earlier_form() {
    let dir = TempDir::new().expect("temp dir");
    let loaded = scratch_path(&dir, "loaded.scm");
    std::fs::write(
        &loaded,
        "(define k #f) (define n 0)\n\
         (define r (+ 1 (call/cc (lambda (c) (set! k c) 1))))\n\
         (set! n (+ n 1))\n\
         (if (< n 3) (k 10))\n\
         (define loaded-result (list r n))\n",
    )
    .expect("loaded file");
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme load))
               (load "{loaded}")
               loaded-result"#
        ),
        "(11 1)",
    );
}

/// What the port procedures do with their port when the callback escapes —
/// Scheme since #471, primitives when this was written.
/// R7RS 6.13.1: `call-with-port` closes the port "if `proc` returns" — and
/// only then, because a `guard` clause runs after that escape and is entitled
/// to read what the callback wrote, or to decide the port is still its own.
/// Until 2026-09-01 all three closed on every exit. Closing on the normal
/// return is the other half, so it is checked in the same program.
#[test]
fn test_an_escape_out_of_a_port_callback_leaves_the_port_open() {
    let dir = TempDir::new().expect("temp dir");
    let input = scratch_path(&dir, "in.txt");
    std::fs::write(&input, "seed\n").expect("seed file");
    let output = scratch_path(&dir, "out.txt");

    assert_program_eval_to(
        r#"(import (scheme base))
           (define out #f)
           (list
             (guard (e (#t (get-output-string out)))
               (call-with-port (open-output-string)
                 (lambda (p) (set! out p) (write-string "inside" p) (raise 'x))))
             (call-with-port (open-output-string)
               (lambda (p) (set! out p) 'returned))
             (output-port-open? out))"#,
        "(\"inside\" returned #f)",
    );
    // The condition is in the answer too: until 2026-09-10 the tree-walker
    // delivered a raise inside a primitive's callback to the outer `guard`
    // as an "unhandled exception" error object rather than as `'x`, and this
    // test left it out so as not to test that defect instead of its own
    // subject.
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme file))
               (define seen #f)
               (list
                 (guard (e (#t (list e (read-char seen))))
                   (call-with-input-file "{input}"
                     (lambda (p) (set! seen p) (raise 'x))))
                 (input-port-open? (call-with-input-file "{input}" (lambda (p) p)))
                 (guard (e (#t (list e (output-port-open? seen))))
                   (call-with-output-file "{output}"
                     (lambda (p) (set! seen p) (raise 'y))))
                 (output-port-open? (call-with-output-file "{output}" (lambda (p) p))))"#
        ),
        "((x #\\s) #f (y #t) #f)",
    );
}

/// Re-entering, after `call-with-input-file` has returned, a continuation
/// captured inside its procedure: the rest of the call — closing the port and
/// returning the value — runs again (#471). The two file procedures were Rust
/// primitives, which a continuation cannot carry, so the re-entry found
/// nothing to return into and the VM answered a stray internal `#<cell>`;
/// they are `call-with-port` over an opened port now, in Scheme, and answer
/// as chibi and Gauche do. Rust because it needs a file on disk.
#[test]
fn test_reentering_a_file_callback_after_the_call_returned() {
    let dir = TempDir::new().expect("temp dir");
    let input = scratch_path(&dir, "in.txt");
    std::fs::write(&input, "abc").expect("input file");
    let output = scratch_path(&dir, "out.txt");
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme file))
               (define (reenter call)
                 (let ((saved #f) (out '()))
                   (let ((r (call (lambda (port)
                                    (+ 100 (call/cc (lambda (c)
                                                      (if (not saved) (set! saved c))
                                                      1)))))))
                     (set! out (cons r out))
                     (if (< (length out) 3) (saved (length out)) (reverse out)))))
               (list (reenter (lambda (proc) (call-with-input-file "{input}" proc)))
                     (reenter (lambda (proc) (call-with-output-file "{output}" proc))))"#
        ),
        "((101 101 102) (101 101 102))",
    );
}

/// Re-entering, after `eval` has returned, a continuation captured inside the
/// code it evaluated: `eval` returns again, with the new value (#477), in and
/// out of tail position. `eval` ran the code on a run of its own from Rust,
/// which a continuation cannot carry: out of tail position the VM answered a
/// stray internal `#<cell>`, and the tree-walker abandoned the form. The
/// machine runs it now (`patina_primitives::Step::Eval`), and both answer as
/// chibi does. Rust rather than a suite row because Gauche's
/// `interaction-environment` does not see a script's top-level definitions.
#[test]
fn test_reentering_eval_after_it_returned() {
    assert_program_eval_to(
        r#"(import (scheme base) (scheme eval) (scheme repl))
           (define %kk #f)
           (define (non-tail)
             (let ((out '()))
               (let ((r (eval '(+ 100 (call/cc (lambda (c) (set! %kk c) 1)))
                              (interaction-environment))))
                 (set! out (cons r out))
                 (if (< (length out) 3) (%kk (length out)) (reverse out)))))
           (define (ev)
             (eval '(+ 100 (call/cc (lambda (c) (set! %kk c) 1))) (interaction-environment)))
           (define (tail)
             (let ((out '()))
               (let ((r (ev)))
                 (set! out (cons r out))
                 (if (< (length out) 3) (%kk (length out)) (reverse out)))))
           (list (non-tail) (tail))"#,
        "((101 101 102) (101 101 102))",
    );
}

/// Re-entering, after `load` has returned, a continuation captured by a form
/// of the loaded file: the rest of that form runs again, `load` carries on
/// from where its file stands — at the end — and returns again (#477), as
/// chibi's does. The tree-walker abandoned the form, `load` running each form
/// on a run of its own. Rust because it needs a file on disk, and because
/// Gauche loads into a module that does not see the script's definitions.
#[test]
fn test_reentering_a_loaded_form_after_load_returned() {
    let dir = TempDir::new().expect("temp dir");
    let loaded = scratch_path(&dir, "loaded.scm");
    std::fs::write(
        &loaded,
        "(define lr (+ 100 (call/cc (lambda (c) (set! kk c) 1))))\n",
    )
    .expect("loaded file");
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme load))
               (define kk #f)
               (define (main)
                 (let ((out '()))
                   (let ((r (begin (load "{loaded}") lr)))
                     (set! out (cons r out))
                     (if (< (length out) 3) (kk (length out)) (reverse out)))))
               (main)"#
        ),
        "(101 101 102)",
    );
}

/// A raise in the body of a library that an `eval`'d `import` loads, caught
/// by a `guard` around the `eval`: the handler escapes out of the load. The
/// VM ran the rest of the library body after the escape, registered the
/// half-run library and bound its exports, then pushed `eval`'s stub frame
/// onto the stack the escape had restored, so the stub's value landed in a
/// register of the `guard`'s frame — `a` below — and the second `import`
/// found the library loaded. chibi raises twice and leaves `f` unbound, as
/// the tree-walker does; Gauche raises once, finds its half-loaded module the
/// second time, and leaves `f` unbound too. Rust because it needs a library
/// on disk and `eval`'s environment to see the script's definitions.
#[test]
fn test_escaping_out_of_a_library_body_an_eval_import_loads() {
    use patina_interpreter::Interpreter;
    use patina_runtime::Backend;

    fn run<B: Backend>(interpreter: &Interpreter<B>, program: &str) -> String {
        let value = interpreter
            .eval_program(program)
            .unwrap_or_else(|e| panic!("{program}: {e}"));
        patina_primitives::primitives::io::datum_writer::format_write_tagged(
            value,
            interpreter.backend().global_env().heap(),
        )
    }

    let root = TempDir::new().expect("temp dir");
    let dir = root.path().join("escape482");
    std::fs::create_dir_all(&dir).expect("library dir");
    std::fs::write(
        dir.join("raises.sld"),
        "(define-library (escape482 raises) (import (scheme base)) (export f)
           (begin (define f 1) (car '()) (set! f 2)))",
    )
    .expect("library file");
    const PROGRAM: &str = r#"(import (scheme base) (scheme eval) (scheme repl))
        (define (attempt)
          (let ((a 10))
            (list (guard (e (#t 'raised))
                    (eval '(import (escape482 raises)) (interaction-environment))
                    'imported)
                  a)))
        (list (attempt) (attempt)
              (guard (e (#t 'unbound)) (eval 'f (interaction-environment))))"#;
    const EXPECTED: &str = "((raised 10) (raised 10) unbound)";

    let vm = common::vm_interpreter();
    vm.backend()
        .add_library_search_path(root.path().to_path_buf());
    assert_eq!(run(&vm, PROGRAM), EXPECTED, "VM");

    let tw = common::tree_walker_interpreter();
    tw.backend()
        .add_library_search_path(root.path().to_path_buf());
    assert_eq!(run(&tw, PROGRAM), EXPECTED, "tree-walker");
}
