use patina_interpreter::VmInterpreter;
fn main() {
    let interp = VmInterpreter::new_vm();
    interp.backend().add_library_search_path(std::path::PathBuf::from("lib")); // the repository's lib/: run from the repository root
    interp.eval_program("(import (scheme base) (patina debug))").unwrap();
    // A value the host holds; nothing in Scheme references it.
    let held = interp.eval_str("(list 'host 'held 'value (vector 1 2 3) \"str\")").unwrap();
    println!("before: {}", interp.display_tagged(held));
    let heap = interp.global_env().heap().clone();
    let before = heap.borrow().gc_collections();
    // Allocate garbage and request a collection at the next safe point.
    interp.eval_program("(define (churn n acc) (if (= n 0) acc (churn (- n 1) (cons n '())))) (churn 200000 '()) (gc) (churn 10 '()) (define keep (list 'a 'b 'c 'd 'e 'f 'g 'h))").unwrap();
    let after = heap.borrow().gc_collections();
    println!("collections: {} -> {}", before, after);
    println!("after:  {}", interp.display_tagged(held));
    println!("sizes: TaggedValue={} HeapObjectData={} Heap={}", std::mem::size_of::<patina_core::TaggedValue>(), std::mem::size_of::<patina_core::heap::HeapObjectData>(), std::mem::size_of::<patina_core::Heap>());
}
