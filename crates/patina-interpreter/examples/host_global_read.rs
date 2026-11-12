#![allow(
    clippy::disallowed_methods,
    reason = "a probe host evaluates from outside, as an embedder does; the workspace's \
              re-entry rule (#622) is for the evaluator's own crates"
)]

//! The global-read probe of #829 (spike 4b.1) run in a host namespace
//! (#836, 4b.9's host read path): what a host's global read costs.
//!
//! `host_global_read <probe.scm> <N>` evaluates a prelude in the global
//! namespace, `(import (scheme base) (scheme write) (scheme process-context))
//! (define g 42)`, and then the probe file's forms in a fresh host
//! namespace (`Interpreter::new_environment`, whose parent is the global
//! one), with the command line set to `<probe.scm> <N>` as the CLI sets it,
//! so that a probe reads its N as `(cadr (command-line))`. A probe that
//! imports what it reads reads the host namespace's own records; one that
//! reads a name only the global namespace binds, `g` that the prelude
//! defined or `car` that it imported, reads through the host's read-through
//! record for it. It prints only what the probe displays, and exits non-zero
//! on an error, so that `global_read_probe.py` (#782) can drive it as it
//! drives the CLI, with `--backend vm`, and take its slopes from the count
//! of instructions the process retires.

use patina_interpreter::{VmInterpreter, format_interpreter_error};
use std::process::ExitCode;

/// What the global namespace holds before the probe runs: the libraries a
/// probe imports, and the global a read through a host's record reads.
const PRELUDE: &str = "(import (scheme base) (scheme write) (scheme process-context))
(define g 42)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [probe, n] = &args[..] else {
        eprintln!("usage: host_global_read <probe.scm> <N>");
        return ExitCode::from(2);
    };
    let source = match std::fs::read_to_string(probe) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("host_global_read: {probe}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let interpreter = VmInterpreter::new_vm();
    interpreter.set_command_line(probe.clone(), [n.clone()]);
    let (prelude, sources) =
        interpreter.eval_program_with_source_name_owned(PRELUDE, "host-global-read-prelude");
    if let Err(error) = prelude {
        eprintln!("{}", format_interpreter_error(&error, &sources.borrow()));
        return ExitCode::FAILURE;
    }
    let env = interpreter.new_environment();
    // `eval_program_in` keeps no source map, so its error prints plainly.
    if let Err(error) = interpreter.eval_program_in(&env, &source) {
        eprintln!("{probe}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
