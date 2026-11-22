//! How deep the VM's frame stack is (#903).

/// A depth of the frame stack: where a dispatch loop stops, and, from the
/// next change, where a prompt or a handler was established and where a
/// delimited capture starts. `ExecutionState::depth` reads the live stack's.
///
/// Today it counts frames. GC stage 4d.7 (#908) makes it a byte offset from
/// the register stack's base (`PRD/GC_PRD.md` §11.1) and changes only what
/// it holds: the machine compares depths, and none of that reads the count.
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
    /// call it, and tests.
    #[inline(always)]
    pub(crate) const fn of_frames(frames: usize) -> Depth {
        Depth(frames)
    }

    /// The depth `distance` below this one, or [`Depth::BASE`] if that is
    /// further down than the stack goes.
    #[inline(always)]
    pub(crate) fn lowered_by(self, distance: usize) -> Depth {
        Depth(self.0.saturating_sub(distance))
    }
}
