//! What each collection cost and why it ran (#648, `PRD/GC_PRD.md` §15):
//! the reason and phase times of every collection, the pause and MMU
//! figures `(gc-stats)` reports, [K16]'s two high-water marks with their
//! sites, the time from a collection being posted to the safe point that
//! runs it, and the `PATINA_GC_LOG` CSV.
//!
//! None of it changes what a collection does. The allocation path gains
//! nothing: the posting time is read where the pending flag first rises,
//! once per collection cycle, and K16's counts are differences of the byte
//! counter #606 added, taken at a deferral window's two ends.
//!
//! [K16]: https://github.com/avalonalex/patina/blob/main/PRD/GC_PRD.md#k16

use super::gc::ArenaCounts;
use std::collections::VecDeque;
use std::panic::Location;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The process's time origin, for the log's monotonic start times: when
/// its first heap was made.
pub(crate) fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Why a collection ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectReason {
    /// The adaptive byte trigger (#606).
    Bytes,
    /// The allocation count of `PATINA_GC_STRESS`.
    Stress,
    /// `PATINA_GC_ZEAL=entry`: a collection at every safe point.
    Zeal,
    /// Descriptor pressure (#607).
    Descriptors,
    /// A collection a call asked for where collection was deferred, which
    /// waited for the next safe point that may collect (`Heap::defer_collection`).
    Posted,
    /// At the call that asked for it: `(gc)`, or an open that ran out of
    /// descriptors (`Step::Collect`, #639).
    Call,
}

impl CollectReason {
    /// The name the log and `(gc-stats)` use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Stress => "stress",
            Self::Zeal => "zeal",
            Self::Descriptors => "descriptors",
            Self::Posted => "posted",
            Self::Call => "call",
        }
    }
}

/// The phases of one collection's pause.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhaseTimes {
    /// The root providers' `trace_roots`.
    pub roots: Duration,
    /// Marking from the roots (`GcVisitor::drain`).
    pub mark: Duration,
    /// The fixpoint over weak ids and ephemerons, and breaking the
    /// ephemerons it left.
    pub weak: Duration,
    /// The providers' `sweep_weak`.
    pub prune: Duration,
    /// The heap's sweep.
    pub sweep: Duration,
    /// The backend's work after the collection (the VM's code release).
    pub after: Duration,
}

impl PhaseTimes {
    /// The pause: every phase.
    pub fn total(&self) -> Duration {
        self.roots + self.mark + self.weak + self.prune + self.sweep + self.after
    }
}

/// One collection, as the log writes it.
#[derive(Debug, Clone)]
pub(crate) struct CollectionRecord {
    /// When it started, since [`epoch`].
    pub start: Duration,
    pub reason: CollectReason,
    pub phases: PhaseTimes,
    /// The heap's collection count after it: 1 for the first.
    pub number: u64,
    /// Bytes charged since the collection before.
    pub allocated: u64,
    /// L after it, and the bytes it freed.
    pub live: usize,
    pub freed: u64,
    pub external: usize,
    pub marked: ArenaCounts,
    pub swept: ArenaCounts,
    /// From the collection being posted to its start, in bytes and in time;
    /// zero for one that ran at its call.
    pub wait_bytes: u64,
    pub wait: Duration,
}

/// The windows of the minimum mutator utilisation, in milliseconds.
pub const MMU_WINDOWS_MS: [u64; 7] = [1, 2, 5, 10, 20, 50, 100];

/// The minimum mutator utilisation (Cheng and Blelloch): for each window
/// length, the smallest fraction of any window of that length left to the
/// program, over every non-mutator interval so far (Larceny's
/// `gc_mmu_log.c`).
///
/// The worst window of a length either ends where an interval ends or
/// starts where one starts, so those are the only ones evaluated: a window
/// ending at an interval's end when that interval is recorded, and a window
/// starting at an interval's start once time has passed its far end, at a
/// later interval. The intervals a pending window still needs are kept in a
/// ring, which a window's length bounds.
#[derive(Debug, Clone)]
pub struct Mmu {
    intervals: VecDeque<(Duration, Duration)>,
    /// Per window: the starts not yet evaluated, oldest first.
    pending_starts: [VecDeque<Duration>; MMU_WINDOWS_MS.len()],
    /// Per window: the smallest utilisation seen, 1.0 before any interval.
    minimum: [f64; MMU_WINDOWS_MS.len()],
}

impl Default for Mmu {
    fn default() -> Self {
        Self {
            intervals: VecDeque::new(),
            pending_starts: Default::default(),
            minimum: [1.0; MMU_WINDOWS_MS.len()],
        }
    }
}

impl Mmu {
    /// Record a non-mutator interval, `[start, end]`.
    pub fn record(&mut self, start: Duration, end: Duration) {
        let end = end.max(start);
        self.intervals.push_back((start, end));
        for (slot, &ms) in MMU_WINDOWS_MS.iter().enumerate() {
            let window = Duration::from_millis(ms);
            // The window ending here, once there has been a whole window
            // of time before it: an earlier one would start before the
            // process did, and is left to the windows starting at starts.
            if end >= window {
                let utilisation = self.utilisation(end - window, window);
                self.note(slot, utilisation);
            }
            // Windows starting at earlier intervals' starts, now wholly past.
            while let Some(&from) = self.pending_starts[slot].front() {
                if from + window > end {
                    break;
                }
                self.pending_starts[slot].pop_front();
                let utilisation = self.utilisation(from, window);
                self.note(slot, utilisation);
            }
            self.pending_starts[slot].push_back(start);
        }
        // What no window can reach any more: an interval ending before the
        // oldest pending start, less the longest window.
        let longest = Duration::from_millis(MMU_WINDOWS_MS[MMU_WINDOWS_MS.len() - 1]);
        let horizon = self
            .pending_starts
            .iter()
            .filter_map(|starts| starts.front().copied())
            .min()
            .unwrap_or(end)
            .min(end.saturating_sub(longest));
        while self
            .intervals
            .front()
            .is_some_and(|&(_, finish)| finish < horizon)
        {
            self.intervals.pop_front();
        }
    }

    fn note(&mut self, slot: usize, utilisation: f64) {
        if utilisation < self.minimum[slot] {
            self.minimum[slot] = utilisation;
        }
    }

    /// The fraction of `[from, from + window]` outside every interval.
    fn utilisation(&self, from: Duration, window: Duration) -> f64 {
        let to = from + window;
        let busy: Duration = self
            .intervals
            .iter()
            .map(|&(start, end)| end.min(to).saturating_sub(start.max(from)))
            .sum();
        1.0 - busy.as_secs_f64() / window.as_secs_f64()
    }

    /// The MMU at a window of `ms` milliseconds, one of
    /// [`MMU_WINDOWS_MS`]: 1.0 before any interval. A window starting at
    /// the latest intervals' starts counts once a later interval passes its
    /// far end.
    pub fn at(&self, ms: u64) -> Option<f64> {
        let slot = MMU_WINDOWS_MS.iter().position(|&w| w == ms)?;
        Some(self.minimum[slot].max(0.0))
    }
}

/// A high-water mark with the site that set it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HighWater {
    pub bytes: u64,
    pub site: Option<&'static Location<'static>>,
}

impl HighWater {
    fn note(&mut self, bytes: u64, site: Option<&'static Location<'static>>) {
        if bytes > self.bytes {
            self.bytes = bytes;
            self.site = site;
        }
    }
}

/// A heap's record of its collections and its deferral windows.
#[derive(Debug, Default)]
pub(crate) struct Telemetry {
    pub last_pause: Duration,
    pub pause_max: Duration,
    pub pause_total: Duration,
    pub mmu: Mmu,
    /// K16: the most bytes allocated between a collection being posted and
    /// the safe point that ran it, with the deferral window that held it.
    pub wait: HighWater,
    /// The longest time from a collection being posted to its start (C12).
    pub wait_time_max: Duration,
    /// K16: the most bytes allocated inside one deferral window — while a
    /// guard other than the running loop's own was alive — with the guard
    /// that opened it.
    pub deferral: HighWater,
    /// The deferral window open now: the bytes allocated when it opened,
    /// and the guard that opened it.
    pub window: Option<(u64, &'static Location<'static>)>,
    /// A collection whose backend has not yet reported its work after it
    /// (`GcController::finish`).
    pub unfinished: Option<CollectionRecord>,
    /// This heap's number in the log.
    pub heap_id: u64,
}

impl Telemetry {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_HEAP: AtomicU64 = AtomicU64::new(1);
        // The log's times count from the first heap the process makes.
        epoch();
        Self {
            heap_id: NEXT_HEAP.fetch_add(1, Ordering::Relaxed),
            ..Self::default()
        }
    }

    /// A deferral window opened, with `allocated` bytes charged so far.
    pub fn open_window(&mut self, allocated: u64, site: &'static Location<'static>) {
        self.window = Some((allocated, site));
    }

    /// The window open closed, with `allocated` bytes charged so far.
    pub fn close_window(&mut self, allocated: u64) {
        if let Some((opened, site)) = self.window.take() {
            self.deferral
                .note(allocated.saturating_sub(opened), Some(site));
        }
    }

    /// Note a collection's wait for its safe point.
    pub fn note_wait(
        &mut self,
        bytes: u64,
        time: Duration,
        site: Option<&'static Location<'static>>,
    ) {
        self.wait.note(bytes, site);
        self.wait_time_max = self.wait_time_max.max(time);
    }

    /// A collection's pause is complete: count it, and log it.
    pub fn finish(&mut self, record: CollectionRecord) {
        let pause = record.phases.total();
        self.last_pause = pause;
        self.pause_max = self.pause_max.max(pause);
        self.pause_total += pause;
        self.mmu.record(record.start, record.start + pause);
        gc_log::write(self.heap_id, &record);
    }
}

/// `PATINA_GC_LOG=<path>`: one CSV line per collection, for every heap in
/// the process. The header names the columns; `docs/TEST_ORGANIZATION.md`
/// documents them. The file is opened once, at the first collection, and
/// each line is written whole under a lock, so heaps on other threads
/// interleave by line.
mod gc_log {
    use super::CollectionRecord;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};

    pub(super) const HEADER: &str = "heap,number,start_us,reason,pause_us,roots_us,mark_us,\
        weak_us,prune_us,sweep_us,after_us,wait_us,wait_bytes,allocated_bytes,live_bytes,\
        freed_bytes,external_bytes,marked_pairs,marked_vectors,marked_strings,marked_objects,\
        swept_pairs,swept_vectors,swept_strings,swept_objects";

    fn log() -> Option<&'static Mutex<std::fs::File>> {
        static LOG: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
        LOG.get_or_init(|| {
            let path = std::env::var("PATINA_GC_LOG")
                .ok()
                .filter(|v| !v.is_empty() && v != "0")?;
            match std::fs::File::create(&path) {
                Ok(mut file) => {
                    // A diagnostic: a failed write loses lines, which only
                    // a reader of the log notices.
                    let _ = writeln!(file, "{HEADER}");
                    Some(Mutex::new(file))
                }
                Err(e) => {
                    eprintln!("patina: PATINA_GC_LOG={path}: {e}");
                    None
                }
            }
        })
        .as_ref()
    }

    pub(super) fn line(heap: u64, r: &CollectionRecord) -> String {
        let us = |d: std::time::Duration| d.as_micros();
        let p = &r.phases;
        format!(
            "{heap},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            r.number,
            us(r.start),
            r.reason.as_str(),
            us(p.total()),
            us(p.roots),
            us(p.mark),
            us(p.weak),
            us(p.prune),
            us(p.sweep),
            us(p.after),
            us(r.wait),
            r.wait_bytes,
            r.allocated,
            r.live,
            r.freed,
            r.external,
            r.marked.pairs,
            r.marked.vectors,
            r.marked.strings,
            r.marked.objects,
            r.swept.pairs,
            r.swept.vectors,
            r.swept.strings,
            r.swept.objects,
        )
    }

    pub(super) fn write(heap: u64, record: &CollectionRecord) {
        if let Some(log) = log() {
            let line = line(heap, record);
            let mut file = log.lock().unwrap_or_else(|e| e.into_inner());
            let _ = writeln!(file, "{line}");
        }
    }
}

/// The process's resident size in bytes: its physical footprint on macOS,
/// its resident set on Linux. `None` where neither can be read.
pub fn resident_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: `proc_pid_rusage` fills a `rusage_info_v2` for the flavor
        // `RUSAGE_INFO_V2`, through the pointer it is given.
        let rc = unsafe {
            libc::proc_pid_rusage(
                std::process::id() as libc::c_int,
                libc::RUSAGE_INFO_V2,
                info.as_mut_ptr().cast::<libc::rusage_info_t>(),
            )
        };
        if rc != 0 {
            return None;
        }
        // SAFETY: filled by the successful call above.
        Some(unsafe { info.assume_init() }.ri_phys_footprint)
    }
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        // SAFETY: `sysconf` reads a configuration value.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        u64::try_from(page).ok().map(|page| pages * page)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// The CPU time the process has used, user and system, in microseconds.
/// `None` where it cannot be read.
pub fn cpu_micros() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        // SAFETY: `getrusage` fills a `rusage` through the pointer.
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: filled by the successful call above.
        let usage = unsafe { usage.assume_init() };
        let micros = |t: libc::timeval| {
            u64::try_from(t.tv_sec).unwrap_or(0) * 1_000_000 + u64::try_from(t.tv_usec).unwrap_or(0)
        };
        Some(micros(usage.ru_utime) + micros(usage.ru_stime))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

impl super::Heap {
    /// A guard other than the running loop's own was taken at `site`.
    pub(super) fn open_deferral_window(&mut self, site: &'static Location<'static>) {
        let allocated = self.bytes_allocated();
        self.telemetry.open_window(allocated, site);
        let counts = &self.account.shared;
        counts.open_window.set(Some(site));
        // A collection already pending waits on this window now.
        if counts.pending.get() && counts.wait_site.get().is_none() {
            counts.wait_site.set(Some(site));
        }
    }

    /// The last such guard dropped.
    pub(super) fn close_deferral_window(&mut self) {
        let allocated = self.bytes_allocated();
        self.telemetry.close_window(allocated);
        self.account.shared.open_window.set(None);
    }

    /// A collection starts at `now`: how long it waited since it was
    /// posted, in bytes and in time, noted in K16's first high-water mark
    /// and the time to safepoint. Zero for one nothing posted, which runs
    /// at the call that asked for it.
    pub(crate) fn take_collection_wait(&mut self, now: Instant) -> (u64, Duration) {
        let counts = &self.account.shared;
        let site = counts.wait_site.take();
        let Some((since_gc, posted_at)) = counts.posted.take() else {
            return (0, Duration::ZERO);
        };
        let bytes = self.bytes_since_gc().saturating_sub(since_gc) as u64;
        let time = now.saturating_duration_since(posted_at);
        self.telemetry.note_wait(bytes, time, site);
        (bytes, time)
    }

    /// A collection's record, which the backend completes with
    /// [`Heap::finish_collection`]. One it never completed is counted
    /// now, without its backend's part.
    pub(crate) fn record_collection(&mut self, record: CollectionRecord) {
        if let Some(unfinished) = self.telemetry.unfinished.take() {
            self.telemetry.finish(unfinished);
        }
        self.telemetry.unfinished = Some(record);
    }

    /// The backend has done its work after the collection just run, which
    /// took `after` (the VM's code release; zero on the tree-walker): count
    /// the collection's pause, and log it (#648).
    pub fn finish_collection(&mut self, after: Duration) {
        if let Some(mut record) = self.telemetry.unfinished.take() {
            record.phases.after = after;
            self.telemetry.finish(record);
        }
    }

    /// The last collection's pause, its backend's part included.
    pub fn last_pause(&self) -> Duration {
        self.telemetry.last_pause
    }

    /// The longest pause so far.
    pub fn pause_max(&self) -> Duration {
        self.telemetry.pause_max
    }

    /// Every pause so far.
    pub fn pause_total(&self) -> Duration {
        self.telemetry.pause_total
    }

    /// The minimum mutator utilisation at a window of `ms` milliseconds,
    /// one of [`MMU_WINDOWS_MS`].
    pub fn mmu(&self, ms: u64) -> Option<f64> {
        self.telemetry.mmu.at(ms)
    }

    /// K16's first high-water mark: the most bytes allocated between a
    /// collection being posted and the safe point that ran it, and the
    /// deferral window that held it, if one did.
    pub fn wait_high_water(&self) -> HighWater {
        self.telemetry.wait
    }

    /// The longest time from a collection being posted to its start.
    pub fn wait_time_max(&self) -> Duration {
        self.telemetry.wait_time_max
    }

    /// K16's second high-water mark: the most bytes allocated inside one
    /// deferral window, and the guard that opened it.
    pub fn deferral_high_water(&self) -> HighWater {
        self.telemetry.deferral
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The MMU of a timeline worked by hand: pauses of 2 ms at 10, 20 and
    /// 30 ms, and one of 6 ms at 100 ms.
    #[test]
    fn mmu_follows_the_definition() {
        let mut mmu = Mmu::default();
        assert_eq!(mmu.at(10), Some(1.0));
        for (start, length) in [(10, 2), (20, 2), (30, 2), (100, 6), (300, 1)] {
            mmu.record(ms(start), ms(start + length));
        }
        // A window no longer than a pause can lie inside the 6 ms one.
        assert_eq!(mmu.at(1), Some(0.0));
        assert_eq!(mmu.at(5), Some(0.0));
        // 10 ms: at worst 6 ms of the 100 ms pause and none of the others.
        assert!((mmu.at(10).unwrap() - 0.4).abs() < 1e-9, "{:?}", mmu.at(10));
        // 20 ms: [100, 120] holds 6 ms, and [10, 30] or [12, 32] 4 ms.
        assert!((mmu.at(20).unwrap() - 0.7).abs() < 1e-9, "{:?}", mmu.at(20));
        // 50 ms: [10, 60] holds all three short pauses, 6 ms; [100, 150]
        // holds 6 ms as well.
        assert!(
            (mmu.at(50).unwrap() - 0.88).abs() < 1e-9,
            "{:?}",
            mmu.at(50)
        );
        // 100 ms: [10, 110] holds 2 + 2 + 2 + 6 = 12 ms.
        assert!(
            (mmu.at(100).unwrap() - 0.88).abs() < 1e-9,
            "{:?}",
            mmu.at(100)
        );
        assert_eq!(mmu.at(3), None);
    }

    /// The ring keeps no more than its windows can reach.
    #[test]
    fn the_mmu_ring_is_bounded_by_its_longest_window() {
        let mut mmu = Mmu::default();
        for i in 0..10_000u64 {
            mmu.record(ms(i * 10), ms(i * 10 + 1));
        }
        assert!(mmu.intervals.len() <= 25, "{}", mmu.intervals.len());
        assert!((mmu.at(10).unwrap() - 0.9).abs() < 1e-9);
    }

    #[test]
    fn the_log_line_has_a_field_for_every_column() {
        let record = CollectionRecord {
            start: ms(5),
            reason: CollectReason::Bytes,
            phases: PhaseTimes {
                roots: ms(1),
                mark: ms(2),
                weak: ms(3),
                prune: ms(4),
                sweep: ms(5),
                after: ms(6),
            },
            number: 1,
            allocated: 100,
            live: 50,
            freed: 40,
            external: 10,
            marked: ArenaCounts::default(),
            swept: ArenaCounts::default(),
            wait_bytes: 7,
            wait: ms(1),
        };
        let line = gc_log::line(3, &record);
        assert_eq!(
            line.split(',').count(),
            gc_log::HEADER.split(',').count(),
            "{line}"
        );
        assert!(line.starts_with("3,1,5000,bytes,21000,1000,2000,3000,4000,5000,6000,1000,7,"));
    }

    #[test]
    fn the_process_reports_its_resident_size_and_cpu_time() {
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert!(resident_bytes().is_some_and(|b| b > 0));
            assert!(cpu_micros().is_some());
        }
    }
}
