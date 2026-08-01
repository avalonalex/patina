//! GC policy for the tree-walker (`docs/GC_DESIGN.md` §6, §10 stage 2).
//!
//! The mode table and its environment-variable grammar live in `patina-core`
//! ([`GcMode`]) because `PATINA_GC`/`PATINA_GC_STRESS` are process-global and
//! both backends must agree on them. This type only pairs a mode with the
//! collector instance the tree-walker owns.

use patina_core::{Collector, GcMode, GcRoots, GcStats, Heap, MarkSweepCollector};

pub(crate) struct GcController {
    mode: GcMode,
    collector: MarkSweepCollector,
}

impl GcController {
    pub fn from_env() -> Self {
        Self {
            mode: GcMode::from_env(),
            collector: MarkSweepCollector::new(),
        }
    }

    /// The mode, cached by the caller so the hot-path check costs no borrow.
    pub fn mode(&self) -> GcMode {
        self.mode
    }

    pub fn should_collect(&self, heap: &Heap) -> bool {
        // `(gc)` is honored in every mode; the mode decides only whether to
        // collect automatically.
        heap.gc_requested() || self.mode.wants_collection(heap, &self.collector)
    }

    pub fn collect(&mut self, heap: &mut Heap, roots: &[&dyn GcRoots]) -> GcStats {
        self.collector.collect(heap, roots)
    }
}
