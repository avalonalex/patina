//! The GC census (#651): what a program allocates, what survives a
//! collection, and what it stores into the heap, measured on today's heap
//! for the redesign's decisions (`PRD/GC_PRD.md` §6, §9.2, §10, §19). The
//! in-tree form of the study's `demographics.patch`
//! (`PRD/study/gc/probes/workload-demographics/`).
//!
//! **Compiled only with `patina-core`'s `gc-census` feature.** Without it,
//! every function here is an empty inline function, so the hooks the
//! backends and primitives place cost nothing and the shipped build is
//! unchanged. With it, a run is measured when `PATINA_GC_CENSUS` is set to
//! anything but `0`:
//!
//! - `PATINA_GC_CENSUS_OUT`: the summary, one `key value` line per figure,
//!   written at exit (standard error when unset);
//! - `PATINA_GC_CENSUS_LOG`: one CSV line per collection, the survival of
//!   what was allocated since the previous one, the live objects and the
//!   VM's stack at the safe point;
//! - `PATINA_GC_CENSUS_HEAPS`: appends `exe total=… max_live=…
//!   live_at_exit=…` at exit, the heaps this process made, whether or not
//!   `PATINA_GC_CENSUS` is set.
//!
//! Only the process's first heap is measured, on the thread that made it:
//! a census of several heaps would mix their slot numbers.
//!
//! **Allocations** are counted by kind and by length, and sized twice:
//! today, as `heap/account.rs` charges them (a slot plus its payload), and
//! in the headered layout of §6 (a header word plus the fields, a pair two
//! words with no header). **Survival** is the share of the objects allocated
//! since the previous collection that this one marks: run under
//! `PATINA_GC_STRESS=N` it is the survival of an N-allocation nursery.
//! **Stores** are counted per site; each records whether the value is an
//! immediate, and whether the holder and the value are young in the actual
//! heap (allocated since the last collection) and in five virtual nurseries
//! emptied every 16 K, 64 K, 256 K, 1 M and 4 M allocations — a store of a
//! young value into an old holder is an old-to-young edge, and its holder a
//! remembered-set entry for that interval.

use crate::tagged_value::TaggedValue;

/// Whether this build has the census compiled in.
pub const GC_CENSUS: bool = cfg!(feature = "gc-census");

/// Where a store into the heap, or into an environment, happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    SetCar,
    SetCdr,
    VectorSet,
    /// The VM's inline `vector-set!`, which writes the slot directly.
    VmVectorSet,
    MutableCell,
    ClosureFreeVar,
    PromiseUpdate,
    BreakEphemeron,
    RecordSet,
    ParameterInstall,
    ParameterInstallConverted,
    PromiseForcePrimitive,
    PromiseForceVm,
    /// The environment sites hold their values off the heap, so their
    /// holder is never young or old.
    EnvDefine,
    EnvSetSlot,
    EnvSetScoped,
    /// A tree-walker continuation's `define`, recorded here instead of as
    /// the `EnvDefine` it makes.
    TwContinuationDefine,
}

impl Site {
    pub const ALL: [Site; 17] = [
        Site::SetCar,
        Site::SetCdr,
        Site::VectorSet,
        Site::VmVectorSet,
        Site::MutableCell,
        Site::ClosureFreeVar,
        Site::PromiseUpdate,
        Site::BreakEphemeron,
        Site::RecordSet,
        Site::ParameterInstall,
        Site::ParameterInstallConverted,
        Site::PromiseForcePrimitive,
        Site::PromiseForceVm,
        Site::EnvDefine,
        Site::EnvSetSlot,
        Site::EnvSetScoped,
        Site::TwContinuationDefine,
    ];

    /// The name in the summary, the study's spelling.
    pub fn name(self) -> &'static str {
        match self {
            Site::SetCar => "set_car",
            Site::SetCdr => "set_cdr",
            Site::VectorSet => "vector_set",
            Site::VmVectorSet => "vm_vector_set",
            Site::MutableCell => "write_mutable_cell",
            Site::ClosureFreeVar => "set_vm_closure_free_var",
            Site::PromiseUpdate => "promise_update",
            Site::BreakEphemeron => "break_ephemeron",
            Site::RecordSet => "record_set",
            Site::ParameterInstall => "parameter_install",
            Site::ParameterInstallConverted => "parameter_install_converted",
            Site::PromiseForcePrimitive => "promise_force_prim",
            Site::PromiseForceVm => "promise_force_vm",
            Site::EnvDefine => "env_define",
            Site::EnvSetSlot => "env_set_slot_value",
            Site::EnvSetScoped => "env_set_scoped",
            Site::TwContinuationDefine => "tw_cont_define",
        }
    }
}

/// How `eq?`-hashing found a value's identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityHash {
    /// An immediate, hashed by its bits.
    Immediate,
    /// A heap value, hashed by its slot number.
    Index,
    /// A heap value hashed by the address of its `Rc` payload.
    Rc,
}

/// Record a store of `value` at `site` into `target`, the heap object that
/// holds it (`None` for an environment).
#[inline(always)]
pub fn store(site: Site, target: Option<TaggedValue>, value: TaggedValue) {
    #[cfg(feature = "gc-census")]
    imp::store(site, target, value);
    #[cfg(not(feature = "gc-census"))]
    let _ = (site, target, value);
}

/// Leave the next `EnvDefine` uncounted: its caller recorded it under a
/// site of its own.
#[inline(always)]
pub fn skip_next_env_define() {
    #[cfg(feature = "gc-census")]
    imp::skip_next_env_define();
}

/// One VM instruction dispatched, the denominator of the store rates.
#[inline(always)]
pub fn vm_instruction() {
    #[cfg(feature = "gc-census")]
    imp::vm_instruction();
}

#[inline(always)]
pub fn identity_hash(kind: IdentityHash) {
    #[cfg(feature = "gc-census")]
    imp::identity_hash(kind);
    #[cfg(not(feature = "gc-census"))]
    let _ = kind;
}

/// An `equal-hash` call, from Scheme.
#[inline(always)]
pub fn equal_hash_call() {
    #[cfg(feature = "gc-census")]
    imp::equal_hash_call();
}

/// `equal-hash` fell back to a slot number.
#[inline(always)]
pub fn equal_hash_fallback() {
    #[cfg(feature = "gc-census")]
    imp::equal_hash_fallback();
}

/// A full continuation captured: the frames and registers it copied.
#[inline(always)]
pub fn capture_full(frames: usize, registers: usize) {
    #[cfg(feature = "gc-census")]
    imp::continuation(imp::Continuation::Capture, frames, registers);
    #[cfg(not(feature = "gc-census"))]
    let _ = (frames, registers);
}

/// A full continuation reinstated: the frames and registers it copied back.
#[inline(always)]
pub fn restore_full(frames: usize, registers: usize) {
    #[cfg(feature = "gc-census")]
    imp::continuation(imp::Continuation::Restore, frames, registers);
    #[cfg(not(feature = "gc-census"))]
    let _ = (frames, registers);
}

/// A delimited continuation captured.
#[inline(always)]
pub fn capture_delimited(frames: usize, registers: usize) {
    #[cfg(feature = "gc-census")]
    imp::continuation(imp::Continuation::Delimited, frames, registers);
    #[cfg(not(feature = "gc-census"))]
    let _ = (frames, registers);
}

/// The VM's stack as a collection starts at a safe point: the next
/// collection's log line reports it.
#[inline(always)]
pub fn stack_shape(frames: usize, registers: usize) {
    #[cfg(feature = "gc-census")]
    imp::stack_shape(frames, registers);
    #[cfg(not(feature = "gc-census"))]
    let _ = (frames, registers);
}

/// The census itself: its state, and the heap's side of it (allocation,
/// the survival scan), which `heap/census.rs` calls.
#[cfg(feature = "gc-census")]
pub(crate) mod imp {
    use super::{IdentityHash, Site};
    use crate::tagged_value::TaggedValue;
    use std::cell::Cell;
    use std::fmt::Write as _;
    use std::io::Write as _;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    // -----------------------------------------------------------------
    // Enabling, the primary heap and its thread
    // -----------------------------------------------------------------

    fn enabled() -> bool {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var("PATINA_GC_CENSUS").is_ok_and(|v| v != "0"))
    }

    std::thread_local! {
        /// Whether this thread made the primary heap: the hooks that have no
        /// heap at hand (stores into environments and through primitives)
        /// count only on it.
        static ON_PRIMARY_THREAD: Cell<bool> = const { Cell::new(false) };
    }

    static PRIMARY_TAKEN: AtomicBool = AtomicBool::new(false);
    static STATE: Mutex<Option<Box<State>>> = Mutex::new(None);

    /// The census state, when this thread is the primary heap's.
    fn state() -> Option<MutexGuard<'static, Option<Box<State>>>> {
        if !ON_PRIMARY_THREAD.with(Cell::get) {
            return None;
        }
        Some(
            STATE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    fn with_state(f: impl FnOnce(&mut State)) {
        if let Some(mut guard) = state()
            && let Some(state) = guard.as_mut()
        {
            f(state);
        }
    }

    // -----------------------------------------------------------------
    // Heaps per process
    // -----------------------------------------------------------------

    static HEAPS_TOTAL: AtomicUsize = AtomicUsize::new(0);
    static HEAPS_LIVE: AtomicUsize = AtomicUsize::new(0);
    static HEAPS_MAX: AtomicUsize = AtomicUsize::new(0);
    static HEAPS_AT_EXIT: AtomicBool = AtomicBool::new(false);

    extern "C" fn heaps_at_exit() {
        let Some(path) = std::env::var_os("PATINA_GC_CENSUS_HEAPS") else {
            return;
        };
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_default();
            let _ = writeln!(
                file,
                "{exe} total={} max_live={} live_at_exit={}",
                HEAPS_TOTAL.load(Relaxed),
                HEAPS_MAX.load(Relaxed),
                HEAPS_LIVE.load(Relaxed)
            );
        }
    }

    /// A heap's part of the census: counted among the process's heaps while
    /// it lives, and measured when it is the primary heap.
    #[derive(Debug)]
    pub(crate) struct HeapCensus {
        /// Whether this heap's allocations and collections are measured.
        pub on: bool,
    }

    impl HeapCensus {
        pub fn new() -> Self {
            let live = HEAPS_LIVE.fetch_add(1, Relaxed) + 1;
            HEAPS_TOTAL.fetch_add(1, Relaxed);
            HEAPS_MAX.fetch_max(live, Relaxed);
            if std::env::var_os("PATINA_GC_CENSUS_HEAPS").is_some()
                && !HEAPS_AT_EXIT.swap(true, Relaxed)
            {
                at_exit(heaps_at_exit);
            }
            let on = enabled() && !PRIMARY_TAKEN.swap(true, Relaxed);
            if on {
                ON_PRIMARY_THREAD.with(|primary| primary.set(true));
                *STATE
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Box::default());
                at_exit(summary_at_exit);
            }
            HeapCensus { on }
        }
    }

    /// Run `f` at exit, `process::exit` included: the binary leaves through
    /// it without dropping its heap. Not on other platforms, where the
    /// census writes nothing.
    fn at_exit(f: extern "C" fn()) {
        // SAFETY: registers a function to run at exit. Each one here takes
        // no arguments and reads only atomics, the environment and the
        // state's lock.
        #[cfg(unix)]
        unsafe {
            libc::atexit(f);
        }
        #[cfg(not(unix))]
        let _ = f;
    }

    impl Drop for HeapCensus {
        fn drop(&mut self) {
            HEAPS_LIVE.fetch_sub(1, Relaxed);
        }
    }

    // -----------------------------------------------------------------
    // Kinds, lengths and sizes
    // -----------------------------------------------------------------

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Arena {
        Pair = 0,
        Vector = 1,
        String = 2,
        Object = 3,
    }

    /// An object's variant, numbered as the study numbered them.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Variant(pub usize);

    pub(crate) const VARIANT_NAMES: [&str; 28] = [
        "BigInt",
        "Rational",
        "Real",
        "Complex",
        "Symbol",
        "Bytevector",
        "Exception",
        "Procedure",
        "Port",
        "Macro",
        "RecordType",
        "Record",
        "Identifier",
        "Continuation",
        "Parameter",
        "Promise",
        "Library",
        "Values",
        "EnvironmentSpecifier",
        "PromptTag",
        "LabelPlaceholder",
        "MutableCell",
        "Ephemeron",
        "VmClosure",
        "VmContinuationRef",
        "VmDelimitedContinuationRef",
        "CoreSyntax",
        "Free",
    ];
    pub(crate) const V_REAL: usize = 2;
    pub(crate) const V_RECORD: usize = 11;
    pub(crate) const V_IDENTIFIER: usize = 12;
    pub(crate) const V_VALUES: usize = 17;
    pub(crate) const V_CELL: usize = 21;
    pub(crate) const V_CLOSURE: usize = 23;

    /// The survival classes.
    const CLASS_NAMES: [&str; 9] = [
        "pair",
        "vector",
        "string",
        "flonum",
        "closure",
        "cell",
        "identifier",
        "record",
        "other",
    ];

    pub(crate) fn class_of(arena: Arena, variant: Variant) -> usize {
        match arena {
            Arena::Pair => 0,
            Arena::Vector => 1,
            Arena::String => 2,
            Arena::Object => match variant.0 {
                V_REAL => 3,
                V_CLOSURE => 4,
                V_CELL => 5,
                V_IDENTIFIER => 6,
                V_RECORD => 7,
                _ => 8,
            },
        }
    }

    /// Length buckets: 0, 1, 2, 3, 4, 5–8, 9–16, …, 1025–8192, more.
    const NLEN: usize = 14;
    fn len_bucket(n: usize) -> usize {
        match n {
            0..=4 => n,
            5..=8 => 5,
            9..=16 => 6,
            17..=32 => 7,
            33..=64 => 8,
            65..=128 => 9,
            129..=256 => 10,
            257..=1024 => 11,
            1025..=8192 => 12,
            _ => 13,
        }
    }

    /// Size buckets of the headered layout, by upper bound in bytes.
    const SIZE_BOUNDS: [u64; 16] = [
        16,
        24,
        32,
        48,
        64,
        96,
        128,
        256,
        512,
        1024,
        2048,
        4096,
        8192,
        16384,
        32768,
        u64::MAX,
    ];
    fn size_bucket(bytes: u64) -> usize {
        SIZE_BOUNDS
            .iter()
            .position(|&bound| bytes <= bound)
            .unwrap_or(SIZE_BOUNDS.len() - 1)
    }

    pub(crate) fn round8(n: u64) -> u64 {
        (n + 7) & !7
    }

    /// A vector in the headered layout: a header word and its elements.
    pub(crate) fn vector_bytes(len: usize) -> u64 {
        8 + 8 * len as u64
    }

    /// A string in the headered layout: a header word and 4-byte
    /// characters, rounded to a word.
    pub(crate) fn string_bytes(len: usize) -> u64 {
        8 + round8(4 * len as u64)
    }

    // -----------------------------------------------------------------
    // The state
    // -----------------------------------------------------------------

    /// The virtual nurseries, in allocations; view 0 is the actual heap.
    const NURSERY: [u64; 5] = [16_384, 65_536, 262_144, 1_048_576, 4_194_304];
    const NVIEWS: usize = 6;
    /// The collection number of a free slot.
    pub(crate) const FREE: u32 = u32::MAX;

    #[derive(Default, Clone)]
    struct SiteCounts {
        total: u64,
        immediate: u64,
        heap_value: u64,
        target_unknown: u64,
        young_target: [u64; NVIEWS],
        young_target_heap_value: [u64; NVIEWS],
        young_value: [u64; NVIEWS],
        old_to_young: [u64; NVIEWS],
        old_target_heap_value: [u64; NVIEWS],
    }

    #[derive(Clone, Copy)]
    pub(crate) enum Continuation {
        Capture,
        Restore,
        Delimited,
    }

    #[derive(Default)]
    struct ContinuationCounts {
        count: u64,
        frames: u64,
        registers: u64,
        max_registers: u64,
    }

    #[derive(Default)]
    struct State {
        // The allocation census.
        allocations: u64,
        by_arena: [u64; 4],
        headered_bytes_by_arena: [u64; 4],
        today_bytes_by_arena: [u64; 4],
        vector_len: [u64; NLEN],
        vector_len_bytes: [u64; NLEN],
        string_len: [u64; NLEN],
        string_len_bytes: [u64; NLEN],
        by_variant: [u64; 28],
        variant_bytes: [u64; 28],
        closure_free_vars: [u64; NLEN],
        record_fields: [u64; NLEN],
        values_len: [u64; NLEN],
        size_hist: [u64; 16],
        size_hist_bytes: [u64; 16],
        // Per slot: the allocation number and the collection number at
        // allocation (`FREE` once swept).
        sequence: [Vec<u64>; 4],
        collection_of: [Vec<u32>; 4],
        collections: u32,
        // Survival: over every collection, and over all but the first,
        // whose interval holds the bootstrap.
        young_allocated: [[u64; 9]; 2],
        young_survived: [[u64; 9]; 2],
        young_allocated_bytes: [[u64; 9]; 2],
        young_survived_bytes: [[u64; 9]; 2],
        ages: [u64; 8],
        marked_total: u64,
        // Stores.
        sites: Vec<SiteCounts>,
        remembered: Vec<rustc_hash::FxHashSet<u64>>,
        remembered_epoch: [u64; NVIEWS],
        remembered_sum: [u64; NVIEWS],
        remembered_max: [u64; NVIEWS],
        remembered_nonempty: [u64; NVIEWS],
        skip_env_define: bool,
        vm_instructions: u64,
        // Hashing.
        identity_hash: [u64; 3],
        equal_hash_calls: u64,
        equal_hash_fallbacks: u64,
        // Continuations: captures, restores, delimited captures.
        continuations: [ContinuationCounts; 3],
        // The VM's stack at the next collection's safe point.
        stack_frames: u64,
        stack_registers: u64,
        max_frames: u64,
        max_registers: u64,
        log: Option<std::io::BufWriter<std::fs::File>>,
        log_opened: bool,
    }

    impl State {
        fn ensure_sites(&mut self) {
            if self.sites.is_empty() {
                self.sites = vec![SiteCounts::default(); Site::ALL.len()];
                self.remembered = (0..NVIEWS).map(|_| Default::default()).collect();
            }
        }

        /// The allocation and collection numbers of a heap value's slot,
        /// and a key naming the slot.
        fn epoch(&self, value: TaggedValue) -> Option<(u64, u32, u64)> {
            let arena = if value.is_pair() {
                Arena::Pair
            } else if value.is_vector() {
                Arena::Vector
            } else if value.is_string() {
                Arena::String
            } else if value.is_object() {
                Arena::Object
            } else {
                return None;
            };
            let arena = arena as usize;
            let index = value.heap_index() as usize;
            let sequence = *self.sequence[arena].get(index)?;
            let collection = self.collection_of[arena][index];
            Some((sequence, collection, ((arena as u64) << 32) | index as u64))
        }

        fn young(&self, view: usize, sequence: u64, collection: u32) -> bool {
            if view == 0 {
                collection == self.collections
            } else {
                let n = NURSERY[view - 1];
                sequence >= self.allocations - self.allocations % n
            }
        }

        fn view_epoch(&self, view: usize) -> u64 {
            if view == 0 {
                u64::from(self.collections)
            } else {
                self.allocations / NURSERY[view - 1]
            }
        }

        fn flush_remembered(&mut self, view: usize) {
            let n = self.remembered[view].len() as u64;
            self.remembered_sum[view] += n;
            self.remembered_max[view] = self.remembered_max[view].max(n);
            if n > 0 {
                self.remembered_nonempty[view] += 1;
            }
            self.remembered[view].clear();
        }
    }

    // -----------------------------------------------------------------
    // Hooks
    // -----------------------------------------------------------------

    /// One allocation into `index` of `arena`: `today` the bytes the byte
    /// account charged, `headered` the bytes in §6's layout.
    pub(crate) fn alloc(
        arena: Arena,
        index: u32,
        variant: Variant,
        len: usize,
        today: u64,
        headered: u64,
    ) {
        with_state(|s| {
            let a = arena as usize;
            let i = index as usize;
            if s.sequence[a].len() <= i {
                s.sequence[a].resize(i + 1, 0);
                s.collection_of[a].resize(i + 1, FREE);
            }
            s.sequence[a][i] = s.allocations;
            s.collection_of[a][i] = s.collections;
            s.allocations += 1;
            s.by_arena[a] += 1;
            s.headered_bytes_by_arena[a] += headered;
            s.today_bytes_by_arena[a] += today;
            let bucket = size_bucket(headered);
            s.size_hist[bucket] += 1;
            s.size_hist_bytes[bucket] += headered;
            match arena {
                Arena::Vector => {
                    s.vector_len[len_bucket(len)] += 1;
                    s.vector_len_bytes[len_bucket(len)] += headered;
                }
                Arena::String => {
                    s.string_len[len_bucket(len)] += 1;
                    s.string_len_bytes[len_bucket(len)] += headered;
                }
                Arena::Object => {
                    s.by_variant[variant.0] += 1;
                    s.variant_bytes[variant.0] += headered;
                    match variant.0 {
                        V_CLOSURE => s.closure_free_vars[len_bucket(len)] += 1,
                        V_RECORD => s.record_fields[len_bucket(len)] += 1,
                        V_VALUES => s.values_len[len_bucket(len)] += 1,
                        _ => {}
                    }
                }
                Arena::Pair => {}
            }
        });
    }

    pub(super) fn store(site: Site, target: Option<TaggedValue>, value: TaggedValue) {
        with_state(|s| {
            if site == Site::EnvDefine && s.skip_env_define {
                s.skip_env_define = false;
                return;
            }
            s.ensure_sites();
            let holder = target.and_then(|t| s.epoch(t));
            let stored = if value.is_heap_pointer() {
                s.epoch(value)
            } else {
                None
            };
            let young_holder: [bool; NVIEWS] =
                std::array::from_fn(|v| holder.is_some_and(|(q, c, _)| s.young(v, q, c)));
            let young_value: [bool; NVIEWS] =
                std::array::from_fn(|v| stored.is_some_and(|(q, c, _)| s.young(v, q, c)));
            let counts = &mut s.sites[site as usize];
            counts.total += 1;
            if holder.is_none() {
                counts.target_unknown += 1;
            }
            for (count, &young) in counts.young_target.iter_mut().zip(&young_holder) {
                *count += u64::from(young);
            }
            if !value.is_heap_pointer() {
                counts.immediate += 1;
                return;
            }
            counts.heap_value += 1;
            let mut edge = [false; NVIEWS];
            for v in 0..NVIEWS {
                if young_holder[v] {
                    counts.young_target_heap_value[v] += 1;
                } else {
                    counts.old_target_heap_value[v] += 1;
                }
                counts.young_value[v] += u64::from(young_value[v]);
                edge[v] = !young_holder[v] && young_value[v];
                counts.old_to_young[v] += u64::from(edge[v]);
            }
            if let Some((_, _, key)) = holder {
                for v in (0..NVIEWS).filter(|&v| edge[v]) {
                    let epoch = s.view_epoch(v);
                    if epoch != s.remembered_epoch[v] {
                        s.flush_remembered(v);
                        s.remembered_epoch[v] = epoch;
                    }
                    s.remembered[v].insert(key);
                }
            }
        });
    }

    pub(super) fn skip_next_env_define() {
        with_state(|s| s.skip_env_define = true);
    }

    pub(super) fn vm_instruction() {
        with_state(|s| s.vm_instructions += 1);
    }

    pub(super) fn identity_hash(kind: IdentityHash) {
        with_state(|s| s.identity_hash[kind as usize] += 1);
    }

    pub(super) fn equal_hash_call() {
        with_state(|s| s.equal_hash_calls += 1);
    }

    pub(super) fn equal_hash_fallback() {
        with_state(|s| s.equal_hash_fallbacks += 1);
    }

    pub(crate) fn continuation(kind: Continuation, frames: usize, registers: usize) {
        with_state(|s| {
            let c = &mut s.continuations[kind as usize];
            c.count += 1;
            c.frames += frames as u64;
            c.registers += registers as u64;
            c.max_registers = c.max_registers.max(registers as u64);
        });
    }

    pub(super) fn stack_shape(frames: usize, registers: usize) {
        with_state(|s| {
            s.stack_frames = frames as u64;
            s.stack_registers = registers as u64;
            s.max_frames = s.max_frames.max(frames as u64);
            s.max_registers = s.max_registers.max(registers as u64);
        });
    }

    // -----------------------------------------------------------------
    // Collections
    // -----------------------------------------------------------------

    /// What one collection found, gathered by `Heap::census_scan` between
    /// marking and sweeping.
    #[derive(Default)]
    pub(crate) struct Scan {
        pub young_allocated: [u64; 9],
        pub young_allocated_bytes: [u64; 9],
        pub young_survived: [u64; 9],
        pub young_survived_bytes: [u64; 9],
        pub live: [u64; 4],
        pub live_bytes: u64,
        pub dead_old: u64,
        pub dead_old_bytes: u64,
        pub ages: [u64; 8],
        pub arena_len: [u64; 4],
        pub scan_us: u64,
    }

    fn age_bucket(age: u32) -> usize {
        match age {
            0..=3 => age as usize,
            4..=7 => 4,
            8..=15 => 5,
            16..=63 => 6,
            _ => 7,
        }
    }

    /// Classify an arena's slots for the collection in progress:
    /// `len` slots, `marked(i)` whether slot `i` is live, and `info(i)` its
    /// headered bytes and survival class. Run between marking and
    /// sweeping, on the primary heap.
    pub(crate) fn scan_arena(
        scan: &mut Scan,
        arena: Arena,
        len: usize,
        marked: impl Fn(usize) -> bool,
        info: impl Fn(usize) -> (u64, usize),
    ) {
        with_state(|s| {
            let current = s.collections;
            let collection_of = &mut s.collection_of[arena as usize];
            if collection_of.len() < len {
                collection_of.resize(len, FREE);
            }
            scan.arena_len[arena as usize] = len as u64;
            for (i, born) in collection_of.iter_mut().enumerate().take(len) {
                if *born == FREE {
                    continue;
                }
                let (bytes, class) = info(i);
                let young = *born == current;
                if young {
                    scan.young_allocated[class] += 1;
                    scan.young_allocated_bytes[class] += bytes;
                }
                if marked(i) {
                    scan.live[arena as usize] += 1;
                    scan.live_bytes += bytes;
                    scan.ages[age_bucket(current.wrapping_sub(*born))] += 1;
                    if young {
                        scan.young_survived[class] += 1;
                        scan.young_survived_bytes[class] += bytes;
                    }
                } else {
                    if !young {
                        scan.dead_old += 1;
                        scan.dead_old_bytes += bytes;
                    }
                    *born = FREE;
                }
            }
        });
    }

    /// Record one collection of the primary heap.
    pub(crate) fn collected(scan: Scan) {
        with_state(|s| {
            if !s.log_opened {
                s.log_opened = true;
                s.log = std::env::var_os("PATINA_GC_CENSUS_LOG")
                    .and_then(|path| std::fs::File::create(path).ok())
                    .map(std::io::BufWriter::new);
                if let Some(log) = s.log.as_mut() {
                    let mut header = String::from(
                        "gc,alloc_seq,frames,regs,scan_us,young_alloc,young_alloc_bytes,young_surv,\
                         young_surv_bytes,live_pairs,live_vectors,live_strings,live_objects,\
                         live_bytes,dead_old,dead_old_bytes,arena_pairs,arena_vectors,\
                         arena_strings,arena_objects",
                    );
                    for class in CLASS_NAMES {
                        let _ = write!(header, ",ya_{class},ys_{class}");
                    }
                    for age in 0..8 {
                        let _ = write!(header, ",age{age}");
                    }
                    let _ = writeln!(log, "{header}");
                }
            }
            s.marked_total += scan.live.iter().sum::<u64>();
            for k in 0..2 {
                if k == 1 && s.collections == 0 {
                    continue;
                }
                for c in 0..9 {
                    s.young_allocated[k][c] += scan.young_allocated[c];
                    s.young_survived[k][c] += scan.young_survived[c];
                    s.young_allocated_bytes[k][c] += scan.young_allocated_bytes[c];
                    s.young_survived_bytes[k][c] += scan.young_survived_bytes[c];
                }
            }
            for a in 0..8 {
                s.ages[a] += scan.ages[a];
            }
            let (collections, allocations) = (s.collections, s.allocations);
            let (frames, registers) = (s.stack_frames, s.stack_registers);
            if let Some(log) = s.log.as_mut() {
                let mut line = format!(
                    "{collections},{allocations},{frames},{registers},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    scan.scan_us,
                    scan.young_allocated.iter().sum::<u64>(),
                    scan.young_allocated_bytes.iter().sum::<u64>(),
                    scan.young_survived.iter().sum::<u64>(),
                    scan.young_survived_bytes.iter().sum::<u64>(),
                    scan.live[0],
                    scan.live[1],
                    scan.live[2],
                    scan.live[3],
                    scan.live_bytes,
                    scan.dead_old,
                    scan.dead_old_bytes,
                    scan.arena_len[0],
                    scan.arena_len[1],
                    scan.arena_len[2],
                    scan.arena_len[3],
                );
                for c in 0..9 {
                    let _ = write!(
                        line,
                        ",{},{}",
                        scan.young_allocated[c], scan.young_survived[c]
                    );
                }
                for a in 0..8 {
                    let _ = write!(line, ",{}", scan.ages[a]);
                }
                let _ = writeln!(log, "{line}");
            }
            s.collections += 1;
            s.stack_frames = 0;
            s.stack_registers = 0;
        });
    }

    // -----------------------------------------------------------------
    // The summary
    // -----------------------------------------------------------------

    extern "C" fn summary_at_exit() {
        let mut guard = STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(s) = guard.as_mut() else {
            return;
        };
        if let Some(log) = s.log.as_mut() {
            let _ = log.flush();
        }
        let out = summary(s);
        match std::env::var_os("PATINA_GC_CENSUS_OUT") {
            Some(path) => {
                let _ = std::fs::write(path, out);
            }
            None => eprint!("{out}"),
        }
    }

    /// The summary's lines; names and shapes are the study's, so its
    /// analysis reads them, plus `today_bytes_by_arena`.
    fn summary(s: &mut State) -> String {
        s.ensure_sites();
        for v in 0..NVIEWS {
            s.flush_remembered(v);
        }
        let mut w = String::new();
        let _ = writeln!(
            w,
            "heaps_total {} heaps_max_live {}",
            HEAPS_TOTAL.load(Relaxed),
            HEAPS_MAX.load(Relaxed)
        );
        let _ = writeln!(w, "collections {}", s.collections);
        let _ = writeln!(
            w,
            "max_frames {} max_regs {}",
            s.max_frames, s.max_registers
        );
        let _ = writeln!(w, "alloc_total {}", s.allocations);
        let _ = writeln!(w, "vm_instrs {}", s.vm_instructions);
        let _ = writeln!(w, "alloc_by_arena {:?}", s.by_arena);
        let _ = writeln!(w, "bytes_by_arena {:?}", s.headered_bytes_by_arena);
        let _ = writeln!(w, "today_bytes_by_arena {:?}", s.today_bytes_by_arena);
        let _ = writeln!(w, "vec_len {:?}", s.vector_len);
        let _ = writeln!(w, "vec_len_bytes {:?}", s.vector_len_bytes);
        let _ = writeln!(w, "str_len {:?}", s.string_len);
        let _ = writeln!(w, "str_len_bytes {:?}", s.string_len_bytes);
        let _ = writeln!(w, "variant_names {}", VARIANT_NAMES.join(","));
        let _ = writeln!(w, "obj_variant {:?}", s.by_variant);
        let _ = writeln!(w, "obj_variant_bytes {:?}", s.variant_bytes);
        let _ = writeln!(w, "closure_fv {:?}", s.closure_free_vars);
        let _ = writeln!(w, "record_fields {:?}", s.record_fields);
        let _ = writeln!(w, "values_len {:?}", s.values_len);
        let _ = writeln!(w, "size_hist {:?}", s.size_hist);
        let _ = writeln!(w, "size_hist_bytes {:?}", s.size_hist_bytes);
        for k in 0..2 {
            let _ = writeln!(w, "young_alloc_class{k} {:?}", s.young_allocated[k]);
            let _ = writeln!(w, "young_surv_class{k} {:?}", s.young_survived[k]);
            let _ = writeln!(
                w,
                "young_alloc_bytes_class{k} {:?}",
                s.young_allocated_bytes[k]
            );
            let _ = writeln!(
                w,
                "young_surv_bytes_class{k} {:?}",
                s.young_survived_bytes[k]
            );
        }
        let _ = writeln!(w, "age_hist {:?}", s.ages);
        let _ = writeln!(w, "marked_total_sum {}", s.marked_total);
        for (site, c) in Site::ALL.iter().zip(&s.sites) {
            if c.total == 0 {
                continue;
            }
            let _ = writeln!(
                w,
                "site {} total {} imm {} heapval {} target_unknown {} young_target {:?} \
                 young_target_heapval {:?} young_value {:?} old_to_young {:?} old_target_heapval {:?}",
                site.name(),
                c.total,
                c.immediate,
                c.heap_value,
                c.target_unknown,
                c.young_target,
                c.young_target_heap_value,
                c.young_value,
                c.old_to_young,
                c.old_target_heap_value
            );
        }
        let _ = writeln!(
            w,
            "rs_sum {:?} rs_max {:?} rs_nonempty {:?}",
            s.remembered_sum, s.remembered_max, s.remembered_nonempty
        );
        let _ = writeln!(
            w,
            "idhash imm {} index {} rc {} equal_hash_calls {} equal_hash_fallback {}",
            s.identity_hash[0],
            s.identity_hash[1],
            s.identity_hash[2],
            s.equal_hash_calls,
            s.equal_hash_fallbacks
        );
        let [capture, restore, delimited] = &s.continuations;
        let _ = writeln!(
            w,
            "cap_full {} frames {} regs {} max_regs {} restore {} rframes {} rregs {} \
             cap_delim {} dframes {} dregs {}",
            capture.count,
            capture.frames,
            capture.registers,
            capture.max_registers,
            restore.count,
            restore.frames,
            restore.registers,
            delimited.count,
            delimited.frames,
            delimited.registers
        );
        w
    }
}
