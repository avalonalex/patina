//! Whether a running program has already reported an error, and whether an
//! `exit` it called was interrupted by one.
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
//!
//! `exit` runs the after thunk of every outstanding `dynamic-wind` before it
//! ends the process (R7RS 6.14, #336), and one of those thunks can raise an
//! error that nothing handles. That error is reported like any other, and the
//! process still ends, because the program asked it to: the backend that meets
//! the error notes the exit it interrupted ([`note_interrupted_exit`]), and the
//! runner that reports the error ends the process ([`exit_if_interrupted`])
//! rather than carrying on, whatever `-k` or a session would otherwise do.

use crate::EvalError;
use patina_core::TaggedValue;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

static ERROR_REPORTED: AtomicBool = AtomicBool::new(false);

/// The status of an `exit` that an unhandled error interrupted, or [`NO_EXIT`].
static INTERRUPTED_EXIT: AtomicI64 = AtomicI64::new(NO_EXIT);

/// No interrupted exit: outside the range of any `i32` status.
const NO_EXIT: i64 = i64::MIN;

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

/// The status `exit` asks for with `args`, or the error it reports for them.
///
/// No argument or `#t` is 0, `#f` is 1, and an exact integer is itself (the
/// operating system keeps its low byte). It lives in one place because `exit`
/// is implemented three times — intercepted by each backend, which has to
/// unwind before it ends the process, and in the primitive registry for every
/// caller that has no dynamic-wind machinery — and the three must accept the
/// same arguments and refuse the rest in the same words.
pub fn requested_status(args: &[TaggedValue]) -> Result<i32, EvalError> {
    match args {
        [] => Ok(0),
        [obj] if *obj == TaggedValue::TRUE => Ok(0),
        [obj] if *obj == TaggedValue::FALSE => Ok(1),
        [obj] if obj.is_fixnum() => Ok(obj.as_fixnum_unchecked() as i32),
        [_] => Err(EvalError::TypeError(
            "exit expects boolean or integer".to_string(),
        )),
        _ => Err(EvalError::InvalidSyntax(format!(
            "exit expects 0-1 arguments, got {}",
            args.len()
        ))),
    }
}

/// Record that `exit` was on its way to ending the process with `status` when
/// an error that no handler took was raised by one of the after thunks it was
/// running, or by something one of them called.
pub fn note_interrupted_exit(status: i32) {
    INTERRUPTED_EXIT.store(i64::from(status), Ordering::Relaxed);
}

/// Forget a noted interrupted exit. A backend does this as it starts each
/// top-level form, so a note made for an error that something caught after all
/// cannot outlive the form it was made in.
pub fn forget_interrupted_exit() {
    INTERRUPTED_EXIT.store(NO_EXIT, Ordering::Relaxed);
}

/// End the process if the error just reported interrupted an `exit`, and return
/// otherwise. Every runner calls this after reporting an error nothing handled.
///
/// The program asked to exit and has also reported an error, so it gets the
/// status any program that reported an error gets: a success it asked for is
/// withheld and a failure kept, as [`status_for_exit`] decides.
pub fn exit_if_interrupted() {
    if let Some(status) = take_interrupted_exit() {
        note_error_reported();
        std::process::exit(status_for_exit(status));
    }
}

fn take_interrupted_exit() -> Option<i32> {
    let status = INTERRUPTED_EXIT.swap(NO_EXIT, Ordering::Relaxed);
    (status != NO_EXIT).then_some(status as i32)
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

    #[test]
    fn exit_accepts_booleans_and_integers_and_refuses_the_rest() {
        assert_eq!(requested_status(&[]).unwrap(), 0);
        assert_eq!(requested_status(&[TaggedValue::TRUE]).unwrap(), 0);
        assert_eq!(requested_status(&[TaggedValue::FALSE]).unwrap(), 1);
        assert_eq!(requested_status(&[TaggedValue::fixnum(300)]).unwrap(), 300);
        assert!(requested_status(&[TaggedValue::NULL]).is_err());
        assert!(requested_status(&[TaggedValue::TRUE, TaggedValue::TRUE]).is_err());
    }

    /// Process-wide like the error record; this is the only test that notes
    /// one.
    #[test]
    fn an_interrupted_exit_is_taken_once_and_can_be_forgotten() {
        note_interrupted_exit(3);
        assert_eq!(take_interrupted_exit(), Some(3));
        assert_eq!(take_interrupted_exit(), None);
        note_interrupted_exit(0);
        forget_interrupted_exit();
        assert_eq!(take_interrupted_exit(), None);
    }
}
