//! Whether a running program has already reported an error.
//!
//! `patina -k` reports each evaluation error and carries on to the next
//! top-level form, so a program can reach an `(exit 0)` after it has already
//! failed; SRFI 64's `(test-exit)` is one. `exit` ends the process on the spot,
//! before the runner that counted the errors can choose a status, so the count
//! has to be visible to `exit` itself.
//!
//! The record is process-wide because an exit status is. Only the runners that
//! carry on past an error set it: an interactive session does not, since a
//! mistake typed at a prompt is not the program failing.

use std::sync::atomic::{AtomicBool, Ordering};

static ERROR_REPORTED: AtomicBool = AtomicBool::new(false);

/// Record that a top-level error was reported and the program carried on.
pub fn note_error_reported() {
    ERROR_REPORTED.store(true, Ordering::Relaxed);
}

/// The status to exit with, given the one a program asked for. A success is
/// withheld from a program that has already reported an error; a failure it
/// asked for is kept as asked.
pub fn status_for_exit(requested: i32) -> i32 {
    if requested == 0 && ERROR_REPORTED.load(Ordering::Relaxed) {
        1
    } else {
        requested
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record is process-wide and cannot be cleared, so this is the one
    /// test that sets it, and it asserts only what holds on either side.
    #[test]
    fn a_success_is_withheld_once_an_error_is_noted_and_a_failure_is_kept() {
        assert_eq!(status_for_exit(3), 3);
        note_error_reported();
        assert_eq!(status_for_exit(0), 1);
        assert_eq!(status_for_exit(3), 3);
    }
}
