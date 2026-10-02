use patina_interpreter::VmInterpreter;
use std::rc::Rc;
fn main() {
    let weak;
    {
        let interp = VmInterpreter::new_vm();
        interp.backend().add_library_search_path(std::path::PathBuf::from("lib")); // the repository's lib/: run from the repository root
        interp.eval_program("(import (scheme base)) (define (f x) (lambda () x)) (define g (f 1))").unwrap();
        let heap = interp.global_env().heap().clone();
        weak = Rc::downgrade(&heap);
        println!("strong count while live: {}", Rc::strong_count(&heap));
    }
    println!("heap alive after interpreter dropped: {} (strong={})", weak.upgrade().is_some(), weak.strong_count());
}
