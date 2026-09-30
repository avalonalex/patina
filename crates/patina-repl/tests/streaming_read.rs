//! #371: datum reads pull only the input they need, sharing the port cursor.

mod common;

use common::{BOTH_BACKENDS, patina_command, run_with_deadline};
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn read_answers_before_the_next_datum_newline_or_eof_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        ("1 ", "1"),
        ("()", "()"),
        ("\"ok\"", "\"ok\""),
        ("|UP|", "UP"),
        ("'x ", "'x"),
        ("#; #; gone skipped #!fold-case STRAẞE ", "strasse"),
        ("#true ", "#t"),
        ("+ ", "+"),
        ("#u8(1 2)", "#u8(1 2)"),
    ];
    std::fs::write(
        dir.path().join("read.scm"),
        format!(
            "(import (scheme base) (scheme read) (scheme write))
         (do ((i 0 (+ i 1))) ((= i {}))
           (write (read)) (newline) (flush-output-port))",
            cases.len()
        ),
    )
    .unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.push("read.scm");
        let mut child = patina_command(dir.path(), &args, &[])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let drain = std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        for (text, expected) in cases {
            input.write_all(text.as_bytes()).unwrap();
            input.flush().unwrap();
            // Keep stdin open and send nothing else until read has answered.
            let result = receiver.recv_timeout(Duration::from_secs(10));
            if !matches!(&result, Ok(Ok(line)) if line == expected) {
                child.kill().ok();
                drop(input);
                let output = child.wait_with_output().unwrap();
                drain.join().unwrap();
                panic!(
                    "{backend:?}, {text:?}: {result:?}; {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        drop(input);
        let output = child.wait_with_output().unwrap();
        drain.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn many_datums_on_one_line_finish_on_every_port_and_backend() {
    let dir = tempfile::tempdir().unwrap();
    // Longer identifiers expose copying the unread string tail as well as
    // re-tokenizing a file's whole line. The old paths exceed the deadline;
    // the streaming path reads each character once.
    let count = 80_000;
    let data = "abcdefghijklmnopqrstuvwxyz ".repeat(count);
    std::fs::write(dir.path().join("data.scm"), &data).unwrap();
    for source in [
        "(open-input-string data)",
        "(open-input-bytevector (string->utf8 data))",
        "(open-input-file \"data.scm\")",
        "(current-input-port)",
    ] {
        let program = format!(
            "(import (scheme base) (scheme file) (scheme read) (scheme write))
             (define data (read-string {} (open-input-file \"data.scm\")))
             (define p {source})
             (write (let loop ((n 0)) (if (eof-object? (read p)) n (loop (+ n 1)))))",
            data.len()
        );
        std::fs::write(dir.path().join("read.scm"), program).unwrap();
        for backend in BOTH_BACKENDS {
            let mut args = backend.to_vec();
            args.push("read.scm");
            let (out, err, ok) = run_with_deadline(dir.path(), &args, Some(&data));
            assert!(ok, "{source}, {backend:?}: {err}");
            assert_eq!(out, count.to_string(), "{source}, {backend:?}: {err}");
        }
    }
}

#[test]
fn read_errors_name_the_port_and_its_position_after_mixed_io() {
    let dir = tempfile::tempdir().unwrap();
    // Byte reads split a Unicode character and CRLF; char, string and line
    // reads must all advance the same position used by the following read.
    let data = "first\r\nλ\r\nαx\n  #!unknown ";
    std::fs::write(dir.path().join("data.scm"), data).unwrap();
    for (source, name) in [
        ("(open-input-file \"data.scm\")", "data.scm"),
        ("(open-input-string data)", "<string port>"),
        (
            "(open-input-bytevector (string->utf8 data))",
            "<bytevector port>",
        ),
        ("(current-input-port)", "<stdin>"),
    ] {
        let consume = if name == "<string port>" {
            "(read-char p) (read-char p) (read-char p)"
        } else {
            "(read-u8 p) (read-bytevector 1 p) (read-u8 p) (read-bytevector 1 p)"
        };
        let program = format!(
            r#"
            (import (scheme base) (scheme file) (scheme read) (scheme write))
            (define data (read-string {} (open-input-file "data.scm")))
            (define p {source})
            (read p) (read-line p)
            {consume}
            (peek-char p) (read-string 1 p) (read-line p)
            (guard (e ((read-error? e) (display (error-object-message e)))) (read p))
        "#,
            data.chars().count()
        );
        std::fs::write(dir.path().join("read.scm"), program).unwrap();
        for backend in BOTH_BACKENDS {
            let mut args = backend.to_vec();
            args.push("read.scm");
            let (out, err, ok) = run_with_deadline(dir.path(), &args, Some(data));
            assert!(ok, "{backend:?}: {err}");
            assert!(
                out.contains(&format!("{name}:4:3:")),
                "{backend:?}: {out} {err}"
            );
            assert!(out.contains("Unknown reader directive: #!unknown"), "{out}");
        }
    }
}

#[test]
fn later_parse_errors_use_file_coordinates_even_at_eof() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("read.scm"),
        r#"
        (import (scheme base) (scheme file) (scheme read) (scheme write))
        (define p (open-input-file "data.scm"))
        (read p) (read p)
        (guard (e ((read-error? e) (display (error-object-message e)))) (read p))
    "#,
    )
    .unwrap();
    for bad in ["(third", "12oops ", "'", "\"unfinished"] {
        std::fs::write(
            dir.path().join("data.scm"),
            format!("first\r\nsecond\r  {bad}"),
        )
        .unwrap();
        for backend in BOTH_BACKENDS {
            let mut args = backend.to_vec();
            args.push("read.scm");
            let (out, err, ok) = run_with_deadline(dir.path(), &args, None);
            assert!(ok, "{backend:?}: {err}");
            assert!(
                out.contains("data.scm:3:3:"),
                "{bad}, {backend:?}: {out} {err}"
            );
        }
    }
}

#[test]
fn runtime_reads_count_program_text_already_consumed_from_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let program = concat!(
        "(import (scheme base) (scheme read) (scheme write))\n",
        "(write (read)) #!fold-case STRAẞE\n",
        "(guard (e ((read-error? e) (display (error-object-message e)))) (read))\n",
        "  #!unknown \n",
    );
    for backend in BOTH_BACKENDS {
        let (out, err, ok) = run_with_deadline(dir.path(), backend, Some(program));
        assert!(ok, "{backend:?}: {err}");
        assert!(out.starts_with("strasse"), "{backend:?}: {out}");
        assert!(out.contains("<stdin>:4:3:"), "{backend:?}: {out} {err}");
    }
}
