use patina_interpreter::{Backend, Interpreter};

fn run<B: Backend>(new: fn() -> Interpreter<B>) {
    let a = new();
    let b = new();
    a.eval_program("(import (scheme base)) (define s (open-output-string)) (current-output-port s)")
        .unwrap();
    b.eval_program("(import (scheme base) (scheme write)) (display \"written by B\")")
        .unwrap();
    let got = a.eval_str("(get-output-string s)").unwrap();
    println!("A's string port holds {}", a.display_tagged(got)); // expected ""

    // Input, and an interpreter created afterwards.
    a.eval_program("(current-input-port (open-input-string \"(from A)\"))").unwrap();
    let read = b.eval_program("(import (scheme read)) (read)").unwrap();
    println!("B's (read) answers {}", b.display_tagged(read)); // expected (from stdin)
    let c = new();
    c.eval_program("(import (scheme base) (scheme write)) (display \" and by C\")").unwrap();
    let got = a.eval_str("(get-output-string s)").unwrap();
    println!("A's string port holds {}", a.display_tagged(got));
}

fn run_err<B: Backend>(new: fn() -> Interpreter<B>) {
    let a = new();
    let b = new();
    let _ = a.eval_program(
        "(import (scheme base))
         (define s (open-output-string))
         (parameterize ((current-output-port s)) (car 5))",
    );
    b.eval_program("(import (scheme base) (scheme write)) (display \"written by B\")")
        .unwrap();
    let got = a.eval_str("(get-output-string s)").unwrap();
    println!("after an error: A's string port holds {}", a.display_tagged(got)); // expected ""
}

fn main() {
    let tw = std::env::args().nth(1).as_deref() == Some("tw");
    let err = std::env::args().nth(2).as_deref() == Some("err");
    match (tw, err) {
        (false, false) => run(Interpreter::new_vm),
        (true, false) => run(Interpreter::new_tree_walker),
        (false, true) => run_err(Interpreter::new_vm),
        (true, true) => run_err(Interpreter::new_tree_walker),
    }
}
