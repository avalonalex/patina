//! How deep the VM's register stack is (#903, #908).

/// A depth of the register stack: where a prompt or a handler was
/// established, where a delimited capture starts, and where a dispatch loop
/// stops. `ExecutionState::depth` reads the live stack's, and each
/// continuation snapshot answers its own.
///
/// It is the stack's top as a byte offset from its base
/// (`PRD/GC_PRD.md` §11.1), since GC stage 4d.7b (#908); until then it
/// counted frames. The machine compares depths, takes the smaller of two,
/// moves one from one stack to another ([`Depth::relocate`]) and measures
/// how far one sits above another ([`Depth::distance_above`]), and none of
/// that reads the offset. What does is the stack's own cuts and copies
/// ([`Depth::bytes`]).
///
/// **A depth read off a stack names a place between two frames,** and so
/// does one moved with the frames it was recorded among: a carried prompt's
/// ([`Depth::relocate`]), and the handler's a `raise-continuable` puts back
/// a distance below its stub ([`Depth::lowered_by`]), whose distance is
/// measured from a floor that every region able to replay the stub
/// contains (`vm_raise_value`). One case moves a depth past the frames it
/// was recorded among: a handler's carried below the prompt its
/// continuation was delimited by, [`Depth::relocate`]'s saturating case.
/// It can fall inside a frame of the stack it lands on. Such a depth is
/// only ever compared, never cut at: the handler is dropped once the
/// stack's top is at or below it.
///
/// Every method is the integer operation it wraps, inlined, so a depth read
/// or compare on the dispatch loop's paths costs what the `usize` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub(crate) struct Depth(usize);

impl Depth {
    /// The empty stack's depth.
    pub(crate) const BASE: Depth = Depth(0);

    /// The depth of a stack whose top is `bytes` above its base. Only the
    /// depth accessors call it: `ExecutionState`'s and each continuation
    /// snapshot's.
    #[inline(always)]
    pub(crate) const fn of_bytes(bytes: usize) -> Depth {
        Depth(bytes)
    }

    /// The byte offset this depth is, which the register stack cuts and
    /// copies at.
    #[inline(always)]
    pub(crate) const fn bytes(self) -> usize {
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
    /// distance in bytes, which survives a capture replayed at another
    /// depth, as an absolute depth does not. The raise stub keeps one as a
    /// fixnum (`raise_step::DEPTH_BELOW`).
    #[inline(always)]
    pub(crate) fn distance_above(self, floor: Depth) -> usize {
        self.0.saturating_sub(floor.0)
    }

    /// The depth `distance` bytes below this one, or [`Depth::BASE`] if
    /// that is further down than the stack goes: the inverse of
    /// [`Depth::distance_above`].
    #[inline(always)]
    pub(crate) fn lowered_by(self, distance: usize) -> Depth {
        Depth(self.0.saturating_sub(distance))
    }
}
