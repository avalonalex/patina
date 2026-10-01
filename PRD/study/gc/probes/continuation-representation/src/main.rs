//! Toy comparison of VM continuation representations (read-only study, not Patina code).
//!
//! T  = today's shape: Vec<CallFrame{.., code: Rc<Code>}> + Vec<TV> cloned at capture,
//!      per-pc retire pass on the copy, Rc<payload> in an FxHashMap side table, cloned back on invoke.
//! A  = design A: one flat heap object [hdr | frames(24 B, code as u32 id) | registers], memcpy in/out.
//! C  = design B/C hybrid: live register stack stays contiguous and mutable; capture copies only
//!      frames above a "frozen watermark" into an immutable chunk linked to older chunks; invoke
//!      grafts the chunk chain in O(1) and thaws lazily, a few frames per underflow.
use rustc_hash::FxHashMap;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

struct Counting;
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(l.size(), Relaxed);
        LIVE.fetch_add(l.size(), Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

type TV = u64;
const NULL: TV = 0x2;
const UNSPEC: TV = 0xA;

struct Code {
    num_regs: u16,
    register_roots: Vec<Vec<u64>>, // per pc, as in Patina (#423)
    _live_closures: Cell<u32>,
}

trait Machine {
    fn name(&self) -> &'static str;
    fn push(&mut self, code: usize, ret: u16);
    fn pop(&mut self);
    fn touch(&mut self, v: TV);
    fn top_r0(&mut self) -> TV;
    fn capture(&mut self, deliver: u16) -> usize;
    fn invoke(&mut self, h: usize, v: TV);
    fn drop_handle(&mut self, h: usize);
    fn depth(&self) -> usize;
}

fn make_codes() -> Vec<Rc<Code>> {
    (0..8)
        .map(|i| {
            let n = 4 + (i % 4) as u16; // 4..7 registers, like small Scheme procedures
            Rc::new(Code {
                num_regs: n,
                register_roots: (0..16).map(|pc| vec![if pc % 3 == 0 { 0b0111 } else { 0b1111_1111 }]).collect(),
                _live_closures: Cell::new(0),
            })
        })
        .collect()
}

// ───────────────────────── T: today's representation ─────────────────────────
#[derive(Clone)]
struct FrameT {
    pc: usize,
    base: usize,
    nregs: u16,
    closure: Option<u32>,
    ret: u16,
    code: Rc<Code>,
}
#[derive(Clone)]
struct WindT {
    _id: u64,
    _b: TV,
    _a: TV,
    _h: Rc<[u64]>,
}
struct ContT {
    frames: Vec<FrameT>,
    regs: Vec<TV>,
    winds: Vec<WindT>,
    prompts: Vec<[u64; 6]>,
    handlers: Vec<[u64; 2]>,
    deliver: u16,
    _reentry: Rc<[u64]>,
}
struct MT {
    codes: Vec<Rc<Code>>,
    frames: Vec<FrameT>,
    regs: Vec<TV>,
    winds: Vec<WindT>,
    prompts: Vec<[u64; 6]>,
    handlers: Vec<[u64; 2]>,
    store: FxHashMap<u64, Rc<ContT>>,
    next: u64,
    empty: Rc<[u64]>,
}
fn retire(regs: &mut [TV], frames: &[FrameT]) {
    for f in frames {
        let Some(m) = f.code.register_roots.get(f.pc) else { continue };
        for (r, v) in regs[f.base..f.base + f.nregs as usize].iter_mut().enumerate() {
            if m[r / 64] & (1 << (r % 64)) == 0 {
                *v = UNSPEC;
            }
        }
    }
}
impl Machine for MT {
    fn name(&self) -> &'static str {
        "T(today)"
    }
    fn push(&mut self, c: usize, ret: u16) {
        let code = self.codes[c].clone();
        let base = self.regs.len();
        self.regs.resize(base + code.num_regs as usize, NULL);
        self.regs[base] = self.frames.len() as TV;
        self.frames.push(FrameT { pc: 1 + c, base, nregs: code.num_regs, closure: Some(c as u32), ret, code });
    }
    fn pop(&mut self) {
        let f = self.frames.pop().unwrap();
        self.regs.truncate(f.base);
    }
    fn touch(&mut self, v: TV) {
        let b = self.frames.last().unwrap().base;
        self.regs[b + 1] = v;
    }
    fn top_r0(&mut self) -> TV {
        self.regs[self.frames.last().unwrap().base]
    }
    fn capture(&mut self, deliver: u16) -> usize {
        let b = self.frames.last().unwrap().base;
        self.regs[b + deliver as usize] = NULL; // dst clearing
        let mut c = ContT {
            frames: self.frames.clone(),
            regs: self.regs.clone(),
            winds: self.winds.clone(),
            prompts: self.prompts.clone(),
            handlers: self.handlers.clone(),
            deliver,
            _reentry: self.empty.clone(),
        };
        retire(&mut c.regs, &c.frames);
        let id = self.next;
        self.next += 1;
        self.store.insert(id, Rc::new(c));
        id as usize
    }
    fn invoke(&mut self, h: usize, v: TV) {
        let c = self.store.get(&(h as u64)).unwrap().clone();
        self.regs = c.regs.clone();
        self.frames = c.frames.clone();
        self.winds = c.winds.clone();
        self.prompts = c.prompts.clone();
        self.handlers = c.handlers.clone();
        let b = self.frames.last().unwrap().base;
        self.regs[b + c.deliver as usize] = v;
    }
    fn drop_handle(&mut self, h: usize) {
        self.store.remove(&(h as u64));
    }
    fn depth(&self) -> usize {
        self.frames.len()
    }
}

// ───────────────────────── shared 24-byte frame ─────────────────────────
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Frame {
    pc: u32,
    base: u32,
    code: u32,
    closure: u32,
    nregs: u16,
    ret: u16,
    _pad: u32,
}
const FW: usize = 3; // words per frame

struct Slots<T> {
    v: Vec<Option<T>>,
    free: Vec<usize>,
}
impl<T> Slots<T> {
    fn new() -> Self {
        Slots { v: Vec::new(), free: Vec::new() }
    }
    fn put(&mut self, t: T) -> usize {
        if let Some(i) = self.free.pop() {
            self.v[i] = Some(t);
            i
        } else {
            self.v.push(Some(t));
            self.v.len() - 1
        }
    }
    fn get(&self, i: usize) -> &T {
        self.v[i].as_ref().unwrap()
    }
    fn take(&mut self, i: usize) {
        self.v[i] = None;
        self.free.push(i);
    }
}

// ───────────────────────── A: flat traced heap object ─────────────────────────
struct MA {
    nregs: Vec<u16>,
    frames: Vec<Frame>,
    regs: Vec<TV>,
    conts: Slots<Box<[u64]>>,
}
impl Machine for MA {
    fn name(&self) -> &'static str {
        "A(flat object)"
    }
    fn push(&mut self, c: usize, ret: u16) {
        let n = self.nregs[c];
        let base = self.regs.len();
        self.regs.resize(base + n as usize, NULL);
        self.regs[base] = self.frames.len() as TV;
        self.frames.push(Frame { pc: 1 + c as u32, base: base as u32, code: c as u32, closure: c as u32, nregs: n, ret, _pad: 0 });
    }
    fn pop(&mut self) {
        let f = self.frames.pop().unwrap();
        self.regs.truncate(f.base as usize);
    }
    fn touch(&mut self, v: TV) {
        let b = self.frames.last().unwrap().base as usize;
        self.regs[b + 1] = v;
    }
    fn top_r0(&mut self) -> TV {
        self.regs[self.frames.last().unwrap().base as usize]
    }
    fn capture(&mut self, deliver: u16) -> usize {
        let b = self.frames.last().unwrap().base as usize;
        self.regs[b + deliver as usize] = NULL;
        let nf = self.frames.len();
        let nr = self.regs.len();
        // header: [size/kind, nframes, nregs | deliver]
        let mut obj = vec![0u64; 3 + FW * nf + nr].into_boxed_slice();
        obj[0] = (3 + FW * nf + nr) as u64;
        obj[1] = nf as u64;
        obj[2] = (nr as u64) | ((deliver as u64) << 48);
        let fbytes: &[u64] = unsafe { std::slice::from_raw_parts(self.frames.as_ptr() as *const u64, FW * nf) };
        obj[3..3 + FW * nf].copy_from_slice(fbytes);
        obj[3 + FW * nf..].copy_from_slice(&self.regs);
        self.conts.put(obj)
    }
    fn invoke(&mut self, h: usize, v: TV) {
        let obj = self.conts.get(h);
        let nf = obj[1] as usize;
        let nr = (obj[2] & 0xFFFF_FFFF_FFFF) as usize;
        let deliver = (obj[2] >> 48) as usize;
        let fwords = &obj[3..3 + FW * nf];
        self.frames.clear();
        self.frames.extend_from_slice(unsafe { std::slice::from_raw_parts(fwords.as_ptr() as *const Frame, nf) });
        self.regs.clear();
        self.regs.extend_from_slice(&obj[3 + FW * nf..3 + FW * nf + nr]);
        let b = self.frames.last().unwrap().base as usize;
        self.regs[b + deliver] = v;
    }
    fn drop_handle(&mut self, h: usize) {
        self.conts.take(h);
    }
    fn depth(&self) -> usize {
        self.frames.len()
    }
}

// ───────────────────────── C: watermark + immutable chunk chain + lazy thaw ─────────────────────────
struct Chunk {
    frames: Box<[Frame]>, // bases relative to regs[0]
    regs: Box<[TV]>,
    below: Option<View>,
}
#[derive(Clone)]
struct View {
    chunk: Rc<Chunk>,
    n: u32, // this view is: chunk.below ++ chunk.frames[..n]
}
fn truncate(mut v: Option<View>, mut k: usize) -> Option<View> {
    while k > 0 {
        let view = v.expect("truncate below the bottom");
        if view.n as usize > k {
            return Some(View { chunk: view.chunk, n: view.n - k as u32 });
        }
        k -= view.n as usize;
        v = view.chunk.below.clone();
    }
    v
}
struct ContC {
    view: Option<View>,
    depth: usize,
    deliver: u16,
}
struct MC {
    nregs: Vec<u16>,
    frames: Vec<Frame>,
    regs: Vec<TV>,
    below: Option<View>, // frames not on the live stack
    below_depth: usize,
    frozen: Option<View>, // a view equal to the stack [0, frozen_depth)
    frozen_depth: usize,
    wm: usize, // live frames[..wm] are unchanged since frozen
    thaw: usize,
    conts: Slots<ContC>,
    underflows: usize,
}
impl MC {
    fn resume(&mut self, i: usize) {
        if i < self.wm {
            self.wm = i;
        }
    }
    fn underflow(&mut self) {
        // Thaw up to `thaw` frames from the top chunk of `below` (one memcpy each).
        let view = self.below.clone().expect("underflow with nothing below");
        let n = view.n as usize;
        let k = self.thaw.min(n);
        let first = &view.chunk.frames[n - k];
        let src_lo = first.base as usize;
        let last = &view.chunk.frames[n - 1];
        let src_hi = last.base as usize + last.nregs as usize;
        let shift = self.regs.len() as i64 - src_lo as i64;
        self.regs.extend_from_slice(&view.chunk.regs[src_lo..src_hi]);
        for f in &view.chunk.frames[n - k..n] {
            let mut f = *f;
            f.base = (f.base as i64 + shift) as u32;
            self.frames.push(f);
        }
        self.below = truncate(Some(view), k);
        self.below_depth -= k;
        // frozen still covers [0, below_depth + k): the thawed frames equal their frozen copies
        self.wm = self.frames.len();
        self.underflows += 1;
    }
}
impl Machine for MC {
    fn name(&self) -> &'static str {
        "C(watermark+lazy thaw)"
    }
    fn push(&mut self, c: usize, ret: u16) {
        let n = self.nregs[c];
        let base = self.regs.len();
        self.regs.resize(base + n as usize, NULL);
        self.regs[base] = (self.below_depth + self.frames.len()) as TV;
        self.frames.push(Frame { pc: 1 + c as u32, base: base as u32, code: c as u32, closure: c as u32, nregs: n, ret, _pad: 0 });
    }
    fn pop(&mut self) {
        let f = self.frames.pop().unwrap();
        self.regs.truncate(f.base as usize);
        if self.frames.is_empty() {
            if self.below.is_none() {
                return;
            }
            self.underflow();
        }
        let i = self.frames.len() - 1;
        self.resume(i);
    }
    fn touch(&mut self, v: TV) {
        let i = self.frames.len() - 1;
        self.resume(i);
        let b = self.frames[i].base as usize;
        self.regs[b + 1] = v;
    }
    fn top_r0(&mut self) -> TV {
        self.regs[self.frames.last().unwrap().base as usize]
    }
    fn capture(&mut self, deliver: u16) -> usize {
        let i = self.frames.len() - 1;
        let b = self.frames[i].base as usize;
        self.regs[b + deliver as usize] = NULL;
        // Drop the stale part of `frozen` (frames popped since it was made).
        let keep = self.below_depth + self.wm;
        if self.frozen_depth > keep {
            self.frozen = truncate(self.frozen.take(), self.frozen_depth - keep);
            self.frozen_depth = keep;
        }
        if self.wm < self.frames.len() {
            let new = &self.frames[self.wm..];
            let lo = new[0].base as usize;
            let frames: Box<[Frame]> = new
                .iter()
                .map(|f| Frame { base: f.base - lo as u32, ..*f })
                .collect();
            let regs: Box<[TV]> = self.regs[lo..].into();
            let n = frames.len() as u32;
            let chunk = Rc::new(Chunk { frames, regs, below: self.frozen.take() });
            self.frozen = Some(View { chunk, n });
            self.frozen_depth = self.below_depth + self.frames.len();
            self.wm = self.frames.len();
        }
        self.conts.put(ContC { view: self.frozen.clone(), depth: self.frozen_depth, deliver })
    }
    fn invoke(&mut self, h: usize, v: TV) {
        let c = self.conts.get(h);
        let (view, depth, deliver) = (c.view.clone(), c.depth, c.deliver);
        self.frames.clear();
        self.regs.clear();
        self.below = view.clone();
        self.below_depth = depth;
        self.frozen = view;
        self.frozen_depth = depth;
        self.wm = 0;
        self.underflow();
        let i = self.frames.len() - 1;
        self.resume(i);
        let b = self.frames[i].base as usize;
        self.regs[b + deliver as usize] = v;
    }
    fn drop_handle(&mut self, h: usize) {
        self.conts.take(h);
    }
    fn depth(&self) -> usize {
        self.below_depth + self.frames.len()
    }
}

// ───────────────────────── workloads ─────────────────────────
const DST: u16 = 2;

/// cc_20000.scm's shape: recurse D, capture at the bottom, return all the way, repeat.
fn w_dead<M: Machine>(m: &mut M, d: usize, n: usize, keep: bool) -> u64 {
    let mut acc = 0;
    let mut kept = Vec::new();
    for it in 0..n {
        for j in 0..d {
            m.push(j % 8, DST);
        }
        let h = m.capture(DST);
        if keep { kept.push(h) } else { m.drop_handle(h) }
        m.touch(it as TV);
        for _ in 0..d {
            m.pop();
            m.touch(1);
        }
        acc += m.top_r0();
    }
    for h in kept {
        m.drop_handle(h);
    }
    acc
}

/// A loop at depth D capturing every iteration (samedepth1000.scm's shape).
fn w_same_depth<M: Machine>(m: &mut M, d: usize, n: usize) -> u64 {
    for j in 0..d {
        m.push(j % 8, DST);
    }
    let mut acc = 0;
    for it in 0..n {
        let h = m.capture(DST);
        m.drop_handle(h);
        m.touch(it as TV);
        acc += m.top_r0();
    }
    for _ in 0..d - 1 {
        m.pop();
    }
    acc
}

/// escape1000.scm's shape: at depth D, capture, dive 10 frames, escape back.
fn w_escape<M: Machine>(m: &mut M, d: usize, n: usize) -> u64 {
    for j in 0..d {
        m.push(j % 8, DST);
    }
    let mut acc = 0;
    for it in 0..n {
        let h = m.capture(DST);
        for j in 0..10 {
            m.push(j % 8, DST);
        }
        m.invoke(h, it as TV);
        m.drop_handle(h);
        acc += m.top_r0();
        m.touch(1);
    }
    for _ in 0..d - 1 {
        m.pop();
    }
    acc
}

/// pingpong1000.scm's shape: two coroutines over a D-deep base, switching by full continuations.
fn w_pingpong<M: Machine>(m: &mut M, d: usize, n: usize) -> u64 {
    for j in 0..d {
        m.push(j % 8, DST);
    }
    // Coroutine B starts 2 frames deeper on a copy of the base.
    m.push(1, DST);
    let mut other = m.capture(DST);
    m.touch(0);
    m.push(2, DST);
    m.push(3, DST);
    let mut acc = 0;
    for it in 0..n {
        let me = m.capture(DST);
        m.invoke(other, it as TV);
        m.drop_handle(other);
        other = me;
        acc += m.top_r0();
        m.touch(1);
    }
    m.drop_handle(other);
    while m.depth() > 1 {
        m.pop();
    }
    acc
}

/// Re-enter a D-deep continuation and return through every frame (generator finishing; lazy thaw's worst case).
fn w_reinstate_unwind<M: Machine>(m: &mut M, d: usize, n: usize) -> u64 {
    for j in 0..d {
        m.push(j % 8, DST);
    }
    let h = m.capture(DST);
    m.touch(0);
    let mut acc = 0;
    for it in 0..n {
        m.invoke(h, it as TV);
        for _ in 0..d {
            m.pop();
            m.touch(1);
        }
        acc += m.top_r0();
    }
    m.drop_handle(h);
    acc
}

fn bench<M: Machine>(mk: impl Fn() -> M, label: &str, f: impl Fn(&mut M) -> u64, ops: usize) {
    let mut best = f64::MAX;
    let mut alloc = 0;
    let mut check = 0;
    let mut name = "";
    for _ in 0..5 {
        let mut m = mk();
        m.push(0, DST); // the outer `run` frame
        name = m.name();
        let a0 = ALLOCATED.load(Relaxed);
        let t = Instant::now();
        check = black_box(f(&mut m));
        let dt = t.elapsed().as_secs_f64();
        alloc = ALLOCATED.load(Relaxed) - a0;
        best = best.min(dt);
    }
    println!(
        "{label:<28} {name:<24} {:>9.0} ns/op  {:>9.0} B allocated/op  (check {check})",
        best * 1e9 / ops as f64,
        alloc as f64 / ops as f64
    );
}

fn main() {
    let codes = make_codes();
    let nregs: Vec<u16> = codes.iter().map(|c| c.num_regs).collect();
    let mt = || MT {
        codes: codes.clone(),
        frames: Vec::new(),
        regs: Vec::new(),
        winds: Vec::new(),
        prompts: Vec::new(),
        handlers: Vec::new(),
        store: FxHashMap::default(),
        next: 0,
        empty: Rc::from(Vec::new()),
    };
    let ma = || MA { nregs: nregs.clone(), frames: Vec::new(), regs: Vec::new(), conts: Slots::new() };
    let mc = |thaw| MC {
        nregs: nregs.clone(),
        frames: Vec::new(),
        regs: Vec::new(),
        below: None,
        below_depth: 0,
        frozen: None,
        frozen_depth: 0,
        wm: 0,
        thaw,
        conts: Slots::new(),
        underflows: 0,
    };
    let n = 20_000;
    for d in [100usize, 1000] {
        let l = format!("dead capture d={d}");
        bench(mt, &l, |m| w_dead(m, d, n, false), n);
        bench(ma, &l, |m| w_dead(m, d, n, false), n);
        bench(|| mc(1), &l, |m| w_dead(m, d, n, false), n);
        let l = format!("no capture   d={d} (baseline)");
        bench(ma, &l, |m| {
            let mut acc = 0;
            for _ in 0..n {
                for j in 0..d { m.push(j % 8, DST) }
                for _ in 0..d { m.pop(); m.touch(1) }
                acc += m.top_r0();
            }
            acc
        }, n);
    }
    for d in [100usize, 1000] {
        let l = format!("no capture   d={d} (baseline)");
        bench(|| mc(1), &l, |m| {
            let mut acc = 0;
            for _ in 0..n {
                for j in 0..d { m.push(j % 8, DST) }
                for _ in 0..d { m.pop(); m.touch(1) }
                acc += m.top_r0();
            }
            acc
        }, n);
        let l = format!("reinstate+unwind d={d}");
        bench(mt, &l, |m| w_reinstate_unwind(m, d, n), n);
        bench(ma, &l, |m| w_reinstate_unwind(m, d, n), n);
        bench(|| mc(1), &l, |m| w_reinstate_unwind(m, d, n), n);
        bench(|| mc(8), &format!("{l} thaw=8"), |m| w_reinstate_unwind(m, d, n), n);
    }
    for d in [10usize, 1000] {
        let l = format!("same-depth   d={d}");
        bench(mt, &l, |m| w_same_depth(m, d, n), n);
        bench(ma, &l, |m| w_same_depth(m, d, n), n);
        bench(|| mc(1), &l, |m| w_same_depth(m, d, n), n);
    }
    for d in [10usize, 1000] {
        let l = format!("escape       d={d}");
        bench(mt, &l, |m| w_escape(m, d, n), n);
        bench(ma, &l, |m| w_escape(m, d, n), n);
        bench(|| mc(1), &l, |m| w_escape(m, d, n), n);
    }
    for d in [10usize, 1000] {
        let l = format!("ping-pong    d={d}");
        bench(mt, &l, |m| w_pingpong(m, d, 2 * n), 2 * n);
        bench(ma, &l, |m| w_pingpong(m, d, 2 * n), 2 * n);
        bench(|| mc(1), &l, |m| w_pingpong(m, d, 2 * n), 2 * n);
        bench(|| mc(4), &format!("{l} thaw=4"), |m| w_pingpong(m, d, 2 * n), 2 * n);
    }
    // Retained bytes per kept capture (what the trigger must account for).
    for d in [100usize, 1000] {
        for which in 0..3 {
            let before = LIVE.load(Relaxed);
            let k = 2000;
            let (name, live) = match which {
                0 => { let mut m = mt(); for _ in 0..d { m.push(1, DST) } let hs: Vec<_> = (0..k).map(|_| m.capture(DST)).collect(); let l = LIVE.load(Relaxed); black_box(&hs); ("T", l) }
                1 => { let mut m = ma(); for _ in 0..d { m.push(1, DST) } let hs: Vec<_> = (0..k).map(|_| m.capture(DST)).collect(); let l = LIVE.load(Relaxed); black_box(&hs); ("A", l) }
                _ => { let mut m = mc(1); for _ in 0..d { m.push(1, DST) } let hs: Vec<_> = (0..k).map(|_| { let h = m.capture(DST); m.touch(0); h }).collect(); let l = LIVE.load(Relaxed); black_box(&hs); ("C", l) }
            };
            println!("retained/capture d={d:<5} {name}: {:>8.0} B", (live - before) as f64 / k as f64);
        }
    }
    println!("sizes: FrameT={} Frame={} ContT={} View={}", std::mem::size_of::<FrameT>(), std::mem::size_of::<Frame>(), std::mem::size_of::<ContT>(), std::mem::size_of::<View>());
}
