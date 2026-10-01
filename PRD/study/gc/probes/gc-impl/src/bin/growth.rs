use patina_core::*;
use std::time::Instant;
fn main() {
    let mut heap = Heap::new();
    let mut x = TaggedValue::NULL;
    let mut worst = Vec::new();
    for i in 0..16_000_000i64 {
        let t = Instant::now();
        x = heap.alloc_pair(TaggedValue::fixnum(i), x);
        let e = t.elapsed();
        if e.as_micros() > 20 { worst.push((i, e)); }
    }
    for (i, e) in worst { println!("alloc #{i}: {:?}", e); }
    let mut heap = Heap::new();
    let mut worst = Vec::new();
    for i in 0..4_000_000i64 {
        let t = Instant::now();
        let _ = heap.alloc_real(i as f64);
        let e = t.elapsed();
        if e.as_micros() > 20 { worst.push((i, e)); }
    }
    for (i, e) in worst { println!("alloc_real #{i}: {:?}", e); }
}
