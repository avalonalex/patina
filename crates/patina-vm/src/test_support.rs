//! Test-only hooks into the VM, compiled only with the `test-support`
//! feature, which only `patina-tests`' [dev-dependencies] enable. Resolver 2
//! keeps a dev-dependency's features out of normal builds, so no build that
//! ships has any of this. Each hook is a switch per test thread, which a
//! check build's positive control turns on to make the machine do what the
//! check exists to catch.

use std::cell::Cell;

thread_local! {
    static DROP_HIGHEST_LIVE: Cell<bool> = const { Cell::new(false) };
    static SKIP_CODE_IDS: Cell<bool> = const { Cell::new(false) };
    static POLL_IN_WINDOW: Cell<Option<WindowEnd>> = const { Cell::new(None) };
}

/// The wrong-map switch behind the `DEAD_SLOT` controls (#625,
/// `crates/patina-tests/tests/retired_registers.rs`). While one is alive,
/// register retirement on this thread treats the highest register each
/// per-pc liveness map calls live as dead, which is what a wrong map does:
/// the register is filled although it is still to be read. In a check build
/// that read panics; elsewhere it reads `UNSPECIFIED` and the program goes
/// wrong somewhere unrelated, or not at all.
///
/// Thread-local, because the test harness runs tests in parallel and an
/// interpreter runs on the thread that drives it.
pub struct DropHighestLive(());

impl DropHighestLive {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        DROP_HIGHEST_LIVE.set(true);
        Self(())
    }
}

impl Drop for DropHighestLive {
    fn drop(&mut self) {
        DROP_HIGHEST_LIVE.set(false);
    }
}

/// The register the switch drops from `roots`: the highest one it calls
/// live inside the frame's window, if the switch is on.
pub(crate) fn dropped(roots: &[u64], num_regs: u16) -> Option<usize> {
    if !DROP_HIGHEST_LIVE.get() {
        return None;
    }
    (0..num_regs as usize)
        .rev()
        .find(|&reg| roots.get(reg / 64).copied().unwrap_or(0) & (1 << (reg % 64)) != 0)
}

/// The switch behind the control of `release_dead_code`'s check (#836,
/// `a_check_build_refuses_a_kept_unit_whose_table_was_not_traced` in
/// `crates/patina-tests/tests/gc_vm.rs`). While one is alive, a collection
/// on this thread traces no unit's link table: `trace_code_ids` returns
/// before it marks any table traced, which is what a holder that keeps a
/// unit and reports no code id leaves. A check build then refuses the first
/// unit the collection keeps. Thread-local, as [`DropHighestLive`] is.
pub struct SkipCodeIds(());

impl SkipCodeIds {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        SKIP_CODE_IDS.set(true);
        Self(())
    }
}

impl Drop for SkipCodeIds {
    fn drop(&mut self) {
        SKIP_CODE_IDS.set(false);
    }
}

/// Whether [`SkipCodeIds`]'s switch is on.
pub(crate) fn skip_code_ids() -> bool {
    SKIP_CODE_IDS.get()
}

/// The switch behind the controls of the load windows' extent (#836,
/// `docs/GC_DESIGN.md` §7, the windows S1 and S2, in
/// `crates/patina-tests/tests/gc_protocol.rs`). While one is alive, the
/// machine on this thread makes the check every poll makes first (#864) at
/// the last point one of the windows covers, as a poll added there would:
/// just before `execute` or `push_frame_in` pushes a loaded unit's frame
/// (S1), or just before `set_suspension` writes an `eval`'s closure into the
/// stub frame's `CALLEE` register (S2). In a check build it panics, since
/// the window is still open there; a window closed before that point lets
/// it pass, which is what a window closed early leaves, with no poll in
/// the gap to see it. Thread-local, as [`DropHighestLive`] is.
pub struct PollInWindow(());

/// Where [`PollInWindow`] checks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowEnd {
    FramePush,
    CalleeWrite,
}

impl PollInWindow {
    /// Check just before a loaded unit's frame is pushed (S1).
    pub fn before_frame_push() -> Self {
        POLL_IN_WINDOW.set(Some(WindowEnd::FramePush));
        Self(())
    }

    /// Check just before an `eval`'s closure is written into `CALLEE` (S2).
    pub fn before_callee_write() -> Self {
        POLL_IN_WINDOW.set(Some(WindowEnd::CalleeWrite));
        Self(())
    }
}

impl Drop for PollInWindow {
    fn drop(&mut self) {
        POLL_IN_WINDOW.set(None);
    }
}

/// Whether [`PollInWindow`] checks before a loaded unit's frame is pushed.
pub(crate) fn poll_before_frame_push() -> bool {
    POLL_IN_WINDOW.get() == Some(WindowEnd::FramePush)
}

/// Whether [`PollInWindow`] checks before an `eval`'s closure is written.
pub(crate) fn poll_before_callee_write() -> bool {
    POLL_IN_WINDOW.get() == Some(WindowEnd::CalleeWrite)
}
