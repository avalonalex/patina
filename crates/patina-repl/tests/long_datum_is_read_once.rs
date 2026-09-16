//! A datum spanning many lines is read once, not read again after every line
//! that arrives (#341).
//!
//! Reading everything since the datum began after each line costs time
//! proportional to the square of its length. Measured on the commit before
//! this one, in a release build, a datum of 40,000 lines took 44 seconds to
//! `read` from standard input and 2.75 at 10,000 lines, where the same text in
//! a file took 0.02 either way. These run the binary under the suite's
//! ten-second deadline, which a reader of that shape cannot meet; reading each
//! line once, they take about two hundredths of a second.
//!
//! A session reading a pipe is here too, since #348: rustyline's own path for
//! input that is not a terminal walked the whole form again for every line
//! added to it, which took 11.4 seconds for the 40,000-line form below where
//! chibi, Gauche and Chez are flat.

mod common;

use common::{BOTH_BACKENDS, run_with_deadline};

/// Far enough past the old cost to be unmistakable, and cheap to read once.
const LINES: usize = 40_000;

/// A list of [`LINES`] symbols, one to a line.
fn long_list() -> String {
    let mut text = String::with_capacity(LINES * 2 + 4);
    text.push_str("(\n");
    for _ in 0..LINES {
        text.push_str("x\n");
    }
    text.push_str(")\n");
    text
}

/// Run `program` on standard input, on both backends, and check what it
/// printed. `prepare` writes whatever the program reads from the directory it
/// runs in.
fn each_backend_prints(program: &str, expected: &str, prepare: impl Fn(&std::path::Path)) {
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path());
    for backend in BOTH_BACKENDS {
        let (stdout, stderr, ok) = run_with_deadline(dir.path(), backend, Some(program));
        assert!(ok, "{backend:?}: {stderr}");
        assert_eq!(stdout, expected, "{backend:?}: {stderr}");
    }
}

/// A program on standard input holding a datum of 40,000 lines.
///
/// The datum is behind `#;`, so what is measured is reading it: evaluating a
/// literal list that long overflows the stack of a debug build, which is the
/// evaluator's depth and nothing to do with the reader.
#[test]
fn a_long_datum_in_a_program_on_standard_input() {
    let program = format!(
        "(import (scheme base) (scheme write))\n#;{}(display 'read-it-all)\n",
        long_list()
    );
    each_backend_prints(&program, "read-it-all", |_| {});
}

/// `read` takes a 40,000-line datum from standard input, where the program and
/// the datum it reads share the one stream.
#[test]
fn a_long_datum_read_from_standard_input() {
    let program = format!(
        "(import (scheme base) (scheme read) (scheme write))\n\
         (display (length (read)))\n{}",
        long_list()
    );
    each_backend_prints(&program, &LINES.to_string(), |_| {});
}

/// `read` takes one from a file port, the other reader of a line-oriented
/// source.
#[test]
fn a_long_datum_read_from_a_file_port() {
    let program = "(import (scheme base) (scheme read) (scheme file) (scheme write))\n\
                   (display (length (read (open-input-file \"big.scm\"))))\n";
    each_backend_prints(program, &LINES.to_string(), |dir| {
        std::fs::write(dir.join("big.scm"), long_list()).unwrap();
    });
}

/// A session reading a pipe — an editor's inferior-Scheme buffer, or a program
/// driving Patina — is given a form of 40,000 lines.
///
/// It is the reading that is timed here: the form is behind `#;` for the
/// reason the program case gives, and what the session prints afterwards shows
/// it got past it.
#[test]
fn a_long_datum_in_a_session_reading_a_pipe() {
    let dir = tempfile::tempdir().unwrap();
    let program = format!(
        "(import (scheme base) (scheme write))\n#;{}(display 'read-it-all)\n",
        long_list()
    );
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("-i");
        let (stdout, stderr, ok) = run_with_deadline(dir.path(), &args, Some(&program));
        assert!(ok, "{backend:?}: {stderr}");
        assert!(
            stdout.contains("read-it-all"),
            "{backend:?}: {stdout} {stderr}"
        );
    }
}
