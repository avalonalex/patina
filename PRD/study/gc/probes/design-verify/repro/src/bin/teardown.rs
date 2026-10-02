use patina_interpreter::{Backend, Interpreter};
use std::rc::Rc;

fn run<B: Backend>(interp: Interpreter<B>, path: &str) {
    let heap = {
        let interp = interp;
        interp
            .eval_program(&format!(
                "(import (scheme base) (scheme file))
                 (define p (open-output-file {path:?}))
                 (write-string \"hello\" p)"
            ))
            .unwrap();
        Rc::downgrade(interp.global_env().heap())
    };
    println!("after drop: heap alive = {}, strong = {}", heap.upgrade().is_some(), heap.strong_count());
}

fn main() {
    let path = std::env::args().nth(1).expect("output path");
    match std::env::args().nth(2).as_deref() {
        Some("tw") => run(Interpreter::new_tree_walker(), &path),
        _ => run(Interpreter::new_vm(), &path),
    }
}
