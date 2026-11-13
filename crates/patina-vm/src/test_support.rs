//! Test-only hooks into the VM, compiled only with the `test-support`
//! feature, which only `patina-tests`' [dev-dependencies] enable. Resolver 2
//! keeps a dev-dependency's features out of normal builds, so no build that
//! ships has any of this. Each hook is a switch per test thread, which a
//! check build's positive control turns on to make the machine do what the
//! check exists to catch.

use std::cell::Cell;
use std::rc::Rc;

use crate::types::CodeObject;

thread_local! {
    static DROP_HIGHEST_LIVE: Cell<bool> = const { Cell::new(false) };
    static SKIP_CODE_IDS: Cell<Option<Keeper>> = const { Cell::new(None) };
    static POLL_IN_WINDOW: Cell<Option<WindowEnd>> = const { Cell::new(None) };
    static SKIP_LINK_ENTRIES: Cell<bool> = const { Cell::new(false) };
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

/// The switch behind the controls of `release_dead_code`'s check (#836,
/// the `a_check_build_refuses_…` tests in
/// `crates/patina-tests/tests/gc_vm.rs`). While one is alive, a collection
/// on this thread passes over each reported code id whose code one kind of
/// keeper keeps, before `trace_code_ids` marks its unit's table traced:
/// what a keeper of that kind that reported no code id would leave. A check
/// build then refuses the first unit the collection kept with its table
/// untraced. One kind for each of `unit_in_use`'s two conditions, so that a
/// check that covered only the units one of them keeps, or only some kept
/// units, fails a control. Thread-local, as [`DropHighestLive`] is.
pub struct SkipCodeIds(());

/// What keeps the code whose ids [`SkipCodeIds`] passes over.
#[derive(Clone, Copy)]
enum Keeper {
    /// A live closure (`CodeObject::live_closures`).
    Closure,
    /// Something outside the code store that holds the code (a second
    /// `Rc`): a frame, a captured continuation's frame, the running code.
    Holder,
}

impl SkipCodeIds {
    /// Pass over an id whose code a live closure keeps.
    pub fn kept_by_closures() -> Self {
        SKIP_CODE_IDS.set(Some(Keeper::Closure));
        Self(())
    }

    /// Pass over an id whose code a frame, a captured continuation's frame
    /// or the running code keeps.
    pub fn kept_by_frames() -> Self {
        SKIP_CODE_IDS.set(Some(Keeper::Holder));
        Self(())
    }
}

impl Drop for SkipCodeIds {
    fn drop(&mut self) {
        SKIP_CODE_IDS.set(None);
    }
}

/// Whether [`SkipCodeIds`]'s switch passes over an id that names `code`.
pub(crate) fn skip_code_id(code: &Rc<CodeObject>) -> bool {
    match SKIP_CODE_IDS.get() {
        None => false,
        Some(Keeper::Closure) => code.live_closures.get() > 0,
        Some(Keeper::Holder) => Rc::strong_count(code) > 1,
    }
}

/// The switch behind the controls of the load windows' extent (#836,
/// `docs/GC_DESIGN.md` §7, the windows S1 and S2, in
/// `crates/patina-tests/tests/gc_protocol.rs`). While one is alive, the
/// machine on this thread makes the check every poll makes first (#864) at
/// the last point one of the windows covers, as a poll added there would:
/// just before `execute` or `execute_nested` pushes a loaded unit's frame
/// (S1, `push_top_frame`), or just before `set_suspension` writes an
/// `eval`'s closure into the stub frame's `CALLEE` register (S2). A test
/// can turn it on from inside a running form, so that the first frame
/// pushed with it on is a nested loop's. In a check build it panics, since
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

/// The switch behind the control of the placeholders a link table keeps
/// (#836, `a_check_build_refuses_a_placeholder_the_pass_dropped_while_linked`
/// in `crates/patina-tests/tests/gc_vm.rs`). While one is alive, a
/// collection on this thread marks each unit's table traced this epoch, as
/// it does, and then passes over what the table would mark: its entries,
/// and with them the placeholders and weak read-through records nothing
/// else marks, which the pass at the sweep's start then drops and the sweep
/// frees, although the unit is kept and its code can read them. A check
/// build refuses the next read through such an entry as a read of a freed
/// or reused pair. `release_dead_code`'s check stays quiet, since each kept
/// unit's table was marked traced. The whole trace is passed over, so the
/// table's namespace goes unmarked from it too, which the control does not
/// notice: its namespace is the global one, which the machine roots.
/// Thread-local, as [`DropHighestLive`] is.
pub struct SkipLinkEntries(());

impl SkipLinkEntries {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        SKIP_LINK_ENTRIES.set(true);
        Self(())
    }
}

impl Drop for SkipLinkEntries {
    fn drop(&mut self) {
        SKIP_LINK_ENTRIES.set(false);
    }
}

/// Whether [`SkipLinkEntries`]'s switch passes over the entries of a table
/// a collection has marked traced.
pub(crate) fn skip_link_entries() -> bool {
    SKIP_LINK_ENTRIES.get()
}
