//! Dropping an interpreter tears its heap down (#604).
//!
//! The heap's objects hold environments, and the environments hold the heap,
//! so a dropped interpreter's heap stayed alive with everything in it: up to
//! 318 heaps alive at once in one test process, and a file port still
//! reachable from a global never dropped, so what it had buffered was lost.
//! Each backend now frees every slot when it is dropped
//! (`patina_core::heap::teardown`), as a sweep that found nothing live would,
//! and then writes out and closes the file ports, with no heap borrow (#785).

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_interpreter::{Backend, Interpreter};
use std::path::Path;
use std::rc::Rc;

/// A program that leaves a value of every kind that holds an environment or
/// the heap: closures, a macro, an inline library and the procedure it
/// exports, an environment specifier, a continuation, a parameter, a promise,
/// a record, and a string port.
const HOLDERS: &str = "
    (import (scheme base) (scheme eval) (scheme lazy))
    (define (adder n) (lambda (x) (+ x n)))
    (define add1 (adder 1))
    (define-syntax swap!
      (syntax-rules () ((_ a b) (let ((t a)) (set! a b) (set! b t)))))
    (define-library (teardown kept)
      (export kept)
      (import (scheme base))
      (begin (define (kept) 'kept)))
    (import (teardown kept))
    (define specifier (environment '(scheme base)))
    (define k (call/cc (lambda (k) k)))
    (define p (make-parameter 1))
    (define promise (delay (add1 1)))
    (define-record-type point (make-point x y) point? (x point-x) (y point-y))
    (define pt (make-point 1 2))
    (define port (open-output-string))
    (write-string \"held\" port)
    (list (add1 1) (kept) (force promise) (point-x pt))";

/// Whether `interp`'s heap is gone once `interp` is dropped, after
/// running [`HOLDERS`].
fn freed_on_drop<B: Backend>(interp: Interpreter<B>) -> bool {
    let value = interp
        .eval_program_owned(HOLDERS)
        .expect("the program runs");
    assert_eq!(interp.display_tagged(value), "(2 kept 2 1)");
    let heap = Rc::downgrade(interp.heap());
    drop(interp);
    heap.upgrade().is_none()
}

#[test]
fn a_dropped_interpreter_frees_its_heap() {
    assert!(freed_on_drop(vm_interpreter()), "the VM's heap outlived it");
    assert!(
        freed_on_drop(tree_walker_interpreter()),
        "the tree-walker's heap outlived it"
    );
}

/// Set when this test binary runs itself as the embedder that drops its
/// interpreters, naming the directory to write into.
const EMBEDDER: &str = "PATINA_TEARDOWN_EMBEDDER";

/// A host that writes through a file port a global keeps, drops its
/// interpreter and returns from `main` finds the output in the file. The host
/// is this test binary run again in a child process, so that nothing but the
/// drop can write the port out: when a process ends, a buffer that was never
/// dropped is never written. The CLI flushes every open port at exit
/// (`end_process`), and a host does not go through it.
#[test]
fn a_dropped_interpreter_writes_out_the_file_ports_it_left_open() {
    let program = |path: &Path| {
        format!(
            "(import (scheme base) (scheme file))
             (define port (open-output-file {path:?}))
             (write-string \"hello\" port)"
        )
    };
    if let Some(dir) = std::env::var_os(EMBEDDER) {
        let dir = Path::new(&dir);
        vm_interpreter()
            .eval_program_owned(&program(&dir.join("vm")))
            .expect("the VM's program runs");
        tree_walker_interpreter()
            .eval_program_owned(&program(&dir.join("tree-walker")))
            .expect("the tree-walker's program runs");
        return;
    }
    let dir = tempfile::tempdir().expect("a scratch directory");
    let embedder = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .env(EMBEDDER, dir.path())
        .args([
            "--exact",
            "a_dropped_interpreter_writes_out_the_file_ports_it_left_open",
            "--test-threads=1",
        ])
        .output()
        .expect("run the embedder");
    assert!(
        embedder.status.success(),
        "the embedder failed: {}\n{}{}",
        embedder.status,
        String::from_utf8_lossy(&embedder.stdout),
        String::from_utf8_lossy(&embedder.stderr)
    );
    for backend in ["vm", "tree-walker"] {
        let written = std::fs::read_to_string(dir.path().join(backend)).unwrap_or_default();
        assert_eq!(written, "hello", "the {backend}'s port was not written out");
    }
}

/// The contract teardown leaves: values do not outlive their interpreter. One
/// kept past it names a freed slot, which a check build reports as a use
/// after free rather than reading whatever comes to occupy the slot.
#[test]
fn a_value_kept_past_its_interpreter_is_a_use_after_free() {
    if !patina_core::heap::GC_CHECK {
        return;
    }
    let interp = vm_interpreter();
    let held = interp.eval_str_owned("(list 'held 1 2)").expect("a list");
    let value = interp.raw_value(&held);
    let heap = interp.heap().clone();
    drop(interp);
    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        heap.borrow().get_pair(value);
    }));
    let message = read
        .expect_err("a value read after its interpreter was dropped")
        .downcast::<String>()
        .map(|message| *message)
        .unwrap_or_default();
    assert!(message.contains("use-after-free"), "{message}");
}

/// Teardown is safe under posting (#701): a thread posts in a tight loop to
/// the interrupt handle of each interpreter this one makes, runs a form on
/// and drops, on both backends. A post begun after the drop has returned
/// answers `false`, so the drop tore the carrier down with the heap; one
/// begun before it holds the teardown until it is done. The core's own test
/// (`heap::interrupt`) also checks that no post writes the freed memory.
#[test]
fn interpreters_are_torn_down_safely_while_a_thread_posts_to_them() {
    use patina_core::{InterruptHandle, event};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};

    type Posted = (InterruptHandle, Arc<AtomicBool>);

    fn round<B: Backend>(interp: Interpreter<B>, send: &mpsc::Sender<Posted>) {
        let value = interp.eval_str_owned("(+ 1 2)").expect("the form runs");
        assert_eq!(interp.display_tagged(value), "3");
        let dropped = Arc::new(AtomicBool::new(false));
        let handle = interp.heap().borrow().interrupt_handle();
        send.send((handle, Arc::clone(&dropped))).unwrap();
        drop(interp);
        dropped.store(true, Ordering::SeqCst);
    }

    let (send, receive) = mpsc::channel::<Posted>();
    let done = Arc::new(AtomicBool::new(false));
    let poster = {
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            let mut handles = Vec::new();
            let (mut reached, mut late) = (0usize, 0usize);
            while !done.load(Ordering::SeqCst) {
                while let Ok(next) = receive.try_recv() {
                    handles.push(next);
                }
                for (handle, dropped) in &handles {
                    let after_drop = dropped.load(Ordering::SeqCst);
                    let posted = handle.post(event::SIGNAL);
                    assert!(
                        !(after_drop && posted),
                        "a post begun after the interpreter's drop reached its carrier"
                    );
                    reached += usize::from(posted);
                    late += usize::from(after_drop);
                }
                if handles.len() > 4 {
                    handles.drain(..handles.len() - 4);
                }
            }
            (reached, late)
        })
    };
    for _ in 0..16 {
        round(vm_interpreter(), &send);
        round(tree_walker_interpreter(), &send);
    }
    done.store(true, Ordering::SeqCst);
    let (reached, late) = poster.join().unwrap();
    assert!(
        reached > 0 && late > 0,
        "the poster reached {reached} carriers and posted {late} times after a drop"
    );
}
