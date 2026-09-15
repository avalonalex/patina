//! A session at a terminal, driven through a pseudo-terminal. The line editor
//! edits a terminal line itself, which it never does over a pipe, so these
//! claims cannot be made with one.

#![cfg(unix)]

mod common;

use common::BOTH_BACKENDS;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// A pseudo-terminal: the side a test types into and reads the screen from,
/// and the terminal a child is given.
fn open_pty() -> (File, OwnedFd) {
    let (mut controller, mut terminal) = (0, 0);
    // SAFETY: openpty writes two descriptors into the given integers; the
    // name, settings and size are optional and left null.
    let result = unsafe {
        libc::openpty(
            &mut controller,
            &mut terminal,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(result, 0, "openpty: {}", std::io::Error::last_os_error());
    // SAFETY: both descriptors were just opened, and nothing else owns them.
    unsafe {
        (
            File::from_raw_fd(controller),
            OwnedFd::from_raw_fd(terminal),
        )
    }
}

/// Terminal output with its escape sequences (colours, cursor movement)
/// removed, leaving the text a person would read.
fn text_of(output: &[u8]) -> String {
    let output = String::from_utf8_lossy(output);
    let mut text = String::new();
    let mut chars = output.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            text.push(c);
        }
    }
    text
}

/// The text the child has written to the terminal so far, waiting up to ten
/// seconds for it to include `expected`.
fn screen_until(screen: &mpsc::Receiver<Vec<u8>>, seen: &mut Vec<u8>, expected: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !text_of(seen).contains(expected) {
        let left = deadline.saturating_duration_since(Instant::now());
        match screen.recv_timeout(left) {
            Ok(chunk) => seen.extend(chunk),
            Err(_) => break,
        }
    }
    text_of(seen)
}

/// A form typed and then erased is gone: ending input on the empty line that
/// is left ends the session cleanly, rather than reporting — and running —
/// what was erased.
#[test]
fn a_form_erased_before_the_end_of_input_is_not_reported() {
    for backend in BOTH_BACKENDS {
        let dir = tempfile::tempdir().unwrap();
        let (controller, terminal) = open_pty();
        let mut child = common::patina_command(dir.path(), backend, &[])
            .env("TERM", "xterm")
            .stdin(Stdio::from(terminal.try_clone().unwrap()))
            .stdout(Stdio::from(terminal.try_clone().unwrap()))
            .stderr(Stdio::from(terminal))
            .spawn()
            .expect("failed to spawn patina binary");

        let mut keyboard = controller.try_clone().unwrap();
        let mut answers = controller.try_clone().unwrap();
        let mut reader = controller;
        let (sender, screen) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            // Ends when the child is gone: the read fails or returns nothing.
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                let chunk = buf[..n].to_vec();
                // The editor may ask where the cursor is; say the corner.
                if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = answers.write_all(b"\x1b[1;1R");
                }
                if sender.send(chunk).is_err() {
                    break;
                }
            }
        });

        let mut seen = Vec::new();
        screen_until(&screen, &mut seen, "patina> ");
        // A form that creates a file and one left open, entered as one line,
        // which the editor takes to be unfinished and keeps editing.
        keyboard
            .write_all(
                b"(call-with-output-file \"erased\" (lambda (p) (write-char #\\x p))) (define y\r",
            )
            .unwrap();
        screen_until(&screen, &mut seen, "(define y");
        // Erase all of it, then end input on the empty line.
        keyboard.write_all(&[0x7f; 100]).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        keyboard.write_all(b"\x04").unwrap();

        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().ok();
                child.wait().ok();
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let output = screen_until(&screen, &mut seen, "Goodbye!");
        let status = status.unwrap_or_else(|| {
            panic!("{backend:?}: the session was still running after 10 s:\n{output}")
        });
        assert!(status.success(), "{backend:?}: {output}");
        assert!(output.contains("Goodbye!"), "{backend:?}: {output}");
        assert!(
            !output.contains("Unexpected end of input"),
            "{backend:?}: {output}"
        );
        assert!(
            !dir.path().join("erased").exists(),
            "{backend:?}: the erased form ran"
        );
    }
}
