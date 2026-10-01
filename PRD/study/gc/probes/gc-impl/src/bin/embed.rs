fn main() {
    let interp = patina_interpreter::Interpreter::new_vm();
    interp.eval_str("(import (scheme base) (patina debug))").unwrap();
    let tv = interp.eval_str("(list (vector 1 2) (vector 3 4) \"str\")").unwrap();
    println!("before: {}", interp.display_tagged(tv));
    interp.eval_str("(gc)").unwrap();
    interp.eval_str("(define junk (let loop ((i 0) (acc '())) (if (= i 1000) acc (loop (+ i 1) (cons (vector i) acc)))))").unwrap();
    println!("after a collection + reuse: {}", interp.display_tagged(tv));
}
