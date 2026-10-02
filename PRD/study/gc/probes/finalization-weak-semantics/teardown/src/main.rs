// Does an embedder that drops an Interpreter get the output it left in an
// unclosed file port? (a) port reachable from a global, interpreter dropped;
// (b) port unreachable, (gc) run before drop; (c) the process-exit helper.
use patina_interpreter::Interpreter;
use patina_runtime::Backend;
use std::rc::Rc;

fn read(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|_| "<missing>".into())
}

fn case<B: Backend>(label: &str, dir: &str, mk: &dyn Fn() -> Interpreter<B>) {
    let path_a = format!("{dir}/teardown-a-{label}.txt");
    let weak = {
        let i = mk();
        i.eval_program(&format!(
            "(import (scheme base) (scheme file)) (define p (open-output-file \"{path_a}\")) (write-string \"hello\" p)"
        ))
        .unwrap();
        Rc::downgrade(i.global_env().heap())
    };
    println!(
        "{label} (a) global port, interpreter dropped: file={:?} heap_alive={} strong={}",
        read(&path_a),
        weak.upgrade().is_some(),
        weak.strong_count()
    );
    let path_b = format!("{dir}/teardown-b-{label}.txt");
    {
        let i = mk();
        i.eval_program(&format!(
            "(import (scheme base) (scheme file) (patina debug)) (let ((p (open-output-file \"{path_b}\"))) (write-string \"hello\" p) #f)"
        ))
        .unwrap();
        i.eval_program("(gc)").unwrap();
        i.eval_program("(cons 1 2)").unwrap();
        println!(
            "{label} (b) unreachable port after (gc), before drop: file={:?}",
            read(&path_b)
        );
    }
}

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    case("VM", &dir, &|| Interpreter::new_vm());
    case("TW", &dir, &|| Interpreter::new_tree_walker());
    if std::env::args().nth(2).as_deref() == Some("noflush") {
        println!("(c) skipped: returning from main without the exit helper");
        return;
    }
    let failures = patina_core::port::flush_open_output_files();
    println!(
        "(c) after flush_open_output_files: failures={} a-VM={:?} a-TW={:?}",
        failures.len(),
        read(&format!("{dir}/teardown-a-VM.txt")),
        read(&format!("{dir}/teardown-a-TW.txt"))
    );
}
