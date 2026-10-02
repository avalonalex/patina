use patina_interpreter::{Backend, Interpreter};

fn run<B: Backend>(new: fn() -> Interpreter<B>) {
    let a = new();
    let b = new();
    a.eval_program("(import (scheme base)) (define s (open-output-string)) (current-output-port s)")
        .unwrap();
    b.eval_program("(import (scheme base) (scheme write)) (display \"written by B\")").unwrap();
    let got = a.eval_str("(get-output-string s)").unwrap();
    println!("{}", a.display_tagged(got)); // expected "", with B's text on stdout

    a.eval_program("(current-input-port (open-input-string \"(from A)\"))").unwrap();
    let datum = b.eval_program("(import (scheme read)) (read)").unwrap();
    println!("{}", b.display_tagged(datum)); // expected the datum on standard input
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("tw") { run(Interpreter::new_tree_walker) } else { run(Interpreter::new_vm) }
}
