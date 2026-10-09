#![allow(
    clippy::disallowed_methods,
    reason = "a benchmark host evaluates from outside, as an embedder does; the workspace's \
              re-entry rule (#622) is for the evaluator's own crates"
)]

//! The `many-heaps` probe of the GC benchmark set (#649): many interpreters
//! alive in one process, each with its own heap, as a test binary has them
//! (318 at most in one `patina-tests` process). It makes 300, runs a small
//! program in each and keeps them all, then prints how many it holds; the
//! benchmark mode reads the process's peak footprint. Stage 5's per-heap
//! reservations are judged by it ([K13]).
//!
//! [K13]: https://github.com/avalonalex/patina/blob/main/PRD/GC_PRD.md#k13

use patina_interpreter::{VmInterpreter, format_interpreter_error};

fn main() {
    let count = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(300);
    let mut held = Vec::with_capacity(count);
    for i in 0..count {
        let interpreter = VmInterpreter::new_vm();
        let (result, sources) = interpreter.eval_program_with_source_name_owned(
            "(import (scheme base)) \
             (define (build n acc) (if (= n 0) acc (build (- n 1) (cons n acc)))) \
             (length (build 10000 '()))",
            "many-heaps.scm",
        );
        match result {
            Ok(value) => assert_eq!(interpreter.display_tagged(&value), "10000", "heap {i}"),
            Err(error) => panic!("{}", format_interpreter_error(&error, &sources.borrow())),
        }
        held.push(interpreter);
    }
    println!("{}", held.len());
}
