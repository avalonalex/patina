use patina_core::*;
struct Roots(Vec<TaggedValue>);
impl GcRoots for Roots { fn trace_roots(&self, v: &mut GcVisitor<'_>) { v.visit_slice(&self.0); } }
fn maxrss() -> i64 { unsafe { let mut u: libc_rusage = std::mem::zeroed(); getrusage(0, &mut u); u.ru_maxrss / 1024 / 1024 } }
#[repr(C)] struct libc_rusage { ru_utime: [i64;2], ru_stime: [i64;2], ru_maxrss: i64, rest: [i64; 13] }
unsafe extern "C" { fn getrusage(who: i32, r: *mut libc_rusage) -> i32; }
fn main() {
    let car_first = std::env::args().nth(1).as_deref() == Some("vec");
    let mut heap = Heap::new();
    let mut list = TaggedValue::NULL;
    for i in 0..2_000_000 {
        let car = if car_first { heap.alloc_vector(vec![TaggedValue::fixnum(i), TaggedValue::fixnum(i)]) } else { TaggedValue::fixnum(i) };
        list = heap.alloc_pair(car, list);
    }
    println!("maxrss before collect: {} MB", maxrss());
    let mut c = MarkSweepCollector::new();
    let t = std::time::Instant::now();
    c.collect(&mut heap, &[&Roots(vec![list])]);
    println!("collect {:?}; maxrss after collect: {} MB", t.elapsed(), maxrss());
}
