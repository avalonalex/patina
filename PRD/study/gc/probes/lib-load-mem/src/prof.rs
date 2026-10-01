//! A small sampling heap profiler: a counting global allocator that samples
//! one allocation per RATE bytes, captures its return-address stack with
//! libSystem's `backtrace()`, and keeps the live sampled blocks in a map so
//! that live memory can be attributed to call sites at any moment. Profiler
//! bookkeeping runs with a thread-local guard set and is excluded from all
//! counters.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed};

unsafe extern "C" {
    fn backtrace(buf: *mut *mut libc::c_void, size: libc::c_int) -> libc::c_int;
    pub fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
}

#[derive(Default)]
pub struct Fx(u64);
impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ (*b as u64)).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64)
    }
}
pub type FxBuild = BuildHasherDefault<Fx>;

pub static LIVE: AtomicI64 = AtomicI64::new(0);
pub static PEAK: AtomicI64 = AtomicI64::new(0);
pub static TOTAL: AtomicU64 = AtomicU64::new(0);
pub static COUNT: AtomicU64 = AtomicU64::new(0);
pub static ENABLED: AtomicBool = AtomicBool::new(false);
pub static RATE: AtomicU64 = AtomicU64::new(32 * 1024);
static ACC: AtomicU64 = AtomicU64::new(0);
/// Live bytes and live block counts per power-of-two size class.
pub static CLASS_BYTES: [AtomicI64; 48] = [const { AtomicI64::new(0) }; 48];
pub static CLASS_COUNT: [AtomicI64; 48] = [const { AtomicI64::new(0) }; 48];
/// Cumulative allocated bytes per class (churn).
pub static CLASS_TOTAL: [AtomicU64; 48] = [const { AtomicU64::new(0) }; 48];

thread_local! { static IN: Cell<bool> = const { Cell::new(false) }; }

pub struct State {
    pub stack_index: HashMap<u64, u32, FxBuild>,
    pub frames: Vec<usize>,
    pub stacks: Vec<(u32, u16)>,
    pub live: HashMap<usize, (u32, u64), FxBuild>,
    pub peak_snapshot: Vec<(u32, u64)>,
    pub peak_snapshot_live: i64,
    /// Cumulative sampled bytes per stack (allocation volume, not live).
    pub churn: HashMap<u32, u64, FxBuild>,
}

pub static STATE: Mutex<Option<State>> = Mutex::new(None);

fn class_of(size: usize) -> usize {
    (usize::BITS - size.max(1).leading_zeros()) as usize
}

pub fn with_guard<T>(f: impl FnOnce() -> T) -> T {
    let prev = IN.with(|g| g.replace(true));
    let r = f();
    IN.with(|g| g.set(prev));
    r
}

fn in_guard() -> bool {
    IN.try_with(|g| g.get()).unwrap_or(true)
}

pub fn init() {
    with_guard(|| {
        let mut s = STATE.lock().unwrap();
        *s = Some(State {
            stack_index: HashMap::with_capacity_and_hasher(400_000, FxBuild::default()),
            frames: Vec::with_capacity(400_000 * 40),
            stacks: Vec::with_capacity(400_000),
            live: HashMap::with_capacity_and_hasher(2_000_000, FxBuild::default()),
            peak_snapshot: Vec::new(),
            peak_snapshot_live: 0,
            churn: HashMap::with_capacity_and_hasher(400_000, FxBuild::default()),
        });
    });
    ENABLED.store(true, Relaxed);
}

/// Aggregate the live sampled blocks by stack id.
pub fn snapshot_locked(s: &State) -> Vec<(u32, u64)> {
    let mut agg: HashMap<u32, u64, FxBuild> = HashMap::default();
    for (_, (st, w)) in s.live.iter() {
        *agg.entry(*st).or_default() += *w;
    }
    let mut v: Vec<_> = agg.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

pub fn snapshot() -> Vec<(u32, u64)> {
    with_guard(|| {
        let s = STATE.lock().unwrap();
        s.as_ref().map(snapshot_locked).unwrap_or_default()
    })
}

/// Restart peak tracking at the current live level (new phase).
pub fn reset_peak() {
    with_guard(|| {
        let live = LIVE.load(Relaxed);
        PEAK.store(live, Relaxed);
        let mut s = STATE.lock().unwrap();
        let Some(s) = s.as_mut() else { return };
        s.peak_snapshot = snapshot_locked(s);
        s.peak_snapshot_live = live;
    });
}

pub fn take_peak_snapshot() -> (i64, Vec<(u32, u64)>) {
    with_guard(|| {
        let s = STATE.lock().unwrap();
        let Some(s) = s.as_ref() else { return (0, vec![]) };
        (s.peak_snapshot_live, s.peak_snapshot.clone())
    })
}

#[inline(never)]
fn record_sample(ptr: *mut u8, weight: u64) {
    let mut buf = [std::ptr::null_mut::<libc::c_void>(); 64];
    let n = unsafe { backtrace(buf.as_mut_ptr(), 64) } as usize;
    let mut h = Fx::default();
    for f in &buf[..n] {
        h.write_usize(*f as usize);
    }
    let key = h.finish();
    let mut guard = STATE.lock().unwrap();
    let s = guard.as_mut().unwrap();
    let id = match s.stack_index.get(&key) {
        Some(id) => *id,
        None => {
            let id = s.stacks.len() as u32;
            let start = s.frames.len() as u32;
            for f in &buf[..n] {
                s.frames.push(*f as usize);
            }
            s.stacks.push((start, n as u16));
            s.stack_index.insert(key, id);
            id
        }
    };
    s.live.insert(ptr as usize, (id, weight));
    *s.churn.entry(id).or_default() += weight;
    let live = LIVE.load(Relaxed);
    if live > s.peak_snapshot_live + (s.peak_snapshot_live / 50).max(4 << 20) {
        s.peak_snapshot = snapshot_locked(s);
        s.peak_snapshot_live = live;
    }
}

#[inline]
fn on_alloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() || in_guard() {
        return;
    }
    let live = LIVE.fetch_add(size as i64, Relaxed) + size as i64;
    if live > PEAK.load(Relaxed) {
        PEAK.store(live, Relaxed);
    }
    TOTAL.fetch_add(size as u64, Relaxed);
    COUNT.fetch_add(1, Relaxed);
    let c = class_of(size);
    CLASS_BYTES[c].fetch_add(size as i64, Relaxed);
    CLASS_COUNT[c].fetch_add(1, Relaxed);
    CLASS_TOTAL[c].fetch_add(size as u64, Relaxed);
    if !ENABLED.load(Relaxed) {
        return;
    }
    let rate = RATE.load(Relaxed);
    let before = ACC.fetch_add(size as u64, Relaxed);
    let crossed = (before + size as u64) / rate - before / rate;
    if crossed > 0 {
        IN.with(|g| g.set(true));
        record_sample(ptr, crossed * rate);
        IN.with(|g| g.set(false));
    }
}

#[inline]
fn on_free(ptr: *mut u8, size: usize) {
    if in_guard() {
        return;
    }
    LIVE.fetch_sub(size as i64, Relaxed);
    let c = class_of(size);
    CLASS_BYTES[c].fetch_sub(size as i64, Relaxed);
    CLASS_COUNT[c].fetch_sub(1, Relaxed);
    if !ENABLED.load(Relaxed) {
        return;
    }
    IN.with(|g| g.set(true));
    if let Ok(mut s) = STATE.lock() {
        if let Some(s) = s.as_mut() {
            s.live.remove(&(ptr as usize));
        }
    }
    IN.with(|g| g.set(false));
}

pub struct Prof;

unsafe impl GlobalAlloc for Prof {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        on_alloc(p, l.size());
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        on_alloc(p, l.size());
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        on_free(p, l.size());
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new_size: usize) -> *mut u8 {
        on_free(p, l.size());
        let q = unsafe { System.realloc(p, l, new_size) };
        on_alloc(q, new_size);
        q
    }
}

pub fn rss_kb() -> u64 {
    let pid = std::process::id();
    with_guard(|| {
        std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    })
}

pub fn max_rss_kb() -> u64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    (ru.ru_maxrss as u64) / 1024
}

/// Write the stacks referenced by the given snapshots, as static addresses.
pub fn dump(path: &str, snaps: &[(&str, i64, &[(u32, u64)])]) {
    use std::io::Write;
    with_guard(|| {
        let s = STATE.lock().unwrap();
        let Some(s) = s.as_ref() else { return };
        let slide = unsafe { _dyld_get_image_vmaddr_slide(0) } as usize;
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let mut used = std::collections::BTreeSet::new();
        for (name, live, snap) in snaps {
            writeln!(f, "SNAP {name} {live} {}", snap.len()).unwrap();
            for (id, w) in snap.iter() {
                writeln!(f, "{id} {w}").unwrap();
                used.insert(*id);
            }
        }
        let mut churn: Vec<_> = s.churn.iter().map(|(a, b)| (*a, *b)).collect();
        churn.sort_by(|a, b| b.1.cmp(&a.1));
        writeln!(f, "SNAP churn 0 {}", churn.len()).unwrap();
        for (id, w) in &churn {
            writeln!(f, "{id} {w}").unwrap();
            used.insert(*id);
        }
        writeln!(f, "STACKS {}", used.len()).unwrap();
        for id in used {
            let (start, n) = s.stacks[id as usize];
            let fr = &s.frames[start as usize..start as usize + n as usize];
            write!(f, "{id}").unwrap();
            for a in fr {
                let a = a.wrapping_sub(slide);
                write!(f, " {:x}", a).unwrap();
            }
            writeln!(f).unwrap();
        }
    });
}
