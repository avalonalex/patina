//! Output a program writes to a file port it never closes is kept when the
//! program ends, however it ends (#343).
//!
//! These run the binary: what is at stake is the process ending, which no
//! in-process driver does. Every case runs on both backends.
//!
//! # The oracle
//!
//! Measured 2026-09-15 against chibi 0.12 and Gauche 0.9.15, with a program
//! that opens `out.txt`, writes `hello` to it, and ends without closing it:
//!
//! | The program ends | chibi | Gauche |
//! |---|---|---|
//! | by running off its end, from a file or from standard input | kept | kept |
//! | with an uncaught error | kept | kept |
//! | by `exit`, from a file or from standard input | kept | kept |
//! | by an `exit` whose after thunk raises an uncaught error | kept | kept |
//! | by `emergency-exit` | kept | lost |
//! | having written to a binary file port | kept | kept |
//!
//! `emergency-exit` follows chibi. R7RS has it skip the outstanding
//! `dynamic-wind` after thunks, and says nothing that would make it drop the
//! program's output.

mod common;

use common::{BOTH_BACKENDS, patina_command, run_with_deadline_status};

/// Opens `out.txt` and writes `hello` to it, leaving the port open.
const OPEN: &str = "(import (scheme base) (scheme file) (scheme process-context))\n\
                    (define p (open-output-file \"out.txt\"))\n\
                    (write-string \"hello\" p)\n";

#[derive(Clone, Copy, Debug)]
enum Source {
    File,
    Stdin,
}

/// Run `program` with `flags` on `backend`, as a file or on standard input,
/// and return what `out.txt` holds afterwards, the exit code and stderr.
fn run(backend: &[&str], flags: &[&str], program: &str, source: Source) -> (String, i32, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut args = backend.to_vec();
    args.extend_from_slice(flags);
    let input = match source {
        Source::File => {
            std::fs::write(dir.path().join("p.scm"), program).unwrap();
            args.push("p.scm");
            None
        }
        Source::Stdin => Some(program),
    };
    let (_, stderr, status) = run_with_deadline_status(dir.path(), &args, input);
    let code = status
        .code()
        .unwrap_or_else(|| panic!("killed by a signal: {status}\n{stderr}"));
    let out = std::fs::read_to_string(dir.path().join("out.txt")).unwrap_or_default();
    (out, code, stderr)
}

/// chibi and Gauche.
#[test]
fn output_left_in_an_open_file_port_is_kept_however_the_program_ends() {
    let interrupted = "(dynamic-wind (lambda () #f) (lambda () (exit 3)) (lambda () (car 1)))\n";
    for backend in BOTH_BACKENDS {
        for (ending, flags, tail, source, status) in [
            ("off its end", &[][..], "", Source::File, 0),
            ("off its end", &[][..], "", Source::Stdin, 0),
            (
                "off its end, in a session",
                &["-i"][..],
                "",
                Source::Stdin,
                0,
            ),
            (
                "with an uncaught error",
                &[][..],
                "(car 1)\n",
                Source::File,
                1,
            ),
            (
                "with an uncaught error",
                &[][..],
                "(car 1)\n",
                Source::Stdin,
                1,
            ),
            (
                "with an error, under -k",
                &["-k"][..],
                "(car 1)\n",
                Source::File,
                1,
            ),
            ("by exit", &[][..], "(exit)\n", Source::File, 0),
            ("by exit", &[][..], "(exit 4)\n", Source::Stdin, 4),
            (
                "by exit, in a session",
                &["-i"][..],
                "(exit 4)\n",
                Source::Stdin,
                4,
            ),
            (
                "by an interrupted exit",
                &[][..],
                interrupted,
                Source::File,
                3,
            ),
            (
                "by an interrupted exit, under -k",
                &["-k"][..],
                interrupted,
                Source::Stdin,
                3,
            ),
        ] {
            let (out, code, stderr) = run(backend, flags, &format!("{OPEN}{tail}"), source);
            assert_eq!(
                (out.as_str(), code),
                ("hello", status),
                "{backend:?} {source:?} {ending}: {stderr}"
            );
        }

        let binary = "(import (scheme base) (scheme file))\n\
                      (define p (open-binary-output-file \"out.txt\"))\n\
                      (write-u8 104 p)\n";
        let (out, code, stderr) = run(backend, &[], binary, Source::File);
        assert_eq!(
            (out.as_str(), code),
            ("h", 0),
            "{backend:?} binary: {stderr}"
        );
    }
}

/// chibi; Gauche loses the output.
#[test]
fn emergency_exit_keeps_it_too() {
    let program = format!(
        "{OPEN}(dynamic-wind (lambda () #f) (lambda () (emergency-exit 5)) (lambda () (display \"after\")))\n"
    );
    for backend in BOTH_BACKENDS {
        for source in [Source::File, Source::Stdin] {
            let (out, code, stderr) = run(backend, &[], &program, source);
            assert_eq!(
                (out.as_str(), code),
                ("hello", 5),
                "{backend:?} {source:?}: {stderr}"
            );
        }
    }
}

/// Output that cannot be written as the program ends is reported, and the
/// program has failed. Here the file is `/dev/stdout`, a pipe whose reader has
/// gone, so writing it out fails with a broken pipe.
#[test]
fn output_that_cannot_be_written_at_the_end_is_reported() {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    for backend in BOTH_BACKENDS {
        for (ending, tail, status) in [
            ("off its end", "", 1),
            ("by exit", "(exit)\n", 1),
            ("by exit with a status", "(exit 4)\n", 4),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let program = format!(
                "(import (scheme base) (scheme file) (scheme process-context))\n\
                 (define p (open-output-file \"/dev/stdout\"))\n\
                 (write-string \"hello\" p)\n{tail}"
            );
            std::fs::write(dir.path().join("p.scm"), program).unwrap();
            let mut args = backend.to_vec();
            args.push("p.scm");
            let output = patina_command(dir.path(), &args, &[])
                .stdin(std::process::Stdio::null())
                .stdout(writer.try_clone().unwrap())
                .output()
                .unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.code(),
                Some(status),
                "{backend:?} {ending}: {stderr}"
            );
            assert!(
                stderr.contains("could not write the output to /dev/stdout"),
                "{backend:?} {ending}: {stderr}"
            );
        }
    }
}
