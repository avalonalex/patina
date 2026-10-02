use patina_interpreter::{Backend, Interpreter};

fn run<B: Backend>(label: &str, interp: Interpreter<B>) {
    interp.eval_program("(import (scheme base) (patina debug))").unwrap();
    let held = interp.eval_str("(list 'held (vector 1 2 3))").unwrap();
    println!("{label} before: {}", interp.display_tagged(held));
    interp
        .eval_program(
            "(define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))
             (churn 200000) (gc) (define keep (list 'a 'b 'c 'd))",
        )
        .unwrap();
    println!("{label} after:  {}", interp.display_tagged(held));
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("tw") {
        run("tree-walker", Interpreter::new_tree_walker());
    } else {
        run("vm", Interpreter::new_vm());
    }
}
