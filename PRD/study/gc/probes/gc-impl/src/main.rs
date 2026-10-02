use patina_core::*;
use patina_core::heap::HeapObjectData;
use std::mem::size_of;
use std::time::Instant;

struct Roots(Vec<TaggedValue>);
impl GcRoots for Roots {
    fn trace_roots(&self, v: &mut GcVisitor<'_>) { v.visit_slice(&self.0); }
}

fn main() {
    println!("size_of TaggedValue = {}", size_of::<TaggedValue>());
    println!("size_of (TV,TV) pair slot = {}", size_of::<(TaggedValue, TaggedValue)>());
    println!("size_of Vec<TV> vector slot = {}", size_of::<Vec<TaggedValue>>());
    println!("size_of Vec<char> string slot = {}", size_of::<Vec<char>>());
    println!("size_of HeapObjectData object slot = {}", size_of::<HeapObjectData>());
    println!("size_of Environment = {}", size_of::<environment::Environment>());
    println!("size_of CpsContinuation = {}", size_of::<continuation::CpsContinuation>());
    println!("size_of Procedure = {}", size_of::<procedure::Procedure>());
    println!("size_of patina_vm CallFrame = {}", size_of::<patina_vm::types::CallFrame>());
    println!("size_of patina_vm VmContinuation = {}", size_of::<patina_vm::types::continuation::VmContinuation>());
    println!("size_of patina_vm CodeObject = {}", size_of::<patina_vm::types::CodeObject>());

    for &n in &[100_000usize, 1_000_000, 4_000_000] {
        // live list of n pairs + n dead pairs interleaved
        let mut heap = Heap::new();
        let mut list = TaggedValue::NULL;
        for i in 0..n {
            list = heap.alloc_pair(TaggedValue::fixnum(i as i64), list);
            heap.alloc_pair(TaggedValue::fixnum(0), TaggedValue::NULL);
        }
        let roots = Roots(vec![list]);
        let mut c = MarkSweepCollector::new();
        let t = Instant::now();
        let st = c.collect(&mut heap, &[&roots]);
        let el = t.elapsed();
        println!("pairs: live={} dead={} pause={:?} ({:.2} ns/slot) marked={} swept={}", n, n, el,
            el.as_nanos() as f64 / (2*n) as f64, st.last_marked.pairs, st.last_swept.pairs);
        // second collection: all live, nothing to sweep (free list prefilled)
        let t = Instant::now();
        let _ = c.collect(&mut heap, &[&roots]);
        let el2 = t.elapsed();
        println!("   re-collect (live list, half arena free): {:?}", el2);

        // mark phase only
        let t = Instant::now();
        let marks = run_mark_phase(&heap, &[&roots]);
        let el3 = t.elapsed();
        println!("   mark only: {:?} ({:.2} ns/live pair)", el3, el3.as_nanos() as f64 / n as f64);
        drop(marks);
    }

    // vectors of 4 elements
    for &n in &[100_000usize, 1_000_000] {
        let mut heap = Heap::new();
        let mut keep = Vec::new();
        for i in 0..n {
            let v = heap.alloc_vector(vec![TaggedValue::fixnum(i as i64); 4]);
            if i % 2 == 0 { keep.push(v); }
        }
        let holder = heap.alloc_vector(keep);
        let roots = Roots(vec![holder]);
        let mut c = MarkSweepCollector::new();
        let t = Instant::now();
        let st = c.collect(&mut heap, &[&roots]);
        let el = t.elapsed();
        println!("vectors(4): n={} pause={:?} marked={} swept={}", n, el, st.last_marked.vectors, st.last_swept.vectors);
    }
    // boxed flonums (objects)
    for &n in &[100_000usize, 1_000_000] {
        let mut heap = Heap::new();
        let mut list = TaggedValue::NULL;
        for i in 0..n {
            let r = heap.alloc_real(i as f64);
            list = heap.alloc_pair(r, list);
            heap.alloc_real(0.5);
        }
        let roots = Roots(vec![list]);
        let mut c = MarkSweepCollector::new();
        let t = Instant::now();
        let st = c.collect(&mut heap, &[&roots]);
        let el = t.elapsed();
        println!("flonum objects: n={} pause={:?} marked objs={} swept objs={}", n, el, st.last_marked.objects, st.last_swept.objects);
    }
    // symbol table cost: intern many symbols, collect with no roots
    for &n in &[10_000usize, 100_000] {
        let mut heap = Heap::new();
        for i in 0..n { heap.intern_symbol(&format!("sym-{i}")); }
        let roots = Roots(vec![]);
        let mut c = MarkSweepCollector::new();
        let t = Instant::now();
        let st = c.collect(&mut heap, &[&roots]);
        println!("symbols: n={} pause={:?} swept objs={}", n, t.elapsed(), st.last_swept.objects);
    }
    // alloc throughput
    let mut heap = Heap::new();
    let t = Instant::now();
    let mut x = TaggedValue::NULL;
    for i in 0..10_000_000 { x = heap.alloc_pair(TaggedValue::fixnum(i), x); }
    let el = t.elapsed();
    println!("alloc 10M pairs (bump, growing Vec): {:?} ({:.2} ns/pair)", el, el.as_nanos() as f64/1e7);
    let mut c = MarkSweepCollector::new();
    c.collect(&mut heap, &[&Roots(vec![])]);
    let t = Instant::now();
    for i in 0..10_000_000 { x = heap.alloc_pair(TaggedValue::fixnum(i), x); }
    let el = t.elapsed();
    println!("alloc 10M pairs (free-list pop): {:?} ({:.2} ns/pair)", el, el.as_nanos() as f64/1e7);
    let t = Instant::now();
    for i in 0..1_000_000 { heap.alloc_vector(vec![TaggedValue::fixnum(i); 4]); }
    let el = t.elapsed();
    println!("alloc 1M 4-elt vectors (malloc per vector): {:?} ({:.2} ns/vec)", el, el.as_nanos() as f64/1e6);
    let t = Instant::now();
    for i in 0..1_000_000 { heap.alloc_real(i as f64); }
    let el = t.elapsed();
    println!("alloc 1M flonums (object arena): {:?} ({:.2} ns)", el, el.as_nanos() as f64/1e6);
}
