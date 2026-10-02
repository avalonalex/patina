use std::hint::black_box;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering::*, fence};
use std::sync::{Arc, Mutex};
use std::cell::Cell;
use std::time::Instant;

fn best<F: FnMut() -> u64>(reps: usize, mut f: F) -> f64 {
    let mut b = f64::MAX;
    for _ in 0..reps { let t = Instant::now(); black_box(f()); let e = t.elapsed().as_secs_f64(); if e < b { b = e; } }
    b
}

fn main() {
    const N: usize = 1 << 20; // 1M words = 8 MiB
    let plain: Vec<u64> = (0..N as u64).map(|i| i * 8).collect();
    let atom: Vec<AtomicU64> = (0..N as u64).map(|i| AtomicU64::new(i * 8)).collect();
    let r = 20;
    // 1. linear scan sum (vector-ref loop / equal? / GC visitor over a vector)
    let t_plain = best(r, || plain.iter().fold(0u64, |a, x| a.wrapping_add(*x)));
    let t_atom = best(r, || atom.iter().fold(0u64, |a, x| a.wrapping_add(x.load(Relaxed))));
    let t_acq = best(r, || atom.iter().fold(0u64, |a, x| a.wrapping_add(x.load(Acquire))));
    println!("scan 1M words: plain {:.3} ms, relaxed {:.3} ms ({:.2}x), acquire {:.3} ms ({:.2}x)", t_plain*1e3, t_atom*1e3, t_atom/t_plain, t_acq*1e3, t_acq/t_plain);
    // 2. fill (vector-fill!)
    let mut pv = plain.clone();
    let t_fill_p = best(r, || { for x in pv.iter_mut() { *x = black_box(17); } pv[5] });
    let t_fill_a = best(r, || { for x in atom.iter() { x.store(black_box(17), Relaxed); } 0 });
    let t_fill_r = best(r, || { for x in atom.iter() { x.store(black_box(17), Release); } 0 });
    println!("fill 1M words: plain {:.3} ms, relaxed {:.3} ms ({:.2}x), release {:.3} ms ({:.2}x)", t_fill_p*1e3, t_fill_a*1e3, t_fill_a/t_fill_p, t_fill_r*1e3, t_fill_r/t_fill_p);
    // 3. pointer chase (list traversal): next index stored in slot
    let mut perm: Vec<usize> = (0..N).collect();
    let mut s = 0x9E3779B97F4A7C15u64;
    for i in (1..N).rev() { s ^= s << 13; s ^= s >> 7; s ^= s << 17; let j = (s as usize) % (i + 1); perm.swap(i, j); }
    let mut nextp = vec![0u64; N]; for i in 0..N { nextp[perm[i]] = perm[(i + 1) % N] as u64; }
    let nexta: Vec<AtomicU64> = nextp.iter().map(|&x| AtomicU64::new(x)).collect();
    let steps = N;
    let t_ch_p = best(5, || { let mut i = 0usize; for _ in 0..steps { i = nextp[i] as usize; } i as u64 });
    let t_ch_a = best(5, || { let mut i = 0usize; for _ in 0..steps { i = nexta[i].load(Relaxed) as usize; } i as u64 });
    let t_ch_q = best(5, || { let mut i = 0usize; for _ in 0..steps { i = nexta[i].load(Acquire) as usize; } i as u64 });
    println!("pointer chase 1M (random): plain {:.2} ns/step, relaxed {:.2} ns/step, acquire {:.2} ns/step", t_ch_p*1e9/steps as f64, t_ch_a*1e9/steps as f64, t_ch_q*1e9/steps as f64);
    // 4. bump allocation + init 2 words with / without publication fence (pair cons)
    const A: usize = 1 << 22;
    let mut heap = vec![0u64; 2 * A];
    let t_alloc_p = best(r, || { let h = black_box(&mut heap); let mut ap = 0usize; for i in 0..A { h[ap] = i as u64; h[ap+1] = black_box(ap) as u64; ap += 2; } h[7] });
    let heapa: Vec<AtomicU64> = (0..2*A).map(|_| AtomicU64::new(0)).collect();
    let t_alloc_r = best(r, || { let h = black_box(&heapa); let mut ap = 0usize; for i in 0..A { h[ap].store(i as u64, Relaxed); h[ap+1].store(black_box(ap) as u64, Relaxed); ap += 2; } h[7].load(Relaxed) });
    let t_alloc_f = best(r, || { let h = black_box(&heapa); let mut ap = 0usize; for i in 0..A { h[ap].store(i as u64, Relaxed); h[ap+1].store(black_box(ap) as u64, Relaxed); fence(Release); ap += 2; } h[7].load(Relaxed) });
    // publish via release store of the new ref into an "old" slot each iteration
    let slot = AtomicU64::new(0);
    let t_alloc_pub = best(r, || { let h = black_box(&heapa); let sl = black_box(&slot); let mut ap = 0usize; for i in 0..A { h[ap].store(i as u64, Relaxed); h[ap+1].store(black_box(ap) as u64, Relaxed); sl.store(ap as u64, Release); ap += 2; } sl.load(Relaxed) });
    println!("cons+init 4M pairs: plain {:.2} ns/pair, relaxed {:.2}, relaxed+fence(Release) {:.2}, relaxed+release-store publish {:.2}", t_alloc_p*1e9/A as f64, t_alloc_r*1e9/A as f64, t_alloc_f*1e9/A as f64, t_alloc_pub*1e9/A as f64);
    // 5. metadata byte RMW: plain vs fetch_or (barrier disarm / HASHED)
    let mp = vec![Cell::new(0u8); N];
    let ma: Vec<AtomicU8> = (0..N).map(|_| AtomicU8::new(0)).collect();
    let t_m_p = best(r, || { let m = black_box(&mp); for i in 0..N { let b = &m[black_box(i)]; b.set(b.get() | (i as u8 & 0x40)); } m[3].get() as u64 });
    let t_m_lo = best(r, || { let m = black_box(&ma); for i in 0..N { let b = &m[black_box(i)]; let v = b.load(Relaxed); b.store(v | (i as u8 & 0x40), Relaxed); } m[3].load(Relaxed) as u64 });
    let t_m_a = best(r, || { let m = black_box(&ma); for i in 0..N { m[black_box(i)].fetch_or(i as u8 & 0x40, Relaxed); } m[3].load(Relaxed) as u64 });
    println!("meta byte RMW 1M: plain {:.2} ns, relaxed load/store {:.2} ns, fetch_or {:.2} ns", t_m_p*1e9/N as f64, t_m_lo*1e9/N as f64, t_m_a*1e9/N as f64);
    // 6. Rc vs Arc clone+drop (tree-walker env per call)
    let rc = Rc::new(5u64); let ar = Arc::new(5u64);
    const C: usize = 1 << 24;
    let t_rc = best(r, || { let mut s = 0u64; for _ in 0..C { let c = black_box(rc.clone()); s = s.wrapping_add(*c); } s });
    let t_arc = best(r, || { let mut s = 0u64; for _ in 0..C { let c = black_box(ar.clone()); s = s.wrapping_add(*c); } s });
    println!("clone+drop: Rc {:.2} ns, Arc {:.2} ns (uncontended)", t_rc*1e9/C as f64, t_arc*1e9/C as f64);
    // 7. uncontended mutex lock/unlock (per-port lock, interner lock)
    let m = Mutex::new(0u64);
    let t_mx = best(r, || { let m = black_box(&m); for _ in 0..C { *m.lock().unwrap() += 1; } *m.lock().unwrap() });
    let rcell = std::cell::RefCell::new(0u64);
    let t_rcell = best(r, || { let rc = black_box(&rcell); for _ in 0..C { *black_box(rc).borrow_mut() += 1; } *rc.borrow() });
    println!("uncontended lock+inc+unlock: Mutex {:.2} ns, RefCell borrow_mut {:.2} ns", t_mx*1e9/C as f64, t_rcell*1e9/C as f64);
    // 8. SeqCst fence alone
    let fa = AtomicU64::new(0);
    let t_nf = best(r, || { let f = black_box(&fa); for i in 0..C { f.store(i as u64, Relaxed); } f.load(Relaxed) });
    let t_f = best(r, || { let f = black_box(&fa); for i in 0..C { f.store(i as u64, Relaxed); fence(SeqCst); } f.load(Relaxed) });
    let t_fr = best(r, || { let f = black_box(&fa); for i in 0..C { f.store(i as u64, Relaxed); fence(Release); } f.load(Relaxed) });
    println!("store loop: no fence {:.2} ns, fence(Release) {:.2} ns, fence(SeqCst) {:.2} ns", t_nf*1e9/C as f64, t_fr*1e9/C as f64, t_f*1e9/C as f64);
}
