//! par-measure: the single-thread tax of the mechanisms shared-memory
//! parallelism would add to Patina, measured one mechanism at a time.
//!
//! Every kernel is `#[inline(never)]` + `#[unsafe(no_mangle)]` so its machine
//! code can be read in the emitted assembly under the same name. Every kernel
//! runs `n` operations; the harness reports ns per operation.
//!
//! Usage: par-measure [filter-substring] [--rounds N] [--target-ms M]
//!        par-measure --contention
//!        par-measure --probe        (calls the asm probes once; for asm only)

#![allow(dead_code, clippy::missing_safety_doc, clippy::new_without_default)]

use std::arch::asm;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::hint::black_box;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::atomic::{
    AtomicBool, AtomicIsize, AtomicU8, AtomicU32, AtomicU64, AtomicUsize,
    Ordering::{AcqRel, Acquire, Relaxed, Release, SeqCst},
    fence,
};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

unsafe extern "C" {
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}
const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;

// ───────────────────────────── tiny RNG ─────────────────────────────
struct XorShift(u64);
impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ───────────────────────────── baselines ─────────────────────────────
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_empty(n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n {
        s = s.wrapping_add(black_box(i));
    }
    s
}

/// 8 dependent `add`s per iteration: 8 cycles/iteration, gives the clock.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_add_chain8(n: u64) -> u64 {
    let mut x = 0u64;
    for _ in 0..n {
        unsafe {
            asm!(
                "add {0}, {0}, #1", "add {0}, {0}, #1", "add {0}, {0}, #1", "add {0}, {0}, #1",
                "add {0}, {0}, #1", "add {0}, {0}, #1", "add {0}, {0}, #1", "add {0}, {0}, #1",
                inout(reg) x, options(nomem, nostack)
            );
        }
    }
    x
}

// ───────────────────────────── A. refcounts ─────────────────────────────
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_rc_base(r: &Rc<u64>, n: u64) -> u64 {
    let mut s = 0u64;
    for _ in 0..n {
        let c: &Rc<u64> = black_box(r);
        s = s.wrapping_add(**c);
    }
    s
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_rc_clone_drop(r: &Rc<u64>, n: u64) -> u64 {
    let mut s = 0u64;
    for _ in 0..n {
        let c = black_box(Rc::clone(r));
        s = s.wrapping_add(*c);
        drop(c);
    }
    s
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_arc_clone_drop(r: &Arc<u64>, n: u64) -> u64 {
    let mut s = 0u64;
    for _ in 0..n {
        let c = black_box(Arc::clone(r));
        s = s.wrapping_add(*c);
        drop(c);
    }
    s
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_rc_clone_drop_rot64(r: &[Rc<u64>], n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n as usize {
        let c = black_box(Rc::clone(&r[i & 63]));
        s = s.wrapping_add(*c);
        drop(c);
    }
    s
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_arc_clone_drop_rot64(r: &[Arc<u64>], n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n as usize {
        let c = black_box(Arc::clone(&r[i & 63]));
        s = s.wrapping_add(*c);
        drop(c);
    }
    s
}

/// Patina's call/return shape (`call_closure_from_regs` → `push_frame`,
/// `dispatch_frame`, `Return` → `pop_frame`, `dispatch_frame`): three
/// clone/drop pairs of the code-object pointer per call+return, with an opaque
/// step between each (the dispatch loop between instructions).
pub struct Code {
    pub num_regs: u64,
    pub id: u64,
}
pub trait SharedPtr: Clone + Deref<Target = Code> {
    fn same(a: &Self, b: &Self) -> bool;
}
impl SharedPtr for Rc<Code> {
    #[inline(always)]
    fn same(a: &Self, b: &Self) -> bool {
        Rc::ptr_eq(a, b)
    }
}
impl SharedPtr for Arc<Code> {
    #[inline(always)]
    fn same(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(a, b)
    }
}
pub struct Frame<P> {
    code: P,
    pc: usize,
    base: usize,
}

#[inline(always)]
fn callret<P: SharedPtr>(codes: &[P], n: u64) -> u64 {
    let mut frames: Vec<Frame<P>> = Vec::with_capacity(16);
    frames.push(Frame { code: codes[0].clone(), pc: 0, base: 0 });
    let mut cur = codes[0].clone();
    let mut s = 0u64;
    for i in 0..n {
        // Call: code_object(id) clones the Rc, push_frame stores it.
        let callee = black_box(&codes[1]).clone();
        frames.push(Frame { code: callee, pc: 0, base: (i & 7) as usize });
        black_box(&mut frames);
        // dispatch_frame at the callee's first instruction.
        let f = frames.last_mut().unwrap();
        if !P::same(&cur, &f.code) {
            cur = f.code.clone();
        }
        f.pc += 1;
        s = s.wrapping_add(cur.num_regs);
        black_box(&mut cur);
        black_box(&mut frames);
        // Return: pop_frame drops the frame (and its code pointer).
        let fr = frames.pop().unwrap();
        s = s.wrapping_add(fr.base as u64);
        drop(fr);
        black_box(&mut frames);
        // dispatch_frame back in the caller.
        let f = frames.last_mut().unwrap();
        if !P::same(&cur, &f.code) {
            cur = f.code.clone();
        }
        f.pc += 1;
        s = s.wrapping_add(cur.num_regs);
        black_box(&mut cur);
        black_box(&mut frames);
    }
    s
}

/// Same shape with a `Copy` code reference (the redesign: code descriptors are
/// heap values, frames hold a word, no refcount).
#[inline(always)]
fn callret_word(codes: &[&Code], n: u64) -> u64 {
    struct FrameW<'a> {
        code: &'a Code,
        pc: usize,
        base: usize,
    }
    let mut frames: Vec<FrameW> = Vec::with_capacity(16);
    frames.push(FrameW { code: codes[0], pc: 0, base: 0 });
    let mut cur: &Code = codes[0];
    let mut s = 0u64;
    for i in 0..n {
        let callee: &Code = black_box(&codes[1]);
        frames.push(FrameW { code: callee, pc: 0, base: (i & 7) as usize });
        black_box(&mut frames);
        let f = frames.last_mut().unwrap();
        if !std::ptr::eq(cur, f.code) {
            cur = f.code;
        }
        f.pc += 1;
        s = s.wrapping_add(cur.num_regs);
        black_box(&mut cur);
        black_box(&mut frames);
        let fr = frames.pop().unwrap();
        s = s.wrapping_add(fr.base as u64);
        black_box(&mut frames);
        let f = frames.last_mut().unwrap();
        if !std::ptr::eq(cur, f.code) {
            cur = f.code;
        }
        f.pc += 1;
        s = s.wrapping_add(cur.num_regs);
        black_box(&mut cur);
        black_box(&mut frames);
    }
    s
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_callret_rc(codes: &[Rc<Code>], n: u64) -> u64 {
    callret(codes, n)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_callret_arc(codes: &[Arc<Code>], n: u64) -> u64 {
    callret(codes, n)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_callret_word(codes: &[&Code], n: u64) -> u64 {
    callret_word(codes, n)
}

// ───────────────────── B. Cell / plain vs atomics ─────────────────────
const SLOTS: usize = 1024;
const SMASK: usize = SLOTS - 1;

#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_cell_rmw(a: &[Cell<u64>], n: u64) -> u64 {
    for i in 0..n as usize {
        let c = black_box(&a[i & SMASK]);
        c.set(c.get().wrapping_add(1));
    }
    a[0].get()
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_relaxed_rmw(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        let c = black_box(&a[i & SMASK]);
        c.store(c.load(Relaxed).wrapping_add(1), Relaxed);
    }
    a[0].load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_acqrel_rmw(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        let c = black_box(&a[i & SMASK]);
        c.store(c.load(Acquire).wrapping_add(1), Release);
    }
    a[0].load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_seqcst_rmw(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        let c = black_box(&a[i & SMASK]);
        c.store(c.load(SeqCst).wrapping_add(1), SeqCst);
    }
    a[0].load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_fetch_add_relaxed(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        black_box(&a[i & SMASK]).fetch_add(1, Relaxed);
    }
    a[0].load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_fetch_add_acqrel(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        black_box(&a[i & SMASK]).fetch_add(1, AcqRel);
    }
    a[0].load(Relaxed)
}

// Same-slot counter: a store→load chain through memory.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_cell_counter(a: &Cell<u64>, n: u64) -> u64 {
    for _ in 0..n {
        let c = black_box(a);
        c.set(c.get().wrapping_add(1));
    }
    a.get()
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_relaxed_counter(a: &AtomicU64, n: u64) -> u64 {
    for _ in 0..n {
        let c = black_box(a);
        c.store(c.load(Relaxed).wrapping_add(1), Relaxed);
    }
    a.load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_acqrel_counter(a: &AtomicU64, n: u64) -> u64 {
    for _ in 0..n {
        let c = black_box(a);
        c.store(c.load(Acquire).wrapping_add(1), Release);
    }
    a.load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_seqcst_counter(a: &AtomicU64, n: u64) -> u64 {
    for _ in 0..n {
        let c = black_box(a);
        c.store(c.load(SeqCst).wrapping_add(1), SeqCst);
    }
    a.load(Relaxed)
}

// Pointer chase (a cdr chain): dependent loads.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_chase_plain(next: &[u64], mask: usize, n: u64) -> u64 {
    let mut i = 0usize;
    for _ in 0..n {
        i = next[i & mask] as usize;
    }
    i as u64
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_chase_relaxed(next: &[AtomicU64], mask: usize, n: u64) -> u64 {
    let mut i = 0usize;
    for _ in 0..n {
        i = next[i & mask].load(Relaxed) as usize;
    }
    i as u64
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_chase_acquire(next: &[AtomicU64], mask: usize, n: u64) -> u64 {
    let mut i = 0usize;
    for _ in 0..n {
        i = next[i & mask].load(Acquire) as usize;
    }
    i as u64
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_chase_seqcst(next: &[AtomicU64], mask: usize, n: u64) -> u64 {
    let mut i = 0usize;
    for _ in 0..n {
        i = next[i & mask].load(SeqCst) as usize;
    }
    i as u64
}

// Pair walk: sum the cars along a cdr chain (2 loads per node, `car`+`cdr`).
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_pairwalk_relaxed(p: &[AtomicU64], mask: usize, n: u64) -> u64 {
    let mut i = 0usize;
    let mut s = 0u64;
    for _ in 0..n {
        let k = (i & mask) * 2;
        s = s.wrapping_add(p[k].load(Relaxed));
        i = p[k + 1].load(Relaxed) as usize;
    }
    s ^ i as u64
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_pairwalk_acquire(p: &[AtomicU64], mask: usize, n: u64) -> u64 {
    let mut i = 0usize;
    let mut s = 0u64;
    for _ in 0..n {
        let k = (i & mask) * 2;
        s = s.wrapping_add(p[k].load(Acquire));
        i = p[k + 1].load(Acquire) as usize;
    }
    s ^ i as u64
}

// Independent loads (throughput).
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_loads_relaxed(a: &[AtomicU64], n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n as usize {
        s = s.wrapping_add(a[i & SMASK].load(Relaxed));
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_loads_acquire(a: &[AtomicU64], n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n as usize {
        s = s.wrapping_add(a[i & SMASK].load(Acquire));
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_loads_seqcst(a: &[AtomicU64], n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n as usize {
        s = s.wrapping_add(a[i & SMASK].load(SeqCst));
    }
    s
}

// Stores (throughput).
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_stores_relaxed(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        a[i & SMASK].store(i as u64, Relaxed);
    }
    a[0].load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_stores_release(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        a[i & SMASK].store(i as u64, Release);
    }
    a[0].load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_stores_fence_release(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        fence(Release);
        a[i & SMASK].store(i as u64, Relaxed);
    }
    a[0].load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_stores_dmb_ishst(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        unsafe { asm!("dmb ishst", options(nostack, preserves_flags)) };
        a[i & SMASK].store(i as u64, Relaxed);
    }
    a[0].load(Relaxed)
}

// Store then load of another slot (the RCsc vs RCpc question).
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_stld_relaxed(a: &[AtomicU64], b: &[AtomicU64], n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n as usize {
        a[i & SMASK].store(s, Relaxed);
        s = s.wrapping_add(b[(i * 7) & SMASK].load(Relaxed));
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_stld_rel_acq(a: &[AtomicU64], b: &[AtomicU64], n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n as usize {
        a[i & SMASK].store(s, Release);
        s = s.wrapping_add(b[(i * 7) & SMASK].load(Acquire));
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_stld_seqcst(a: &[AtomicU64], b: &[AtomicU64], n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n as usize {
        a[i & SMASK].store(s, SeqCst);
        s = s.wrapping_add(b[(i * 7) & SMASK].load(SeqCst));
    }
    s
}

// Bulk copy: what atomics forbid (memcpy, vectorization).
const VLEN: usize = 1024;
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_copy_plain(dst: &mut [u64], src: &[u64], n: u64) -> u64 {
    for _ in 0..n {
        dst[..VLEN].copy_from_slice(&src[..VLEN]);
        black_box(&mut *dst);
    }
    dst[1]
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_copy_relaxed(dst: &[AtomicU64], src: &[AtomicU64], n: u64) -> u64 {
    for _ in 0..n {
        for (d, s) in dst[..VLEN].iter().zip(&src[..VLEN]) {
            d.store(s.load(Relaxed), Relaxed);
        }
        black_box(dst);
    }
    dst[1].load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_fill_plain(dst: &mut [u64], n: u64) -> u64 {
    for i in 0..n {
        dst[..VLEN].fill(i);
        black_box(&mut *dst);
    }
    dst[1]
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_fill_relaxed(dst: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n {
        for d in &dst[..VLEN] {
            d.store(i, Relaxed);
        }
        black_box(dst);
    }
    dst[1].load(Relaxed)
}

// ───────────────────── C. RefCell vs locks vs atomics ─────────────────────
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_lock_none(c: &Cell<u64>, n: u64) -> u64 {
    let mut s = 0u64;
    for _ in 0..n {
        s = s.wrapping_add(*black_box(&c.get()));
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_refcell_borrow(c: &RefCell<u64>, n: u64) -> u64 {
    let mut s = 0u64;
    for _ in 0..n {
        let g = c.borrow();
        s = s.wrapping_add(*black_box(&*g));
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_refcell_borrow_mut(c: &RefCell<u64>, n: u64) -> u64 {
    for _ in 0..n {
        let mut g = c.borrow_mut();
        *g = g.wrapping_add(1);
        black_box(&mut *g);
    }
    *c.borrow()
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_std_mutex(m: &Mutex<u64>, n: u64) -> u64 {
    for _ in 0..n {
        let mut g = m.lock().unwrap();
        *g = g.wrapping_add(1);
        black_box(&mut *g);
    }
    *m.lock().unwrap()
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_pl_mutex(m: &parking_lot::Mutex<u64>, n: u64) -> u64 {
    for _ in 0..n {
        let mut g = m.lock();
        *g = g.wrapping_add(1);
        black_box(&mut *g);
    }
    *m.lock()
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_std_rwlock_read(m: &RwLock<u64>, n: u64) -> u64 {
    let mut s = 0u64;
    for _ in 0..n {
        let g = m.read().unwrap();
        s = s.wrapping_add(*black_box(&*g));
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_pl_rwlock_read(m: &parking_lot::RwLock<u64>, n: u64) -> u64 {
    let mut s = 0u64;
    for _ in 0..n {
        let g = m.read();
        s = s.wrapping_add(*black_box(&*g));
    }
    s
}
pub struct SpinLock {
    locked: AtomicBool,
    val: Cell<u64>,
}
unsafe impl Sync for SpinLock {}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_spinlock(l: &SpinLock, n: u64) -> u64 {
    for _ in 0..n {
        while l
            .locked
            .compare_exchange_weak(false, true, Acquire, Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        l.val.set(l.val.get().wrapping_add(1));
        black_box(&l.val);
        l.locked.store(false, Release);
    }
    l.val.get()
}
/// A RefCell-shaped shared borrow made of atomics (what a `Sync` RefCell
/// would cost): fetch_add(Acquire) to enter, fetch_sub(Release) to leave.
pub struct AtomicBorrow {
    flag: AtomicIsize,
    val: Cell<u64>,
}
unsafe impl Sync for AtomicBorrow {}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_atomic_borrow(c: &AtomicBorrow, n: u64) -> u64 {
    let mut s = 0u64;
    for _ in 0..n {
        let prev = c.flag.fetch_add(1, Acquire);
        if prev < 0 {
            panic!("writer");
        }
        s = s.wrapping_add(*black_box(&c.val.get()));
        c.flag.fetch_sub(1, Release);
    }
    s
}

// ───────────────────── D. bump allocation ± publication fence ─────────────────────
const BLOCK: usize = 32 * 1024;
const AREA: usize = 4 * 1024 * 1024;
pub struct Area {
    buf: Vec<u64>,
    base: usize,
    ap: usize,
    limit: usize,
    blk: usize,
    pool: Mutex<VecDeque<usize>>,
    refills: u64,
}
impl Area {
    pub fn new() -> Area {
        let buf = vec![0u64; AREA / 8];
        let base = buf.as_ptr() as usize;
        let nblocks = AREA / BLOCK;
        let pool = Mutex::new((1..nblocks).collect::<VecDeque<usize>>());
        Area { buf, base, ap: base, limit: base + BLOCK, blk: 0, pool, refills: 0 }
    }
}
#[cold]
#[inline(never)]
fn refill_free(a: &mut Area) -> *mut u64 {
    a.blk = (a.blk + 1) % (AREA / BLOCK);
    a.ap = a.base + a.blk * BLOCK;
    a.limit = a.ap + BLOCK;
    a.refills += 1;
    let p = a.ap;
    a.ap += 16;
    p as *mut u64
}
#[cold]
#[inline(never)]
fn refill_locked(a: &mut Area) -> *mut u64 {
    {
        let mut g = a.pool.lock().unwrap();
        g.push_back(a.blk);
        a.blk = g.pop_front().unwrap();
    }
    a.ap = a.base + a.blk * BLOCK;
    a.limit = a.ap + BLOCK;
    a.refills += 1;
    let p = a.ap;
    a.ap += 16;
    p as *mut u64
}
#[inline(always)]
fn bump(a: &mut Area) -> *mut u64 {
    let p = a.ap;
    let np = p + 16;
    if np > a.limit {
        return refill_free(a);
    }
    a.ap = np;
    p as *mut u64
}
#[inline(always)]
fn bump_locked(a: &mut Area) -> *mut u64 {
    let p = a.ap;
    let np = p + 16;
    if np > a.limit {
        return refill_locked(a);
    }
    a.ap = np;
    p as *mut u64
}

macro_rules! alloc_kernel {
    ($name:ident, $bump:ident, $pre:block) => {
        #[unsafe(no_mangle)]
        #[inline(never)]
        pub fn $name(a: &mut Area, regs: &mut [u64; 16], n: u64) -> u64 {
            for i in 0..n as usize {
                let p = $bump(a);
                unsafe {
                    p.write((i as u64) << 3);
                    p.add(1).write(regs[i.wrapping_sub(1) & 15]);
                }
                $pre
                regs[i & 15] = p as u64 | 4;
            }
            regs[0]
        }
    };
}
alloc_kernel!(k_alloc_plain, bump, {});
alloc_kernel!(k_alloc_fence_release, bump, { fence(Release); });
alloc_kernel!(k_alloc_dmb_ishst, bump, {
    unsafe { asm!("dmb ishst", options(nostack, preserves_flags)) };
});
alloc_kernel!(k_alloc_fence_seqcst, bump, { fence(SeqCst); });
alloc_kernel!(k_alloc_locked_refill, bump_locked, {});

/// Publication by storing the new pair into the previous pair's cdr (a list
/// built by `set-cdr!`): relaxed store vs release store (`stlr`).
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_alloc_publish_relaxed(a: &mut Area, regs: &mut [u64; 16], n: u64) -> u64 {
    let mut prev = bump(a);
    for i in 0..n as usize {
        let p = bump(a);
        unsafe {
            p.write((i as u64) << 3);
            p.add(1).write(0x11);
            AtomicU64::from_ptr(prev.add(1)).store(p as u64 | 4, Relaxed);
        }
        prev = p;
    }
    regs[0] = prev as u64;
    regs[0]
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_alloc_publish_release(a: &mut Area, regs: &mut [u64; 16], n: u64) -> u64 {
    let mut prev = bump(a);
    for i in 0..n as usize {
        let p = bump(a);
        unsafe {
            p.write((i as u64) << 3);
            p.add(1).write(0x11);
            AtomicU64::from_ptr(prev.add(1)).store(p as u64 | 4, Release);
        }
        prev = p;
    }
    regs[0] = prev as u64;
    regs[0]
}
/// JIT allocation group: 4 objects initialised, then one `dmb ishst`.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_alloc_group4_plain(a: &mut Area, regs: &mut [u64; 16], n: u64) -> u64 {
    for i in (0..n as usize).step_by(4) {
        for j in 0..4 {
            let p = bump(a);
            unsafe {
                p.write(((i + j) as u64) << 3);
                p.add(1).write(regs[(i + j).wrapping_sub(1) & 15]);
            }
            regs[(i + j) & 15] = p as u64 | 4;
        }
    }
    regs[0]
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_alloc_group4_ishst(a: &mut Area, regs: &mut [u64; 16], n: u64) -> u64 {
    for i in (0..n as usize).step_by(4) {
        for j in 0..4 {
            let p = bump(a);
            unsafe {
                p.write(((i + j) as u64) << 3);
                p.add(1).write(regs[(i + j).wrapping_sub(1) & 15]);
            }
            regs[(i + j) & 15] = p as u64 | 4;
        }
        unsafe { asm!("dmb ishst", options(nostack, preserves_flags)) };
    }
    regs[0]
}

// ───────────────────── E. mark bits ─────────────────────
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_mark_plain(meta: &mut [u8], idx: &[u32], n: u64) -> u64 {
    let m = idx.len() - 1;
    for i in 0..n as usize {
        let k = idx[i & m] as usize;
        meta[k] |= 1;
    }
    meta[0] as u64
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_mark_fetch_or_relaxed(meta: &[AtomicU8], idx: &[u32], n: u64) -> u64 {
    let m = idx.len() - 1;
    for i in 0..n as usize {
        let k = idx[i & m] as usize;
        meta[k].fetch_or(1, Relaxed);
    }
    meta[0].load(Relaxed) as u64
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_mark_fetch_or_acqrel(meta: &[AtomicU8], idx: &[u32], n: u64) -> u64 {
    let m = idx.len() - 1;
    for i in 0..n as usize {
        let k = idx[i & m] as usize;
        meta[k].fetch_or(1, AcqRel);
    }
    meta[0].load(Relaxed) as u64
}
/// Test-then-set (the usual mark loop shape): load; only if unmarked, set.
/// With the epoch flipped per call, every index is unmarked once per pass.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_mark_test_set_plain(meta: &mut [u8], idx: &[u32], epoch: u8, n: u64) -> u64 {
    let m = idx.len() - 1;
    let mut c = 0u64;
    for i in 0..n as usize {
        let k = idx[i & m] as usize;
        if meta[k] != epoch {
            meta[k] = epoch;
            c += 1;
        }
    }
    c
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_mark_test_cas(meta: &[AtomicU8], idx: &[u32], epoch: u8, n: u64) -> u64 {
    let m = idx.len() - 1;
    let mut c = 0u64;
    for i in 0..n as usize {
        let k = idx[i & m] as usize;
        let old = meta[k].load(Relaxed);
        if old != epoch && meta[k].compare_exchange(old, epoch, Relaxed, Relaxed).is_ok() {
            c += 1;
        }
    }
    c
}

// ───────────────────── F. thread_local! vs context in a register ─────────────────────
pub struct Ctx {
    pub count: u64,
    pub ap: usize,
    pub limit: usize,
}
thread_local! {
    static TL_CONST: Cell<u64> = const { Cell::new(0) };
    static TL_LAZY: Cell<u64> = Cell::new(black_box(0));
    static TL_DROP: RefCell<Vec<u64>> = RefCell::new(vec![0]);
    static TL_CTXPTR: Cell<*mut Ctx> = const { Cell::new(std::ptr::null_mut()) };
}
static G_COUNTER: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
#[inline(never)]
pub fn op_ctx(cx: &mut Ctx) {
    cx.count = cx.count.wrapping_add(1);
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn op_tls_const() {
    TL_CONST.with(|c| c.set(c.get().wrapping_add(1)));
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn op_tls_lazy() {
    TL_LAZY.with(|c| c.set(c.get().wrapping_add(1)));
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn op_tls_drop() {
    TL_DROP.with(|c| {
        let mut v = c.borrow_mut();
        v[0] = v[0].wrapping_add(1);
    });
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn op_tls_ctxptr() {
    TL_CTXPTR.with(|c| unsafe {
        let p = c.get();
        (*p).count = (*p).count.wrapping_add(1);
    });
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn op_static_relaxed() {
    G_COUNTER.store(G_COUNTER.load(Relaxed).wrapping_add(1), Relaxed);
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_ctx(cx: &mut Ctx, n: u64) -> u64 {
    for _ in 0..n {
        op_ctx(cx);
    }
    cx.count
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_tls_const(n: u64) -> u64 {
    for _ in 0..n {
        op_tls_const();
    }
    TL_CONST.with(|c| c.get())
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_tls_lazy(n: u64) -> u64 {
    for _ in 0..n {
        op_tls_lazy();
    }
    TL_LAZY.with(|c| c.get())
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_tls_drop(n: u64) -> u64 {
    for _ in 0..n {
        op_tls_drop();
    }
    TL_DROP.with(|c| c.borrow()[0])
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_tls_ctxptr(n: u64) -> u64 {
    for _ in 0..n {
        op_tls_ctxptr();
    }
    TL_CTXPTR.with(|c| unsafe { (*c.get()).count })
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_static_relaxed(n: u64) -> u64 {
    for _ in 0..n {
        op_static_relaxed();
    }
    G_COUNTER.load(Relaxed)
}

// ───────────────────── G. intern lookup ± lock ─────────────────────
type StdMap = HashMap<String, u32>;
type FxMap = rustc_hash::FxHashMap<String, u32>;
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_intern_std(m: &StdMap, keys: &[String], order: &[u32], n: u64) -> u64 {
    let mask = order.len() - 1;
    let mut s = 0u64;
    for i in 0..n as usize {
        let k = keys[order[i & mask] as usize].as_str();
        s = s.wrapping_add(*m.get(k).unwrap() as u64);
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_intern_fx(m: &FxMap, keys: &[String], order: &[u32], n: u64) -> u64 {
    let mask = order.len() - 1;
    let mut s = 0u64;
    for i in 0..n as usize {
        let k = keys[order[i & mask] as usize].as_str();
        s = s.wrapping_add(*m.get(k).unwrap() as u64);
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_intern_std_mutex(m: &Mutex<StdMap>, keys: &[String], order: &[u32], n: u64) -> u64 {
    let mask = order.len() - 1;
    let mut s = 0u64;
    for i in 0..n as usize {
        let k = keys[order[i & mask] as usize].as_str();
        let g = m.lock().unwrap();
        s = s.wrapping_add(*g.get(k).unwrap() as u64);
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_intern_std_rwlock(m: &RwLock<StdMap>, keys: &[String], order: &[u32], n: u64) -> u64 {
    let mask = order.len() - 1;
    let mut s = 0u64;
    for i in 0..n as usize {
        let k = keys[order[i & mask] as usize].as_str();
        let g = m.read().unwrap();
        s = s.wrapping_add(*g.get(k).unwrap() as u64);
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_intern_pl_mutex(
    m: &parking_lot::Mutex<StdMap>,
    keys: &[String],
    order: &[u32],
    n: u64,
) -> u64 {
    let mask = order.len() - 1;
    let mut s = 0u64;
    for i in 0..n as usize {
        let k = keys[order[i & mask] as usize].as_str();
        let g = m.lock();
        s = s.wrapping_add(*g.get(k).unwrap() as u64);
    }
    s
}

// ───────────────────── H. fences and the poll protocol ─────────────────────
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_fence_seqcst_only(n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n {
        fence(SeqCst);
        s = s.wrapping_add(black_box(i));
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_store_fence_seqcst(a: &[AtomicU64], n: u64) -> u64 {
    for i in 0..n as usize {
        a[i & SMASK].store(i as u64, Relaxed);
        fence(SeqCst);
    }
    a[0].load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_swap_seqcst(w: &AtomicU32, n: u64) -> u64 {
    let mut s = 0u64;
    for _ in 0..n {
        s = s.wrapping_add(w.swap(0, SeqCst) as u64);
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_fetch_or_seqcst(w: &AtomicU32, n: u64) -> u64 {
    for _ in 0..n {
        w.fetch_or(1, SeqCst);
    }
    w.load(Relaxed) as u64
}
/// The poll fast path: today's `Cell<bool>` load vs the design's relaxed
/// `AtomicUsize` limit compare.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_poll_cell(flag: &Cell<bool>, n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n {
        if black_box(flag).get() {
            s = s.wrapping_add(1);
        }
        s = s.wrapping_add(i);
    }
    s
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_poll_atomic(limit: &AtomicUsize, n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n {
        if (i as usize) > black_box(limit).load(Relaxed) {
            s = s.wrapping_add(1);
        }
        s = s.wrapping_add(i);
    }
    s
}
/// `set_limit`: store, SeqCst fence, re-check event and pending.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn k_set_limit(limit: &AtomicUsize, event: &AtomicU32, n: u64) -> u64 {
    let mut s = 0u64;
    for i in 0..n {
        limit.store(i as usize | 1, Relaxed);
        fence(SeqCst);
        if event.load(Relaxed) != 0 {
            limit.store(0, Relaxed);
            s += 1;
        }
    }
    s
}


// ───────────────────── H2. barrier cost by what is pending ─────────────────────
macro_rules! dmb_kernel {
    ($name:ident, $setup:ident, $body:expr) => {
        #[unsafe(no_mangle)]
        #[inline(never)]
        pub fn $name(buf: &mut [u64], n: u64) -> u64 {
            let p = buf.as_mut_ptr();
            let len = buf.len();
            let mut acc = 0u64;
            let mut off = 0usize;
            for i in 0..n as usize {
                let $setup = (p, len, i, &mut off, &mut acc);
                $body;
            }
            acc ^ off as u64
        }
    };
}
#[inline(always)]
fn dmb_ish() {
    unsafe { asm!("dmb ish", options(nostack, preserves_flags)) }
}
#[inline(always)]
fn dmb_ishst() {
    unsafe { asm!("dmb ishst", options(nostack, preserves_flags)) }
}
// nothing pending
dmb_kernel!(x_dmb_ish_alone, st, { let _ = st; dmb_ish(); });
dmb_kernel!(x_dmb_ishst_alone, st, { let _ = st; dmb_ishst(); });
// one store to the same word, then the barrier
dmb_kernel!(x_str_same_plain, st, { let (p, _, i, _, _) = st; unsafe { p.write_volatile(i as u64) }; });
dmb_kernel!(x_str_same_ish, st, { let (p, _, i, _, _) = st; unsafe { p.write_volatile(i as u64) }; dmb_ish(); });
dmb_kernel!(x_str_same_ishst, st, { let (p, _, i, _, _) = st; unsafe { p.write_volatile(i as u64) }; dmb_ishst(); });
// sequential 16 B stores through an 8 KiB (L1) window
dmb_kernel!(x_stp_l1_plain, st, { let (p, len, i, off, _) = st; let k = (i * 2) & (1023 & (len - 1)); unsafe { p.add(k).write_volatile(i as u64); p.add(k + 1).write_volatile(i as u64) }; *off = k; });
dmb_kernel!(x_stp_l1_ish, st, { let (p, len, i, off, _) = st; let k = (i * 2) & (1023 & (len - 1)); unsafe { p.add(k).write_volatile(i as u64); p.add(k + 1).write_volatile(i as u64) }; dmb_ish(); *off = k; });
dmb_kernel!(x_stp_l1_ishst, st, { let (p, len, i, off, _) = st; let k = (i * 2) & (1023 & (len - 1)); unsafe { p.add(k).write_volatile(i as u64); p.add(k + 1).write_volatile(i as u64) }; dmb_ishst(); *off = k; });
// sequential 16 B stores through a 4 MiB window (bump allocation's pattern)
dmb_kernel!(x_stp_4m_plain, st, { let (p, len, i, off, _) = st; let k = (i * 2) & (len - 1); unsafe { p.add(k).write_volatile(i as u64); p.add(k + 1).write_volatile(i as u64) }; *off = k; });
dmb_kernel!(x_stp_4m_ish, st, { let (p, len, i, off, _) = st; let k = (i * 2) & (len - 1); unsafe { p.add(k).write_volatile(i as u64); p.add(k + 1).write_volatile(i as u64) }; dmb_ish(); *off = k; });
dmb_kernel!(x_stp_4m_ishst, st, { let (p, len, i, off, _) = st; let k = (i * 2) & (len - 1); unsafe { p.add(k).write_volatile(i as u64); p.add(k + 1).write_volatile(i as u64) }; dmb_ishst(); *off = k; });
// random 8 B stores over 4 MiB (store misses), then the barrier
dmb_kernel!(x_str_rand4m_plain, st, { let (p, len, i, off, _) = st; let k = (i.wrapping_mul(0x9E3779B1) >> 3) & (len - 1); unsafe { p.add(k).write_volatile(i as u64) }; *off ^= k; });
dmb_kernel!(x_str_rand4m_ish, st, { let (p, len, i, off, _) = st; let k = (i.wrapping_mul(0x9E3779B1) >> 3) & (len - 1); unsafe { p.add(k).write_volatile(i as u64) }; dmb_ish(); *off ^= k; });
dmb_kernel!(x_str_rand4m_ishst, st, { let (p, len, i, off, _) = st; let k = (i.wrapping_mul(0x9E3779B1) >> 3) & (len - 1); unsafe { p.add(k).write_volatile(i as u64) }; dmb_ishst(); *off ^= k; });
// a load-heavy step (8 independent L1 loads + 2 stores, ~interpreter-like) ± barrier
dmb_kernel!(x_mixed_plain, st, { let (p, _, i, off, acc) = st; let b = (i * 8) & 1023; let mut s = 0u64; for j in 0..8 { s = s.wrapping_add(unsafe { p.add((b + j * 16) & 1023).read_volatile() }); } unsafe { p.add(1024 + (i & 1023)).write_volatile(s); p.add(2048 + (i & 1023)).write_volatile(i as u64) }; *acc = acc.wrapping_add(s); *off = b; });
dmb_kernel!(x_mixed_ishst, st, { let (p, _, i, off, acc) = st; let b = (i * 8) & 1023; let mut s = 0u64; for j in 0..8 { s = s.wrapping_add(unsafe { p.add((b + j * 16) & 1023).read_volatile() }); } unsafe { p.add(1024 + (i & 1023)).write_volatile(s); p.add(2048 + (i & 1023)).write_volatile(i as u64) }; dmb_ishst(); *acc = acc.wrapping_add(s); *off = b; });
dmb_kernel!(x_mixed_ish, st, { let (p, _, i, off, acc) = st; let b = (i * 8) & 1023; let mut s = 0u64; for j in 0..8 { s = s.wrapping_add(unsafe { p.add((b + j * 16) & 1023).read_volatile() }); } unsafe { p.add(1024 + (i & 1023)).write_volatile(s); p.add(2048 + (i & 1023)).write_volatile(i as u64) }; dmb_ish(); *acc = acc.wrapping_add(s); *off = b; });
dmb_kernel!(x_mixed_stlr, st, { let (p, _, i, off, acc) = st; let b = (i * 8) & 1023; let mut s = 0u64; for j in 0..8 { s = s.wrapping_add(unsafe { p.add((b + j * 16) & 1023).read_volatile() }); } unsafe { p.add(1024 + (i & 1023)).write_volatile(s); AtomicU64::from_ptr(p.add(2048 + (i & 1023))).store(i as u64, Release) }; *acc = acc.wrapping_add(s); *off = b; });

// ───────────────────── asm probes (one operation each) ─────────────────────
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_cell_get(c: &Cell<u64>) -> u64 {
    c.get()
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_cell_set(c: &Cell<u64>, v: u64) {
    c.set(v)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_relaxed_load(c: &AtomicU64) -> u64 {
    c.load(Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_relaxed_store(c: &AtomicU64, v: u64) {
    c.store(v, Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_acquire_load(c: &AtomicU64) -> u64 {
    c.load(Acquire)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_release_store(c: &AtomicU64, v: u64) {
    c.store(v, Release)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_seqcst_load(c: &AtomicU64) -> u64 {
    c.load(SeqCst)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_seqcst_store(c: &AtomicU64, v: u64) {
    c.store(v, SeqCst)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_u32_relaxed_store(c: &AtomicU32, v: u32) {
    c.store(v, Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_u8_relaxed_store(c: &AtomicU8, v: u8) {
    c.store(v, Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_fetch_add_relaxed(c: &AtomicU64) -> u64 {
    c.fetch_add(1, Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_fetch_add_acqrel(c: &AtomicU64) -> u64 {
    c.fetch_add(1, AcqRel)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_fetch_or_u8_relaxed(c: &AtomicU8) -> u8 {
    c.fetch_or(0x40, Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_fetch_and_u8_relaxed(c: &AtomicU8) -> u8 {
    c.fetch_and(!0x40, Relaxed)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_cas_u8_relaxed(c: &AtomicU8, old: u8, new: u8) -> bool {
    c.compare_exchange(old, new, Relaxed, Relaxed).is_ok()
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_swap_seqcst(c: &AtomicU32) -> u32 {
    c.swap(0, SeqCst)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_fence_release() {
    fence(Release)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_fence_acquire() {
    fence(Acquire)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_fence_seqcst() {
    fence(SeqCst)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_rc_clone(r: &Rc<u64>) -> Rc<u64> {
    Rc::clone(r)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_rc_drop(r: Rc<u64>) {
    drop(r)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_arc_clone(r: &Arc<u64>) -> Arc<u64> {
    Arc::clone(r)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_arc_drop(r: Arc<u64>) {
    drop(r)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_refcell_read(c: &RefCell<u64>) -> u64 {
    *c.borrow()
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_tls_get() -> u64 {
    TL_CONST.with(|c| c.get())
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_ctx_get(cx: &Ctx) -> u64 {
    cx.count
}
/// The design's heap-slot read in a threaded build: acquire load of a word
/// from base + address (the `car` of a pair at `w - 4`).
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_car_relaxed(base: *const u8, w: u64) -> u64 {
    unsafe { AtomicU64::from_ptr(base.add(w as usize - 4) as *mut u64).load(Relaxed) }
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_car_acquire(base: *const u8, w: u64) -> u64 {
    unsafe { AtomicU64::from_ptr(base.add(w as usize - 4) as *mut u64).load(Acquire) }
}
/// The design's funnel store with the immediate filter and a release store
/// for heap values (the threaded variant), barrier test elided.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn probe_funnel_store_threaded(base: *const u8, slot: u64, v: u64) {
    unsafe {
        let a = AtomicU64::from_ptr(base.add(slot as usize) as *mut u64);
        if v & 0b100 != 0 {
            a.store(v, Release)
        } else {
            a.store(v, Relaxed)
        }
    }
}

// ───────────────────────────── harness ─────────────────────────────
struct Case {
    group: &'static str,
    name: &'static str,
    /// operations per unit of `n` (e.g. words per vector copy)
    per: f64,
    run: Box<dyn FnMut(u64) -> u64>,
}

fn time_ns(c: &mut Case, n: u64) -> f64 {
    let t = Instant::now();
    let r = (c.run)(n);
    black_box(r);
    t.elapsed().as_nanos() as f64
}

fn calibrate(c: &mut Case, target_ns: f64) -> u64 {
    let mut n: u64 = 256;
    loop {
        let t = time_ns(c, n);
        if t > 2e6 {
            return ((n as f64) * target_ns / t).max(1.0) as u64;
        }
        n *= 4;
    }
}

fn pct(v: &[f64], p: f64) -> f64 {
    let idx = ((v.len() - 1) as f64 * p).round() as usize;
    v[idx]
}

fn build_cases() -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    macro_rules! case {
        ($g:expr, $n:expr, $per:expr, $f:expr) => {
            cases.push(Case { group: $g, name: $n, per: $per, run: Box::new($f) });
        };
    }
    let mut rng = XorShift(0x9E3779B97F4A7C15);

    // baselines
    case!("0-base", "empty_loop_blackbox", 1.0, |n| k_empty(n));
    case!("0-base", "add_chain8 (8 cycles/iter)", 1.0, |n| k_add_chain8(n));

    // A. refcounts
    {
        let r = Rc::new(7u64);
        case!("A-refcount", "rc_base(blackbox only)", 1.0, move |n| k_rc_base(&r, n));
        let r = Rc::new(7u64);
        case!("A-refcount", "rc_clone_drop_same", 1.0, move |n| k_rc_clone_drop(&r, n));
        let r = Arc::new(7u64);
        case!("A-refcount", "arc_clone_drop_same", 1.0, move |n| k_arc_clone_drop(&r, n));
        let v: Vec<Rc<u64>> = (0..64).map(Rc::new).collect();
        case!("A-refcount", "rc_clone_drop_rot64", 1.0, move |n| k_rc_clone_drop_rot64(&v, n));
        let v: Vec<Arc<u64>> = (0..64).map(Arc::new).collect();
        case!("A-refcount", "arc_clone_drop_rot64", 1.0, move |n| k_arc_clone_drop_rot64(&v, n));
        let c: Vec<Rc<Code>> = (0..2).map(|i| Rc::new(Code { num_regs: 8 + i, id: i })).collect();
        case!("A-refcount", "callret_rc (3 pairs/call)", 1.0, move |n| k_callret_rc(&c, n));
        let c: Vec<Arc<Code>> =
            (0..2).map(|i| Arc::new(Code { num_regs: 8 + i, id: i })).collect();
        case!("A-refcount", "callret_arc (3 pairs/call)", 1.0, move |n| k_callret_arc(&c, n));
        let boxed: &'static [Code] = Box::leak(Box::new([
            Code { num_regs: 8, id: 0 },
            Code { num_regs: 9, id: 1 },
        ]));
        let refs: Vec<&'static Code> = boxed.iter().collect();
        case!("A-refcount", "callret_word (no refcount)", 1.0, move |n| k_callret_word(&refs, n));
    }

    // B. Cell/plain vs atomics
    {
        let mk = || (0..SLOTS).map(|_| AtomicU64::new(1)).collect::<Vec<_>>();
        let a: Vec<Cell<u64>> = (0..SLOTS).map(|_| Cell::new(1)).collect();
        case!("B-loadstore", "cell_rmw (ldr+str)", 1.0, move |n| k_cell_rmw(&a, n));
        let a = mk();
        case!("B-loadstore", "relaxed_rmw (ldr+str)", 1.0, move |n| k_relaxed_rmw(&a, n));
        let a = mk();
        case!("B-loadstore", "acq_rel_rmw (ldapr+stlr)", 1.0, move |n| k_acqrel_rmw(&a, n));
        let a = mk();
        case!("B-loadstore", "seqcst_rmw (ldar+stlr)", 1.0, move |n| k_seqcst_rmw(&a, n));
        let a = mk();
        case!("B-loadstore", "fetch_add_relaxed (ldadd)", 1.0, move |n| k_fetch_add_relaxed(&a, n));
        let a = mk();
        case!("B-loadstore", "fetch_add_acqrel (ldaddal)", 1.0, move |n| k_fetch_add_acqrel(&a, n));
        let c = Cell::new(0u64);
        case!("B-loadstore", "counter_cell (same slot)", 1.0, move |n| k_cell_counter(&c, n));
        let c = AtomicU64::new(0);
        case!("B-loadstore", "counter_relaxed (same slot)", 1.0, move |n| k_relaxed_counter(&c, n));
        let c = AtomicU64::new(0);
        case!("B-loadstore", "counter_acq_rel (same slot)", 1.0, move |n| k_acqrel_counter(&c, n));
        let c = AtomicU64::new(0);
        case!("B-loadstore", "counter_seqcst (same slot)", 1.0, move |n| k_seqcst_counter(&c, n));

        // pointer chase: L1-resident (4096 × 8 B = 32 KiB) and L2 (1 Mi × 8 B = 8 MiB)
        for (label_p, label_r, label_a, label_s, size) in [
            ("chase_L1_plain", "chase_L1_relaxed", "chase_L1_acquire", "chase_L1_seqcst", 4096usize),
            ("chase_2MiB_plain", "chase_2MiB_relaxed", "chase_2MiB_acquire", "chase_2MiB_seqcst", 1 << 18),
        ] {
            // Sattolo: one cycle through every element.
            let mut perm: Vec<u64> = (0..size as u64).collect();
            for i in (1..size).rev() {
                let j = rng.below(i as u64) as usize;
                perm.swap(i, j);
            }
            let mut next = vec![0u64; size];
            for i in 0..size {
                next[perm[i] as usize] = perm[(i + 1) % size];
            }
            let mask = size - 1;
            let plain = next.clone();
            case!("B-chase", label_p, 1.0, move |n| k_chase_plain(&plain, mask, n));
            let at: Vec<AtomicU64> = next.iter().map(|&x| AtomicU64::new(x)).collect();
            case!("B-chase", label_r, 1.0, move |n| k_chase_relaxed(&at, mask, n));
            let at: Vec<AtomicU64> = next.iter().map(|&x| AtomicU64::new(x)).collect();
            case!("B-chase", label_a, 1.0, move |n| k_chase_acquire(&at, mask, n));
            let at: Vec<AtomicU64> = next.iter().map(|&x| AtomicU64::new(x)).collect();
            case!("B-chase", label_s, 1.0, move |n| k_chase_seqcst(&at, mask, n));
            if size == 4096 {
                let mut pairs = Vec::with_capacity(size * 2);
                for i in 0..size {
                    pairs.push(AtomicU64::new(i as u64));
                    pairs.push(AtomicU64::new(next[i]));
                }
                let pairs2: Vec<AtomicU64> =
                    pairs.iter().map(|x| AtomicU64::new(x.load(Relaxed))).collect();
                case!("B-chase", "pairwalk_L1_relaxed (car+cdr)", 1.0, move |n| {
                    k_pairwalk_relaxed(&pairs, mask, n)
                });
                case!("B-chase", "pairwalk_L1_acquire (car+cdr)", 1.0, move |n| {
                    k_pairwalk_acquire(&pairs2, mask, n)
                });
            }
        }
        let a = mk();
        case!("B-throughput", "loads_relaxed (ldr)", 1.0, move |n| k_loads_relaxed(&a, n));
        let a = mk();
        case!("B-throughput", "loads_acquire (ldapr)", 1.0, move |n| k_loads_acquire(&a, n));
        let a = mk();
        case!("B-throughput", "loads_seqcst (ldar)", 1.0, move |n| k_loads_seqcst(&a, n));
        let a = mk();
        case!("B-throughput", "stores_relaxed (str)", 1.0, move |n| k_stores_relaxed(&a, n));
        let a = mk();
        case!("B-throughput", "stores_release (stlr)", 1.0, move |n| k_stores_release(&a, n));
        let a = mk();
        case!("B-throughput", "stores_fence_release (dmb ish+str)", 1.0, move |n| {
            k_stores_fence_release(&a, n)
        });
        let a = mk();
        case!("B-throughput", "stores_dmb_ishst (dmb ishst+str)", 1.0, move |n| {
            k_stores_dmb_ishst(&a, n)
        });
        let (a, b) = (mk(), mk());
        case!("B-throughput", "store_then_load relaxed", 1.0, move |n| k_stld_relaxed(&a, &b, n));
        let (a, b) = (mk(), mk());
        case!("B-throughput", "store_then_load stlr+ldapr", 1.0, move |n| {
            k_stld_rel_acq(&a, &b, n)
        });
        let (a, b) = (mk(), mk());
        case!("B-throughput", "store_then_load stlr+ldar (SeqCst)", 1.0, move |n| {
            k_stld_seqcst(&a, &b, n)
        });
        // bulk copy and fill, per word
        let src: Vec<u64> = (0..VLEN as u64).collect();
        let mut dst = vec![0u64; VLEN];
        case!("B-bulk", "copy_plain (memcpy) /word", VLEN as f64, move |n| {
            k_copy_plain(&mut dst, &src, n)
        });
        let src: Vec<AtomicU64> = (0..VLEN as u64).map(AtomicU64::new).collect();
        let dst: Vec<AtomicU64> = (0..VLEN).map(|_| AtomicU64::new(0)).collect();
        case!("B-bulk", "copy_relaxed_atomic /word", VLEN as f64, move |n| {
            k_copy_relaxed(&dst, &src, n)
        });
        let mut dst = vec![0u64; VLEN];
        case!("B-bulk", "fill_plain /word", VLEN as f64, move |n| k_fill_plain(&mut dst, n));
        let dst: Vec<AtomicU64> = (0..VLEN).map(|_| AtomicU64::new(0)).collect();
        case!("B-bulk", "fill_relaxed_atomic /word", VLEN as f64, move |n| {
            k_fill_relaxed(&dst, n)
        });
    }

    // C. RefCell vs locks
    {
        let c = Cell::new(3u64);
        case!("C-lock", "none (blackbox read)", 1.0, move |n| k_lock_none(&c, n));
        let c = RefCell::new(3u64);
        case!("C-lock", "refcell_borrow", 1.0, move |n| k_refcell_borrow(&c, n));
        let c = RefCell::new(3u64);
        case!("C-lock", "refcell_borrow_mut", 1.0, move |n| k_refcell_borrow_mut(&c, n));
        let c = AtomicBorrow { flag: AtomicIsize::new(0), val: Cell::new(3) };
        case!("C-lock", "atomic_borrow (ldadda+ldaddl)", 1.0, move |n| k_atomic_borrow(&c, n));
        let c = SpinLock { locked: AtomicBool::new(false), val: Cell::new(0) };
        case!("C-lock", "spinlock (casa+stlrb)", 1.0, move |n| k_spinlock(&c, n));
        let m = Mutex::new(0u64);
        case!("C-lock", "std_mutex", 1.0, move |n| k_std_mutex(&m, n));
        let m = parking_lot::Mutex::new(0u64);
        case!("C-lock", "parking_lot_mutex", 1.0, move |n| k_pl_mutex(&m, n));
        let m = RwLock::new(0u64);
        case!("C-lock", "std_rwlock_read", 1.0, move |n| k_std_rwlock_read(&m, n));
        let m = parking_lot::RwLock::new(0u64);
        case!("C-lock", "parking_lot_rwlock_read", 1.0, move |n| k_pl_rwlock_read(&m, n));
    }

    // D. allocation
    {
        macro_rules! alloc_case {
            ($label:expr, $k:ident) => {{
                let mut a = Area::new();
                let mut regs = [0u64; 16];
                case!("D-alloc", $label, 1.0, move |n| $k(&mut a, &mut regs, n));
            }};
        }
        alloc_case!("bump16_plain", k_alloc_plain);
        alloc_case!("bump16 + fence(Release) [dmb ish]", k_alloc_fence_release);
        alloc_case!("bump16 + dmb ishst", k_alloc_dmb_ishst);
        alloc_case!("bump16 + fence(SeqCst) [dmb ish]", k_alloc_fence_seqcst);
        alloc_case!("bump16, mutex block refill /32KiB", k_alloc_locked_refill);
        alloc_case!("bump16 publish via str (set-cdr!)", k_alloc_publish_relaxed);
        alloc_case!("bump16 publish via stlr (set-cdr!)", k_alloc_publish_release);
        alloc_case!("bump16 group4, no fence", k_alloc_group4_plain);
        alloc_case!("bump16 group4 + 1 dmb ishst", k_alloc_group4_ishst);
    }

    // E. mark bits
    {
        for (lbl, size) in [("1MiB", 1usize << 20), ("64MiB", 1usize << 26)] {
            let idx: Vec<u32> = (0..(1 << 16)).map(|_| rng.below(size as u64) as u32).collect();
            let idx2 = idx.clone();
            let idx3 = idx.clone();
            let idx4 = idx.clone();
            let idx5 = idx.clone();
            let mut meta = vec![0x80u8; size];
            let name: &'static str = Box::leak(format!("mark_plain_or {lbl}").into_boxed_str());
            case!("E-mark", name, 1.0, move |n| k_mark_plain(&mut meta, &idx, n));
            let meta: Vec<AtomicU8> = (0..size).map(|_| AtomicU8::new(0)).collect();
            let name: &'static str =
                Box::leak(format!("mark_fetch_or_relaxed (ldsetb) {lbl}").into_boxed_str());
            case!("E-mark", name, 1.0, move |n| k_mark_fetch_or_relaxed(&meta, &idx2, n));
            let meta: Vec<AtomicU8> = (0..size).map(|_| AtomicU8::new(0)).collect();
            let name: &'static str =
                Box::leak(format!("mark_fetch_or_acqrel (ldsetalb) {lbl}").into_boxed_str());
            case!("E-mark", name, 1.0, move |n| k_mark_fetch_or_acqrel(&meta, &idx3, n));
            let mut meta = vec![0x80u8; size];
            let mut ep = 0u8;
            let name: &'static str =
                Box::leak(format!("mark_test_set_plain {lbl}").into_boxed_str());
            case!("E-mark", name, 1.0, move |n| {
                ep = ep.wrapping_add(1) | 1;
                k_mark_test_set_plain(&mut meta, &idx4, ep, n)
            });
            let meta: Vec<AtomicU8> = (0..size).map(|_| AtomicU8::new(0)).collect();
            let mut ep = 0u8;
            let name: &'static str = Box::leak(format!("mark_test_cas {lbl}").into_boxed_str());
            case!("E-mark", name, 1.0, move |n| {
                ep = ep.wrapping_add(1) | 1;
                k_mark_test_cas(&meta, &idx5, ep, n)
            });
        }
    }

    // F. TLS vs context register
    {
        let mut cx = Box::new(Ctx { count: 0, ap: 0, limit: 0 });
        case!("F-context", "ctx_in_register (call)", 1.0, move |n| k_ctx(&mut cx, n));
        case!("F-context", "thread_local const (call)", 1.0, |n| k_tls_const(n));
        case!("F-context", "thread_local lazy (call)", 1.0, |n| k_tls_lazy(n));
        case!("F-context", "thread_local with Drop (call)", 1.0, |n| k_tls_drop(n));
        let cx: &'static mut Ctx = Box::leak(Box::new(Ctx { count: 0, ap: 0, limit: 0 }));
        TL_CTXPTR.with(|c| c.set(cx as *mut Ctx));
        case!("F-context", "thread_local *mut Ctx (call)", 1.0, |n| k_tls_ctxptr(n));
        case!("F-context", "static AtomicU64 relaxed (call)", 1.0, |n| k_static_relaxed(n));
    }

    // G. intern lookup
    {
        let words = [
            "vector", "ref", "set!", "car", "cdr", "list", "string", "append", "map", "for-each",
            "record", "make", "hash", "table", "port", "char", "lambda", "define", "syntax",
            "let*", "cond", "case", "quasi", "env", "%internal", "srfi", "comparator", "length",
        ];
        let nkeys = 10_000usize;
        let keys: Vec<String> = (0..nkeys)
            .map(|i| {
                let a = words[i % words.len()];
                let b = words[(i / words.len()) % words.len()];
                format!("{a}-{b}-{i}")
            })
            .collect();
        let order: Vec<u32> = (0..4096).map(|_| rng.below(nkeys as u64) as u32).collect();
        let std_map: StdMap = keys.iter().enumerate().map(|(i, k)| (k.clone(), i as u32)).collect();
        let fx_map: FxMap = keys.iter().enumerate().map(|(i, k)| (k.clone(), i as u32)).collect();
        let (k1, o1) = (keys.clone(), order.clone());
        case!("G-intern", "std HashMap get", 1.0, move |n| k_intern_std(&std_map, &k1, &o1, n));
        let (k1, o1) = (keys.clone(), order.clone());
        case!("G-intern", "FxHashMap get", 1.0, move |n| k_intern_fx(&fx_map, &k1, &o1, n));
        let m = Mutex::new(keys.iter().enumerate().map(|(i, k)| (k.clone(), i as u32)).collect());
        let (k1, o1) = (keys.clone(), order.clone());
        case!("G-intern", "std Mutex<HashMap> get", 1.0, move |n| {
            k_intern_std_mutex(&m, &k1, &o1, n)
        });
        let m = RwLock::new(keys.iter().enumerate().map(|(i, k)| (k.clone(), i as u32)).collect());
        let (k1, o1) = (keys.clone(), order.clone());
        case!("G-intern", "std RwLock<HashMap> read get", 1.0, move |n| {
            k_intern_std_rwlock(&m, &k1, &o1, n)
        });
        let m = parking_lot::Mutex::new(
            keys.iter().enumerate().map(|(i, k)| (k.clone(), i as u32)).collect(),
        );
        let (k1, o1) = (keys.clone(), order.clone());
        case!("G-intern", "parking_lot Mutex<HashMap> get", 1.0, move |n| {
            k_intern_pl_mutex(&m, &k1, &o1, n)
        });
    }

    // H. fences, poll protocol
    {
        case!("H-fence", "fence(SeqCst) alone [dmb ish]", 1.0, |n| k_fence_seqcst_only(n));
        let a: Vec<AtomicU64> = (0..SLOTS).map(|_| AtomicU64::new(0)).collect();
        case!("H-fence", "str + fence(SeqCst)", 1.0, move |n| k_store_fence_seqcst(&a, n));
        let w = AtomicU32::new(0);
        case!("H-fence", "swap(SeqCst) [swpal]", 1.0, move |n| k_swap_seqcst(&w, n));
        let w = AtomicU32::new(0);
        case!("H-fence", "fetch_or(SeqCst) [ldsetal]", 1.0, move |n| k_fetch_or_seqcst(&w, n));
        let f = Cell::new(false);
        case!("H-fence", "poll Cell<bool> (today)", 1.0, move |n| k_poll_cell(&f, n));
        let l = AtomicUsize::new(usize::MAX);
        case!("H-fence", "poll AtomicUsize relaxed (design)", 1.0, move |n| k_poll_atomic(&l, n));
        let (l, e) = (AtomicUsize::new(0), AtomicU32::new(0));
        case!("H-fence", "set_limit (str; dmb ish; ldr)", 1.0, move |n| k_set_limit(&l, &e, n));
    }

    // H2. barrier cost by what is pending
    {
        macro_rules! xcase {
            ($label:expr, $k:ident) => {{
                let mut buf = vec![1u64; 4 * 1024 * 1024 / 8];
                case!("H2-barrier", $label, 1.0, move |n| $k(&mut buf, n));
            }};
        }
        xcase!("dmb ish, nothing pending", x_dmb_ish_alone);
        xcase!("dmb ishst, nothing pending", x_dmb_ishst_alone);
        xcase!("str same word", x_str_same_plain);
        xcase!("str same word + dmb ish", x_str_same_ish);
        xcase!("str same word + dmb ishst", x_str_same_ishst);
        xcase!("16B seq store L1", x_stp_l1_plain);
        xcase!("16B seq store L1 + dmb ish", x_stp_l1_ish);
        xcase!("16B seq store L1 + dmb ishst", x_stp_l1_ishst);
        xcase!("16B seq store 4MiB", x_stp_4m_plain);
        xcase!("16B seq store 4MiB + dmb ish", x_stp_4m_ish);
        xcase!("16B seq store 4MiB + dmb ishst", x_stp_4m_ishst);
        xcase!("8B random store 4MiB", x_str_rand4m_plain);
        xcase!("8B random store 4MiB + dmb ish", x_str_rand4m_ish);
        xcase!("8B random store 4MiB + dmb ishst", x_str_rand4m_ishst);
        xcase!("mixed 8ld+2st", x_mixed_plain);
        xcase!("mixed 8ld+2st + dmb ishst", x_mixed_ishst);
        xcase!("mixed 8ld+2st + dmb ish", x_mixed_ish);
        xcase!("mixed 8ld+1st+1stlr", x_mixed_stlr);
    }
    cases
}


fn run_contention() {
    println!("# contention (supplement): wall ns per op per thread, median of 5");
    println!("threads\tshared_arc_clone_drop\tprivate_arc_clone_drop(adjacent allocations)\tprivate_arc_clone_drop(256B-aligned)\tshared_fetch_add\tprivate_fetch_add(padded)");
    #[repr(align(256))]
    struct A256(u64);
    #[repr(align(128))]
    struct Padded(AtomicU64);
    const M: u64 = 5_000_000;
    for t in [1usize, 2, 4, 8] {
        let mut res = [0f64; 5];
        for (which, slot) in res.iter_mut().enumerate() {
            let mut samples = Vec::new();
            for _ in 0..5 {
                let shared = Arc::new(1u64);
                let privs: Vec<Arc<u64>> = (0..t).map(|i| Arc::new(i as u64)).collect();
                let privs256: Vec<Arc<A256>> = (0..t).map(|i| Arc::new(A256(i as u64))).collect();
                let counter = AtomicU64::new(0);
                let pads: Vec<Padded> = (0..t).map(|_| Padded(AtomicU64::new(0))).collect();
                let start = Instant::now();
                std::thread::scope(|s| {
                    for k in 0..t {
                        let shared = &shared;
                        let p = &privs[k];
                        let p256 = &privs256[k];
                        let counter = &counter;
                        let pad = &pads[k];
                        s.spawn(move || {
                            unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
                            match which {
                                0 => {
                                    black_box(k_arc_clone_drop(shared, M));
                                }
                                1 => {
                                    black_box(k_arc_clone_drop(p, M));
                                }
                                2 => {
                                    for _ in 0..M {
                                        let c = black_box(Arc::clone(p256));
                                        black_box(c.0);
                                        drop(c);
                                    }
                                }
                                3 => {
                                    for _ in 0..M {
                                        black_box(counter).fetch_add(1, Relaxed);
                                    }
                                }
                                _ => {
                                    for _ in 0..M {
                                        black_box(&pad.0).fetch_add(1, Relaxed);
                                    }
                                }
                            }
                        });
                    }
                });
                samples.push(start.elapsed().as_nanos() as f64 / M as f64);
            }
            samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
            *slot = samples[2];
        }
        println!("{t}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.2}", res[0], res[1], res[2], res[3], res[4]);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    if args.iter().any(|a| a == "--probe") {
        // Never true at run time in practice; keeps the probes referenced.
        let c = Cell::new(1u64);
        let a = AtomicU64::new(1);
        let a8 = AtomicU8::new(1);
        let a32 = AtomicU32::new(1);
        let r = Rc::new(1u64);
        let ar = Arc::new(1u64);
        let rc = RefCell::new(1u64);
        let cx = Ctx { count: 1, ap: 0, limit: 0 };
        let buf = [0u64; 4];
        let base = buf.as_ptr() as *const u8;
        let mut s = probe_cell_get(&c);
        probe_cell_set(&c, 2);
        s += probe_relaxed_load(&a) + probe_acquire_load(&a) + probe_seqcst_load(&a);
        probe_relaxed_store(&a, 3);
        probe_release_store(&a, 4);
        probe_seqcst_store(&a, 5);
        probe_u32_relaxed_store(&a32, 1);
        probe_u8_relaxed_store(&a8, 1);
        s += probe_fetch_add_relaxed(&a) + probe_fetch_add_acqrel(&a);
        s += probe_fetch_or_u8_relaxed(&a8) as u64 + probe_fetch_and_u8_relaxed(&a8) as u64;
        s += probe_cas_u8_relaxed(&a8, 1, 2) as u64 + probe_swap_seqcst(&a32) as u64;
        probe_fence_release();
        probe_fence_acquire();
        probe_fence_seqcst();
        let r2 = probe_rc_clone(&r);
        probe_rc_drop(r2);
        let a2 = probe_arc_clone(&ar);
        probe_arc_drop(a2);
        s += probe_refcell_read(&rc) + probe_tls_get() + probe_ctx_get(&cx);
        s += probe_car_relaxed(base, 4 + 4) + probe_car_acquire(base, 4 + 4);
        probe_funnel_store_threaded(base, 8, 4);
        println!("{s}");
        return;
    }
    if args.iter().any(|a| a == "--contention") {
        run_contention();
        return;
    }
    let mut rounds = 15usize;
    let mut target_ms = 20.0f64;
    let mut filter: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--rounds" => {
                rounds = args[i + 1].parse().unwrap();
                i += 1;
            }
            "--target-ms" => {
                target_ms = args[i + 1].parse().unwrap();
                i += 1;
            }
            s => filter = Some(s.to_string()),
        }
        i += 1;
    }
    let mut cases = build_cases();
    if let Some(f) = &filter {
        cases.retain(|c| c.name.contains(f.as_str()) || c.group.contains(f.as_str()));
    }
    let target_ns = target_ms * 1e6;
    let ns: Vec<u64> = cases.iter_mut().map(|c| calibrate(c, target_ns)).collect();
    let mut samples: Vec<Vec<f64>> = vec![Vec::new(); cases.len()];
    for r in 0..rounds {
        // rotate the starting case each round to spread drift
        let len = cases.len();
        for k in 0..len {
            let j = (k + r * 7) % len;
            let t = time_ns(&mut cases[j], ns[j]);
            samples[j].push(t / (ns[j] as f64 * cases[j].per));
        }
    }
    // clock estimate from the add chain
    let mut ghz = f64::NAN;
    for (c, s) in cases.iter().zip(samples.iter_mut()) {
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if c.name.starts_with("add_chain8") {
            ghz = 8.0 / pct(s, 0.5);
        }
    }
    println!("# rounds={rounds} target_ms={target_ms} clock_est_GHz={ghz:.3}");
    println!("group\tcase\tmedian_ns\tmin_ns\tp10_ns\tp90_ns\tspread_pct\tmedian_cycles");
    for (c, s) in cases.iter().zip(samples.iter()) {
        let med = pct(s, 0.5);
        let spread = (pct(s, 0.9) - pct(s, 0.1)) / med * 100.0;
        println!(
            "{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.1}\t{:.1}",
            c.group,
            c.name,
            med,
            s[0],
            pct(s, 0.1),
            pct(s, 0.9),
            spread,
            med * ghz
        );
    }
}
