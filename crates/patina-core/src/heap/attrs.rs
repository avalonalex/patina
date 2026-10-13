//! `GcAttrs`: what a heap's collector asks of the code that runs against it
//! (#699; `PRD/GC_PRD.md` §14, additions C1 and C10).
//!
//! A JIT's emitters switch on these values when they compile, and the
//! interpreter's inline fast paths are generated from the same values.
//! **A heap's attributes are fixed for its life** (§9), so no body runs
//! under a policy it was not compiled for.
//!
//! **The arena heap** is today's collector: non-moving, whole-heap, with no
//! barrier and no inline allocation. Its attributes are [`GcAttrs::ARENA`].
//!
//! **Fields still to come.** §14 also lists two tables, which arrive with
//! what fills them:
//! - `layout`, the layout table from `declare_layouts!` (5a);
//! - `helpers`, the runtime helpers' classes (stage 6).

use super::mutator::Mutator;

/// A heap's collector attributes, fixed for its life.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcAttrs {
    /// How code allocates inline.
    pub alloc: AllocAttrs,
    /// The write barrier stores emit.
    pub barrier: BarrierKind,
    /// The bit that tells a heap value from an immediate, which a barrier
    /// tests to skip immediates.
    /// - `None` for the arena heap, whose NaN-boxed encoding has no single such
    ///   bit (and which has no barrier).
    /// - `Some(2)` in §5's encoding.
    pub value_filter_bit: Option<u8>,
    /// How code polls.
    pub poll: PollKind,
    /// The stack watermark, a return barrier (§11.1).
    pub watermark: WatermarkKind,
    /// How a store publishes what it makes reachable to another carrier
    /// (§18.2 rule 4).
    /// - It is read only under `threaded`.
    /// - It is `None` until stage 6's spike chooses the mechanism.
    /// - Reserved now (C1, C10), so that adding it later breaks no ABI.
    pub publication: Option<Publication>,
    /// Whether the collector may move an object.
    pub can_move: bool,
    /// Whether the collector takes pins (`PinToken`). A heap that moves
    /// nothing has nothing to pin.
    pub can_pin: bool,
    /// Whether an initializing store needs the barrier. False for every
    /// configuration §14 ships.
    pub initializing_stores_need_barrier: bool,
}

/// How code allocates inline.
#[repr(C, u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocAttrs {
    /// No inline allocation: the arena heap allocates only through the
    /// runtime, until 5a's blocks.
    Runtime,
    /// Bump allocation from the `Mutator`'s buffer (§8), in granules, up to
    /// `max_inline` bytes.
    BumpPointer {
        ap_off: u16,
        limit_off: u16,
        granule: u16,
        max_inline: u16,
    },
}

/// The write barrier stores emit (§10).
#[repr(C, u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarrierKind {
    /// No barrier: a heap that is not generational, for its whole life.
    None,
    /// Pre-write field logging at granule granularity (§10).
    GranuleLog {
        bias_off: u16,
        shift: u8,
        log_bit: u8,
        cur_off: u16,
        soft_off: u16,
        pending_off: u16,
        limit_off: u16,
    },
}

/// How code polls (§12).
#[repr(C, u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollKind {
    /// A frame entry compares the stack top against `reg_limit`, which a
    /// post zeroes. The stack check and the poll are one compare.
    LimitFold { limit_off: u16 },
    /// As `LimitFold`, and every poll site also counts `ticks` down, for
    /// SRFI 18's deterministic scheduler.
    Tick { ticks_off: u16, limit_off: u16 },
}

/// The stack watermark (§11.1).
#[repr(C, u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatermarkKind {
    /// None: a heap with no minors.
    None,
    /// A return barrier below the frames a minor scanned.
    ReturnBarrier {
        trampoline: usize,
        link_flag_bit: u8,
        stride: u16,
    },
}

/// How a store publishes an object to another carrier (§18.2 rule 4).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Publication {
    FenceAtAllocation,
    ReleaseOnHeapStore,
    FenceOnHeapStore,
}

impl GcAttrs {
    /// Today's collector: non-moving and whole-heap, no barrier, no inline
    /// allocation, and a poll on `reg_limit`.
    pub const ARENA: GcAttrs = GcAttrs {
        alloc: AllocAttrs::Runtime,
        barrier: BarrierKind::None,
        value_filter_bit: None,
        poll: PollKind::LimitFold {
            limit_off: Mutator::REG_LIMIT_OFFSET,
        },
        watermark: WatermarkKind::None,
        publication: None,
        can_move: false,
        can_pin: false,
        initializing_stores_need_barrier: false,
    };
}

#[cfg(test)]
mod tests {
    use super::{GcAttrs, PollKind};

    #[test]
    fn the_arena_heap_polls_the_word_a_post_zeroes() {
        assert_eq!(GcAttrs::ARENA.poll, PollKind::LimitFold { limit_off: 0x30 });
    }
}
