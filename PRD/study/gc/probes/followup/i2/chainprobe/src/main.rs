use patina_core::TaggedValue;
use patina_interpreter::Interpreter;

fn walk(heap: &patina_core::heap::Heap, v: TaggedValue, depth: usize, out: &mut Vec<(String, usize, usize)>) {
    if v.is_pair() {
        let chain = heap
            .source(v)
            .and_then(|l| l.span.as_ref())
            .and_then(|s| s.expansion_chain.as_ref())
            .map(|c| (c.len(), c.iter().map(|s| s.capacity() + 24).sum::<usize>()))
            .unwrap_or((0, 0));
        out.push((format!("{}pair@{}", " ".repeat(depth), v.raw_bits() & 0xffff), chain.0, chain.1));
        let (a, d) = heap.get_pair(v);
        walk(heap, a, depth + 1, out);
        walk(heap, d, depth, out);
    }
}

fn main() {
    let n: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let tw = std::env::args().nth(2).is_some();
    let prog = format!(
        "(import (scheme base) (scheme eval) (scheme repl))
(define form '(case 3 ((1 2) 'a) ((3) 'b) (else 'c)))
(let loop ((i 0)) (when (< i {n}) (eval form (interaction-environment)) (loop (+ i 1))))"
    );
    macro_rules! go {
        ($interp:expr) => {{
            let interp = $interp;
            interp.eval_program_with_source_name(&prog, "probe.scm").0.unwrap();
            let form = interp.eval_str("form").unwrap();
            let heap = interp.global_env().heap().clone();
            let heap = heap.borrow();
            let mut out = vec![];
            walk(&heap, form, 0, &mut out);
            let total: usize = out.iter().map(|x| x.1).sum();
            let bytes: usize = out.iter().map(|x| x.2).sum();
            for (name, len, _) in &out { if *len > 0 { println!("{name} chain len {len}"); } }
            println!("pairs {} total chain entries {} approx string bytes {}", out.len(), total, bytes);
        }};
    }
    if tw { go!(Interpreter::new_tree_walker()) } else { go!(Interpreter::new_vm()) }
}
