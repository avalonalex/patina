use std::hint::black_box;
use std::sync::{Arc, Mutex, Barrier};
use std::time::Instant;
const C: usize = 1 << 22;
fn arc_bench(nthreads: usize) -> f64 {
    let a = Arc::new(5u64);
    let b = Arc::new(Barrier::new(nthreads));
    let hs: Vec<_> = (0..nthreads).map(|_| { let a = a.clone(); let b = b.clone(); std::thread::spawn(move || {
        b.wait(); let t = Instant::now(); let mut s = 0u64; for _ in 0..C { let c = black_box(a.clone()); s = s.wrapping_add(*c); } black_box(s); t.elapsed().as_secs_f64() }) }).collect();
    let ts: Vec<f64> = hs.into_iter().map(|h| h.join().unwrap()).collect();
    ts.iter().cloned().fold(0.0, f64::max) * 1e9 / C as f64
}
fn mutex_bench(nthreads: usize) -> f64 {
    let m = Arc::new(Mutex::new(0u64));
    let b = Arc::new(Barrier::new(nthreads));
    let hs: Vec<_> = (0..nthreads).map(|_| { let m = m.clone(); let b = b.clone(); std::thread::spawn(move || {
        b.wait(); let t = Instant::now(); for _ in 0..C/4 { *m.lock().unwrap() += 1; } t.elapsed().as_secs_f64() }) }).collect();
    let ts: Vec<f64> = hs.into_iter().map(|h| h.join().unwrap()).collect();
    ts.iter().cloned().fold(0.0, f64::max) * 1e9 / (C/4) as f64
}
fn main() {
    for n in [1, 2, 4, 8] {
        let a = (0..3).map(|_| arc_bench(n)).fold(f64::MAX, f64::min);
        let m = (0..3).map(|_| mutex_bench(n)).fold(f64::MAX, f64::min);
        println!("threads={n}: Arc clone+drop on one shared Arc {a:7.2} ns/op/thread; one shared Mutex lock+unlock {m:7.2} ns/op/thread");
    }
}
