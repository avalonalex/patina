//! How deep the VM's frame stack is (#903).

/// A depth of the frame stack: where a prompt or a handler was established,
/// where a delimited capture starts, and where a dispatch loop stops.
/// `ExecutionState::depth` reads the live stack's, and each continuation
/// snapshot answers its own.
///
/// Today it counts frames. GC stage 4d.7 (#908) makes it a byte offset from
/// the register stack's base (`PRD/GC_PRD.md` §11.1) and changes only what
/// it holds: the machine compares depths, takes the smaller of two, moves one
/// from one stack to another ([`Depth::relocate`]) and measures how far one
/// sits above another ([`Depth::distance_above`]), and none of that reads
/// the count. What does read it, [`Depth::frame_count`], is the census hooks,
/// the step tracer and the frame vector's own cuts in `ExecutionState`.
///
/// Every method is the integer operation it wraps, inlined, so a depth read
/// or compare on the dispatch loop's paths costs what the `usize` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub(crate) struct Depth(usize);

impl Depth {
    /// The empty stack's depth.
    pub(crate) const BASE: Depth = Depth(0);

    /// The depth of a stack of `frames` frames. Only the depth accessors
    /// call it: `ExecutionState::depth` and each continuation snapshot's.
    #[inline(always)]
    pub(crate) const fn of_frames(frames: usize) -> Depth {
        Depth(frames)
    }

    /// The number of frames below this depth, which the census hooks and
    /// the step tracer report and `ExecutionState` cuts its frame vector at.
    #[inline(always)]
    pub(crate) const fn frame_count(self) -> usize {
        self.0
    }

    /// Move a depth recorded against one stack onto another: `self` sat
    /// above `from` on that stack, and the same position is as far above
    /// `to` on this one.
    ///
    /// Saturating rather than wrapping, and not hypothetically:
    /// `install_thunk_handlers` clamps a record's handler depths down to the
    /// thunk's frame depth, which can be *below* the prompt a continuation
    /// was delimited by, so `self < from` happens. Wrapping would send such
    /// a depth to about 2^64, where `pop_resolved_handlers`'
    /// `stack_depth >= depth()` test drops it on the next return instead of
    /// keeping it for as long as the resumed frames run.
    #[inline(always)]
    pub(crate) fn relocate(self, from: Depth, to: Depth) -> Depth {
        Depth((to.0 + self.0).saturating_sub(from.0))
    }

    /// How far this depth sits above `floor`, or 0 if it does not: a
    /// distance in the type's own unit, which survives a capture replayed at
    /// another depth, as an absolute depth does not. The raise stub keeps
    /// one as a fixnum (`raise_step::DEPTH_BELOW`).
    #[inline(always)]
    pub(crate) fn distance_above(self, floor: Depth) -> usize {
        self.0.saturating_sub(floor.0)
    }

    /// The depth `distance` below this one, or [`Depth::BASE`] if that is
    /// further down than the stack goes: the inverse of
    /// [`Depth::distance_above`].
    #[inline(always)]
    pub(crate) fn lowered_by(self, distance: usize) -> Depth {
        Depth(self.0.saturating_sub(distance))
    }
}
