// Cost of a publication fence at a "funnel" store (store of a heap reference into an old object),
// after k fresh allocations (2 words each) streamed into cold memory, as a mutator would do.
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering::*, fence};
use std::time::Instant;
#[inline(always)] fn dmb_ishst() { unsafe { std::arch::asm!("dmb ishst", options(nostack, preserves_flags)); } }
fn run(k: usize, mode: u8, heap: &[AtomicU64], old: &[AtomicU64]) -> f64 {
    let total_pairs = heap.len() / 2;
    let groups = total_pairs / k;
    let mut best = f64::MAX;
    for _ in 0..7 {
        let h = black_box(heap); let o = black_box(old);
        let t = Instant::now();
        let mut ap = 0usize;
        for g in 0..groups {
            for _ in 0..k { h[ap].store(g as u64, Relaxed); h[ap + 1].store(black_box(ap) as u64, Relaxed); ap += 2; }
            let slot = &o[g & 1023];
            match mode {
                0 => slot.store(ap as u64, Relaxed),
                1 => { fence(Release); slot.store(ap as u64, Relaxed) }   // dmb ish
                2 => { dmb_ishst(); slot.store(ap as u64, Relaxed) }        // store-store only (Chez)
                _ => slot.store(ap as u64, Release),                       // stlr
            }
        }
        let e = t.elapsed().as_secs_f64();
        black_box(o[0].load(Relaxed));
        if e < best { best = e; }
    }
    best * 1e9 / groups as f64 // ns per publishing store (including its k allocations)
}
fn main() {
    let n = 1usize << 23; // 8M words = 64 MiB
    let heap: Vec<AtomicU64> = (0..n).map(|_| AtomicU64::new(0)).collect();
    let old: Vec<AtomicU64> = (0..1024).map(|_| AtomicU64::new(0)).collect();
    // Also a small, cache-resident heap (256 KiB) to model a TLAB in L2
    let small: Vec<AtomicU64> = (0..(1usize << 15)).map(|_| AtomicU64::new(0)).collect();
    for &(name, hp) in &[("64MiB stream", &heap), ("256KiB L2-resident", &small)] {
        println!("{name}");
        for k in [1usize, 4, 16, 64] {
            let base = run(k, 0, hp, &old);
            let d = run(k, 1, hp, &old);
            let st = run(k, 2, hp, &old);
            let rel = run(k, 3, hp, &old);
            println!("  k={k:>3} allocs/pub: none {base:7.2} ns | dmb ish +{:6.2} | dmb ishst +{:6.2} | stlr +{:6.2}  (ns per publishing store)", d - base, st - base, rel - base);
        }
    }
}
