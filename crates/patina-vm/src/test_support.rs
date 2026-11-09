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
