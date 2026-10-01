use patina_core::*;
use std::rc::Rc;
use std::time::Instant;
struct Roots(Vec<TaggedValue>);
impl GcRoots for Roots { fn trace_roots(&self, v: &mut GcVisitor<'_>) { v.visit_slice(&self.0); } }
fn main() {
    let shared = new_shared_heap();
    let env = Rc::new(environment::Environment::with_heap(shared.clone()));
    let n = 1_000_000;
    {
        let mut h = shared.borrow_mut();
        for i in 0..n { h.alloc_vm_closure(1, vec![TaggedValue::fixnum(i), TaggedValue::fixnum(i)], env.clone()); }
    }
    let mut h = shared.borrow_mut();
    let mut c = MarkSweepCollector::new();
    let t = Instant::now();
    let st = c.collect(&mut h, &[&Roots(vec![])]);
    let e = t.elapsed();
    println!("sweep {} dead VmClosures (2 free vars + Rc<Env>): {:?} = {:.1} ns/object", st.last_swept.objects, e, e.as_nanos() as f64 / n as f64);
    // all-free arena re-collect: pure arena walk cost
    let t = Instant::now();
    c.collect(&mut h, &[&Roots(vec![])]);
    let e = t.elapsed();
    println!("re-collect over {} free object slots: {:?} = {:.2} ns/slot", n, e, e.as_nanos() as f64 / n as f64);
    // live closures: mark cost incl. visit_env dedup per closure
    drop(h);
    let mut keep = Vec::new();
    {
        let mut h = shared.borrow_mut();
        for i in 0..n { keep.push(h.alloc_vm_closure(1, vec![TaggedValue::fixnum(i)], env.clone())); }
    }
    let mut h = shared.borrow_mut();
    let t = Instant::now();
    let st = c.collect(&mut h, &[&Roots(keep.clone())]);
    let e = t.elapsed();
    println!("collect {} live VmClosures: {:?} = {:.1} ns/object", st.last_marked.objects, e, e.as_nanos() as f64 / n as f64);
}
