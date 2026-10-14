//! Ctrl-C while a program runs (#701): SIGINT posts an interrupt to the
//! interpreter running it, which raises `&interrupt` at its next poll,
//! rather than ending the process or reaching the line editor's handler.
//!
//! A script, a program on standard input and `-p`'s expressions forward it
//! for their whole run, and end with status 130 on an interrupt nothing
//! handled (`patina_runtime::exit_status::INTERRUPTED_STATUS`). A session
//! forwards it only while an evaluation runs, so that Ctrl-C at the prompt
//! still abandons the line being typed.
//!
//! A program blocked in a read, or in one long primitive, takes the
//! interrupt only when that returns; a second Ctrl-C while the first waits
//! ends the process by SIGINT's default action
//! (`patina_core::InterruptHandle::forward_sigint`).

use patina_interpreter::{Backend, Interpreter};

/// While it lives, SIGINT interrupts the interpreter it was made for. Where
/// it cannot be forwarded, on a platform without `sigaction` or while
/// something else forwards it, SIGINT keeps its disposition.
pub struct CtrlC {
    #[cfg(unix)]
    _forwarding: Option<patina_core::SigintForwarding>,
}

impl CtrlC {
    /// For a program's whole run.
    pub fn for_program<B: Backend>(interp: &Interpreter<B>) -> Self {
        #[cfg(not(unix))]
        let _ = interp;
        Self {
            #[cfg(unix)]
            _forwarding: interp.interrupt_handle().forward_sigint().ok(),
        }
    }

    /// For one evaluation at a session's prompt: SIGINT is forwarded while
    /// `evaluate` runs, and the handler it replaced is back once it returns.
    /// An interrupt posted too late to stop the evaluation, after the last
    /// poll of its last return, is dropped then, so that it does not stop the
    /// next one.
    pub fn during<B: Backend, T>(interp: &Interpreter<B>, evaluate: impl FnOnce() -> T) -> T {
        let forwarding = Self::for_program(interp);
        let result = evaluate();
        drop(forwarding);
        interp.heap().borrow().mutator().take_signal();
        result
    }
}
