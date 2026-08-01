//! GC policy for the tree-walker (`docs/GC_DESIGN.md` §6, §10 stage 2).
//!
//! Collection is **off by default**: the baseline build must behave exactly as
//! it did before the collector existed. Two escape hatches turn it on:
//!
//! - `(gc)` — always honored, in every mode. The primitive records a request
//!   on the heap and the next trampoline safe point services it.
//! - `PATINA_GC` / `PATINA_GC_STRESS` — enable automatic collection, the
//!   latter being the differential-testing lane.
//!
//! Stress mode deliberately bypasses the adaptive `2 × live` floor: after
//! bootstrap the live set is large enough that the adaptive threshold would
//! almost never fire, which is the opposite of what a stress lane wants.

use patina_core::{Collector, GcRoots, GcStats, Heap, MarkSweepCollector};

#[derive(Debug, Clone, Copy)]
enum GcMode {
    /// Collect only when `(gc)` has been called (default).
    Off,
    /// Collect on the collector's adaptive threshold.
    On,
    /// Collect once `n` allocations have happened since the last collection,
    /// ignoring the adaptive floor.
    Stress(usize),
}

pub(crate) struct GcController {
    mode: GcMode,
    collector: MarkSweepCollector,
}

impl GcController {
    /// Read the mode from the environment. `PATINA_GC_STRESS` takes an
    /// optional allocation count (`PATINA_GC_STRESS=1` collects at nearly
    /// every safe point; larger values trade coverage for runtime).
    pub fn from_env() -> Self {
        let enabled = |name: &str| -> Option<String> {
            match std::env::var(name) {
                Ok(v) if !v.is_empty() && v != "0" => Some(v),
                _ => None,
            }
        };

        let mode = if let Some(v) = enabled("PATINA_GC_STRESS") {
            GcMode::Stress(v.parse().unwrap_or(1).max(1))
        } else if enabled("PATINA_GC").is_some() {
            GcMode::On
        } else {
            GcMode::Off
        };

        Self {
            mode,
            collector: MarkSweepCollector::new(),
        }
    }

    pub fn should_collect(&self, heap: &Heap) -> bool {
        match self.mode {
            GcMode::Off => heap.gc_requested(),
            GcMode::On => self.collector.should_collect(heap),
            GcMode::Stress(n) => heap.gc_requested() || heap.allocs_since_gc() >= n,
        }
    }

    pub fn collect(&mut self, heap: &mut Heap, roots: &[&dyn GcRoots]) -> GcStats {
        self.collector.collect(heap, roots)
    }
}
