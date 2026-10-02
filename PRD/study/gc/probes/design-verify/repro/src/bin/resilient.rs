use patina_interpreter::Interpreter;

fn main() {
    let interp = Interpreter::new_vm();
    interp
        .eval_program(
            "(import (scheme base) (patina debug))
             (define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))",
        )
        .unwrap();
    // The second form collects and then fails, so the first form's value is returned.
    let v = interp.eval_program_resilient("(list 'first 1 2) (begin (churn 200000) (gc) (car 5))");
    println!("returned: {}", interp.display_tagged(v));
}
