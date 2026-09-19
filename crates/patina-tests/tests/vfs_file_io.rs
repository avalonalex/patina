//! Tests for file I/O operations through the VFS abstraction.
//!
//! These tests verify that Scheme file I/O primitives work correctly,
//! exercising the VFS plumbing end-to-end via the interpreter (NativeFs).

mod common;

use common::assert_program_eval_to;

/// Helper: create a temp file path that won't collide across tests.
fn temp_path(name: &str) -> String {
    let dir = std::env::temp_dir();
    dir.join(format!("patina_vfs_test_{}", name))
        .to_str()
        .unwrap()
        .to_string()
}

/// Helper: ensure a temp file is cleaned up after test.
struct TempFile(String);

impl TempFile {
    fn new(name: &str) -> Self {
        let path = temp_path(name);
        // Clean up any leftover from previous runs
        let _ = std::fs::remove_file(&path);
        TempFile(path)
    }
    fn path(&self) -> &str {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// =============================================================================
// peek-char where the buffered bytes end inside a character (#410)
// =============================================================================
//
// A file port reads through an 8 KiB buffer, and a character can straddle its
// end. `peek-char` used to validate the *whole* buffered chunk to hand back
// one character, so it failed wherever a chunk ended mid-character — on the
// very first peek of an ordinary text file over 8 KiB, with nothing read yet.
//
// Every row uses one file: `a`, then 5000 `λ` — 1 + 10000 bytes, so byte 8192
// is the first half of the 4096th `λ`. Rust writes it, so the bytes are what
// the comment says whatever the port layer does. The expected values are what
// chibi 0.12 and Gauche 0.9.15 both answer, measured 2026-09-19.

/// The file the rows below read, and the Scheme prelude they share: `skip`
/// takes `n` characters off a port.
fn straddling_file(name: &str) -> (TempFile, String) {
    let f = TempFile::new(name);
    let mut bytes = vec![b'a'];
    for _ in 0..5000 {
        bytes.extend_from_slice("λ".as_bytes());
    }
    std::fs::write(f.path(), bytes).unwrap();
    let prelude = format!(
        r#"
        (import (scheme file))
        (define path "{path}")
        (define (skip p n) (do ((i 0 (+ i 1))) ((= i n)) (read-char p)))
        (define (with p f) (let ((r (f p))) (close-port p) r))
        "#,
        path = f.path()
    );
    (f, prelude)
}

/// The face #410 was filed on: the first `peek-char`, nothing read yet. The
/// first character is complete; it is byte 8192 that is not.
#[test]
fn test_peek_char_does_not_decode_past_the_character_it_returns() {
    let (_f, prelude) = straddling_file("peek_first");
    let code = format!("{prelude} (with (open-input-file path) peek-char)");
    assert_program_eval_to(&code, r"#\a");
}

/// The face that makes it more than a one-line fix: 4096 characters in, the
/// buffer holds *only* the first byte of the next one. Decoding just the first
/// character is not enough — the rest of it has not been buffered — and the
/// peek must not consume anything to get it.
#[test]
fn test_peek_char_completes_a_character_the_buffer_holds_half_of() {
    let (_f, prelude) = straddling_file("peek_half");
    let code = format!(
        r#"{prelude}
        (with (open-input-file path)
          (lambda (p)
            (skip p 4096)
            (let* ((a (peek-char p)) (b (read-char p)) (c (read-char p)))
              (map char->integer (list a b c)))))"#
    );
    assert_program_eval_to(&code, "(955 955 955)");
}

/// And the property that rules out the easy repair. Reading the character and
/// parking it in the port's *text* pushback would satisfy the two rows above
/// and lose its bytes to everything that reads bytes, since those never look
/// there — once per boundary, silently. After a peek the byte operations must
/// still see the character's own bytes: `peek-char` consumes nothing, here as
/// anywhere.
#[test]
fn test_peek_char_at_a_buffer_boundary_leaves_the_bytes_for_the_byte_operations() {
    let (_f, prelude) = straddling_file("peek_bytes");
    let code = format!(
        r#"{prelude}
        (define (at-boundary f)
          (with (open-binary-input-file path) (lambda (p) (skip p 4096) (f p))))
        (list
          (at-boundary (lambda (p) (let* ((a (peek-char p)) (b (read-u8 p)) (c (read-u8 p)))
                                     (list (char->integer a) b c))))
          (at-boundary (lambda (p) (let* ((a (peek-char p)) (b (peek-char p)) (c (peek-u8 p)))
                                     (list (char->integer a) (char->integer b) c))))
          (at-boundary (lambda (p) (let* ((a (peek-char p)) (b (read-bytevector 3 p)))
                                     (list (char->integer a) b))))
          (at-boundary (lambda (p) (let* ((a (peek-char p)) (b (read-line p)))
                                     (list (char->integer a) (string-length b))))))"#
    );
    assert_program_eval_to(
        &code,
        "((955 206 187) (955 955 206) (955 #u8(206 187 206)) (955 905))",
    );
}

/// The same defect from the other side: the first character is fine and a
/// byte further along is not text at all.
#[test]
fn test_peek_char_on_a_binary_file_ignores_undecodable_bytes_it_does_not_reach() {
    let f = TempFile::new("peek_binary");
    std::fs::write(f.path(), [b'x', b' ', 0xFF]).unwrap();
    let code = format!(
        r#"
        (import (scheme file))
        (let* ((p (open-binary-input-file "{path}"))
               (a (peek-char p)) (b (read-char p)) (c (read-char p)) (d (read-u8 p)))
          (close-port p)
          (list a b c d))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, r"(#\x #\x #\space 255)");
}

// =============================================================================
// read-bytevector across the end of the buffer (#414)
// =============================================================================
//
// R7RS §6.13.2: `read-bytevector` reads "the next k bytes, or as many as are
// available before the end of file". A short result therefore *means* end of
// file. It used to mean only that the request ran past the port's 8 KiB
// chunk: one `Read::read` call, its count taken as final, and a `BufReader`
// answers a small request from whatever is left in the chunk it has.
//
// The file is 10000 bytes, byte `i` being `i mod 250`, so a row can check
// *which* bytes came back as well as how many. Expected values are what chibi
// 0.12 and Gauche 0.9.15 both answer, measured 2026-09-19.

fn numbered_file(name: &str) -> (TempFile, String) {
    let f = TempFile::new(name);
    let bytes: Vec<u8> = (0..10000).map(|i| (i % 250) as u8).collect();
    std::fs::write(f.path(), bytes).unwrap();
    let prelude = format!(
        r#"
        (import (scheme file))
        (define path "{path}")
        (define (with p f) (let ((r (f p))) (close-port p) r))
        "#,
        path = f.path()
    );
    (f, prelude)
}

/// Two bytes short of the chunk's end, ask for ten. Bytes 8190..8199 are
/// 190..199, since 8190 = 32 * 250 + 190.
#[test]
fn test_read_bytevector_is_not_cut_short_by_the_buffer_boundary() {
    let (_f, prelude) = numbered_file("rbv_boundary");
    let code = format!(
        r#"{prelude}
        (with (open-binary-input-file path)
          (lambda (p) (read-bytevector 8190 p) (read-bytevector 10 p)))"#
    );
    assert_program_eval_to(&code, "#u8(190 191 192 193 194 195 196 197 198 199)");
}

#[test]
fn test_read_bytevector_bang_is_not_cut_short_by_the_buffer_boundary() {
    let (_f, prelude) = numbered_file("rbv_bang_boundary");
    let code = format!(
        r#"{prelude}
        (with (open-binary-input-file path)
          (lambda (p)
            (read-bytevector 8190 p)
            (let* ((bv (make-bytevector 10 0)) (n (read-bytevector! bv p)))
              (list n bv))))"#
    );
    assert_program_eval_to(&code, "(10 #u8(190 191 192 193 194 195 196 197 198 199))");
}

/// What it did to a program: fixed-size records, and the 82nd came back 92
/// bytes long with nothing to say the file had not ended, so every record
/// after it was misframed. A short record is the last one, and only that.
#[test]
fn test_fixed_size_records_stay_framed_across_the_buffer_boundary() {
    let (_f, prelude) = numbered_file("rbv_records");
    let code = format!(
        r#"{prelude}
        (with (open-binary-input-file path)
          (lambda (p)
            (let loop ((records 0) (short 0))
              (let ((r (read-bytevector 100 p)))
                (if (eof-object? r)
                    (list records short)
                    (loop (+ records 1)
                          (if (< (bytevector-length r) 100) (+ short 1) short)))))))"#
    );
    assert_program_eval_to(&code, "(100 0)");
}

/// End of file is still a short read, then the end-of-file object.
#[test]
fn test_read_bytevector_at_the_end_of_a_file_is_still_short() {
    let (_f, prelude) = numbered_file("rbv_eof");
    let code = format!(
        r#"{prelude}
        (with (open-binary-input-file path)
          (lambda (p)
            (read-bytevector 9995 p)
            (let* ((a (read-bytevector 10 p)) (b (read-bytevector 10 p)))
              (list a (eof-object? b)))))"#
    );
    assert_program_eval_to(&code, "(#u8(245 246 247 248 249) #t)");
}

// =============================================================================
// open-output-file / open-input-file round-trip
// =============================================================================

#[test]
fn test_file_write_and_read_back() {
    let f = TempFile::new("write_read");
    let code = format!(
        r#"
        (import (scheme file))
        (let ((p (open-output-file "{path}")))
          (write-string "hello world" p)
          (close-output-port p))
        (let ((p (open-input-file "{path}")))
          (let ((line (read-line p)))
            (close-input-port p)
            line))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "\"hello world\"");
}

#[test]
fn test_file_write_multiple_lines() {
    let f = TempFile::new("multiline");
    let code = format!(
        r#"
        (import (scheme file))
        (let ((p (open-output-file "{path}")))
          (write-string "line1" p) (newline p)
          (write-string "line2" p) (newline p)
          (write-string "line3" p)
          (close-output-port p))
        (let ((p (open-input-file "{path}")))
          (let* ((l1 (read-line p))
                 (l2 (read-line p))
                 (l3 (read-line p)))
            (close-input-port p)
            (list l1 l2 l3)))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(\"line1\" \"line2\" \"line3\")");
}

// =============================================================================
// file-exists? and delete-file
// =============================================================================

#[test]
fn test_file_exists_false_for_nonexistent() {
    assert_program_eval_to(
        r#"
        (import (scheme file))
        (file-exists? "/nonexistent/path/that/does/not/exist.txt")
        "#,
        "#f",
    );
}

#[test]
fn test_file_exists_true_after_create() {
    let f = TempFile::new("exists_check");
    let code = format!(
        r#"
        (import (scheme file))
        (let ((p (open-output-file "{path}")))
          (write-string "data" p)
          (close-output-port p))
        (file-exists? "{path}")
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "#t");
}

#[test]
fn test_delete_file() {
    let f = TempFile::new("delete");
    let code = format!(
        r#"
        (import (scheme file))
        (let ((p (open-output-file "{path}")))
          (write-string "temp" p)
          (close-output-port p))
        (delete-file "{path}")
        (file-exists? "{path}")
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "#f");
}

// =============================================================================
// call-with-input-file / call-with-output-file
// =============================================================================

#[test]
fn test_call_with_output_and_input_file() {
    let f = TempFile::new("call_with");
    let code = format!(
        r#"
        (import (scheme file))
        (call-with-output-file "{path}"
          (lambda (p) (write-string "via-call-with" p)))
        (call-with-input-file "{path}"
          (lambda (p) (read-line p)))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "\"via-call-with\"");
}

// =============================================================================
// with-input-from-file / with-output-to-file (current port rebinding)
// =============================================================================

#[test]
fn test_with_output_to_file_rebinds_port() {
    let f = TempFile::new("with_output");
    let code = format!(
        r#"
        (import (scheme base) (scheme file) (scheme write))
        (with-output-to-file "{path}"
          (lambda () (display "redirected")))
        (call-with-input-file "{path}"
          (lambda (p) (read-line p)))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "\"redirected\"");
}

#[test]
fn test_with_input_from_file_rebinds_port() {
    let f = TempFile::new("with_input");
    // Write a file first, then read via with-input-from-file
    let code = format!(
        r#"
        (import (scheme file) (scheme read))
        (call-with-output-file "{path}"
          (lambda (p) (write-string "(hello world)" p)))
        (with-input-from-file "{path}"
          (lambda () (read)))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(hello world)");
}

// =============================================================================
// Binary I/O
// =============================================================================

#[test]
fn test_binary_file_write_and_read() {
    let f = TempFile::new("binary");
    let code = format!(
        r#"
        (import (scheme file))
        (let ((p (open-binary-output-file "{path}")))
          (write-u8 65 p)
          (write-u8 66 p)
          (write-u8 67 p)
          (close-output-port p))
        (let ((p (open-binary-input-file "{path}")))
          (let* ((a (read-u8 p))
                 (b (read-u8 p))
                 (c (read-u8 p))
                 (d (read-u8 p)))
            (close-input-port p)
            (list a b c (eof-object? d))))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(65 66 67 #t)");
}

// =============================================================================
// Error handling
// =============================================================================

#[test]
fn test_open_nonexistent_file_raises_file_error() {
    assert_program_eval_to(
        r#"
        (import (scheme file))
        (guard (ex ((file-error? ex) 'caught))
          (open-input-file "/nonexistent/surely/missing.txt"))
        "#,
        "caught",
    );
}

#[test]
fn test_delete_nonexistent_file_raises_error() {
    assert_program_eval_to(
        r#"
        (import (scheme file))
        (guard (ex ((file-error? ex) 'caught))
          (delete-file "/nonexistent/surely/missing.txt"))
        "#,
        "caught",
    );
}

// =============================================================================
// Port predicates on file ports
// =============================================================================

#[test]
fn test_file_port_predicates() {
    let f = TempFile::new("predicates");
    let code = format!(
        r#"
        (import (scheme file))
        (let ((p (open-output-file "{path}")))
          (let ((results (list (port? p)
                               (output-port? p)
                               (input-port? p)
                               (textual-port? p)
                               (binary-port? p)
                               (output-port-open? p))))
            (close-output-port p)
            results))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(#t #t #f #t #f #t)");
}

#[test]
fn test_binary_port_predicates() {
    let f = TempFile::new("bin_predicates");
    let code = format!(
        r#"
        (import (scheme file))
        (let ((p (open-binary-input-file "{path}")))
          (let ((results (list (port? p)
                               (input-port? p)
                               (output-port? p)
                               (binary-port? p)
                               (textual-port? p))))
            (close-input-port p)
            results))
        "#,
        path = f.path()
    );
    // Need to create the file first
    std::fs::write(f.path(), b"data").unwrap();
    // The last is `textual-port?`, and `#t`: the textual operations work on a
    // binary port, so it is textual as well as binary, which is what chibi
    // and Gauche answer for a binary file port too (#404; the bytevector
    // half is in `tests/scheme/stdlib/ports.scm`).
    assert_program_eval_to(&code, "(#t #t #f #t #t)");
}

/// R7RS 6.13.1: `input-port-open?` "returns #t if port is still open and
/// capable of performing input" — for an output-only port that is `#f`, not
/// a type error. Larceny's `file` suite maps every port predicate over a
/// freshly opened binary port and died here (family 10, fixed 2026-08-24:
/// `#f` for the other direction, on both predicates). Moved from
/// `standard_ports.rs` when #193 took that file's string-port rows to
/// `tests/scheme/stdlib/ports.scm`: the claim is about a *binary file* port,
/// which a suite file cannot make, and a string-port version would assert the
/// same direction through a different port type.
#[test]
fn input_port_open_on_an_output_only_port_is_false() {
    let f = TempFile::new("open_direction.bin");
    let code = format!(
        r#"
        (import (scheme file))
        (define p (open-binary-output-file "{path}"))
        (define q (open-input-string ""))
        (let ((results (list (output-port-open? p) (input-port-open? p)
                             (input-port-open? q) (output-port-open? q))))
          (close-port p)
          results)
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "(#t #f #t #f)");
}

// =============================================================================
// Port close behavior
// =============================================================================

#[test]
fn test_closed_port_not_open() {
    let f = TempFile::new("closed");
    let code = format!(
        r#"
        (import (scheme file))
        (let ((p (open-output-file "{path}")))
          (close-output-port p)
          (output-port-open? p))
        "#,
        path = f.path()
    );
    assert_program_eval_to(&code, "#f");
}

// =============================================================================
// MemoryFs integration tests (full interpreter with injected VFS)
// =============================================================================

/// Create an OverlayFs: in-memory overlay on top of NativeFs.
/// User file I/O hits the overlay; library loading falls through to NativeFs.
fn make_overlay_fs() -> std::sync::Arc<patina_core::OverlayFs> {
    std::sync::Arc::new(patina_core::OverlayFs::new(std::sync::Arc::new(
        patina_core::NativeFs,
    )))
}

/// Helper: create a tree-walker interpreter backed by an OverlayFs.
fn make_overlay_interp(
    fs: &std::sync::Arc<patina_core::OverlayFs>,
) -> patina_interpreter::TreeWalkInterpreter {
    patina_interpreter::TreeWalkInterpreter::new_tree_walker_with_fs(fs.clone())
}

/// Helper: evaluate a program on an OverlayFs-backed interpreter and return display string.
fn eval_with_overlay_fs(fs: &std::sync::Arc<patina_core::OverlayFs>, code: &str) -> String {
    let interp = make_overlay_interp(fs);
    let result = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("Failed to evaluate program: {}", e));
    interp.display_tagged(result)
}

#[test]
fn test_memfs_read_prepopulated_file() {
    let fs = make_overlay_fs();
    fs.overlay()
        .add_text_file(std::path::PathBuf::from("/data.txt"), "hello from memory");

    let result = eval_with_overlay_fs(
        &fs,
        r#"
        (import (scheme file))
        (call-with-input-file "/data.txt"
          (lambda (p) (read-line p)))
        "#,
    );
    assert_eq!(result, "\"hello from memory\"");
}

#[test]
fn test_memfs_write_then_read() {
    let fs = make_overlay_fs();

    let result = eval_with_overlay_fs(
        &fs,
        r#"
        (import (scheme file))
        (call-with-output-file "/out.txt"
          (lambda (p) (write-string "written by scheme" p)))
        (call-with-input-file "/out.txt"
          (lambda (p) (read-line p)))
        "#,
    );
    assert_eq!(result, "\"written by scheme\"");

    // Also verify from Rust side
    let content = fs
        .overlay()
        .get_text_file(std::path::Path::new("/out.txt"))
        .unwrap();
    assert_eq!(content, "written by scheme");
}

#[test]
fn test_memfs_file_exists() {
    let fs = make_overlay_fs();
    fs.overlay()
        .add_text_file(std::path::PathBuf::from("/exists.txt"), "yes");

    let result = eval_with_overlay_fs(
        &fs,
        r#"
        (import (scheme file))
        (list (file-exists? "/exists.txt")
              (file-exists? "/missing.txt"))
        "#,
    );
    assert_eq!(result, "(#t #f)");
}

#[test]
fn test_memfs_delete_file() {
    let fs = make_overlay_fs();
    fs.overlay()
        .add_text_file(std::path::PathBuf::from("/doomed.txt"), "bye");

    let result = eval_with_overlay_fs(
        &fs,
        r#"
        (import (scheme file))
        (delete-file "/doomed.txt")
        (file-exists? "/doomed.txt")
        "#,
    );
    assert_eq!(result, "#f");

    // Confirm from Rust side
    assert!(
        fs.overlay()
            .get_file(std::path::Path::new("/doomed.txt"))
            .is_none()
    );
}

#[test]
fn test_memfs_open_nonexistent_raises_error() {
    let fs = make_overlay_fs();

    let result = eval_with_overlay_fs(
        &fs,
        r#"
        (import (scheme file))
        (guard (ex ((file-error? ex) 'caught))
          (open-input-file "/no-such-file.txt"))
        "#,
    );
    assert_eq!(result, "caught");
}

#[test]
fn test_memfs_binary_io() {
    let fs = make_overlay_fs();

    let result = eval_with_overlay_fs(
        &fs,
        r#"
        (import (scheme file))
        (let ((p (open-binary-output-file "/bin.dat")))
          (write-u8 1 p)
          (write-u8 2 p)
          (write-u8 3 p)
          (close-output-port p))
        (let ((p (open-binary-input-file "/bin.dat")))
          (let* ((a (read-u8 p))
                 (b (read-u8 p))
                 (c (read-u8 p)))
            (close-input-port p)
            (list a b c)))
        "#,
    );
    assert_eq!(result, "(1 2 3)");
}

#[test]
fn test_memfs_with_output_to_file() {
    let fs = make_overlay_fs();

    let result = eval_with_overlay_fs(
        &fs,
        r#"
        (import (scheme base) (scheme file) (scheme write))
        (with-output-to-file "/redirected.txt"
          (lambda () (display "hello")))
        (call-with-input-file "/redirected.txt"
          (lambda (p) (read-line p)))
        "#,
    );
    assert_eq!(result, "\"hello\"");
}

#[test]
fn test_memfs_with_input_from_file() {
    let fs = make_overlay_fs();
    fs.overlay()
        .add_text_file(std::path::PathBuf::from("/input.txt"), "(+ 1 2)");

    let result = eval_with_overlay_fs(
        &fs,
        r#"
        (import (scheme file) (scheme read))
        (with-input-from-file "/input.txt"
          (lambda () (read)))
        "#,
    );
    assert_eq!(result, "(+ 1 2)");
}

// =============================================================================
// MemoryFs unit tests (Port-level, no interpreter needed)
// =============================================================================

#[test]
fn test_memory_fs_port_binary_roundtrip() {
    use patina_core::Port;
    use patina_core::vfs::MemoryFs;

    let fs = MemoryFs::new();

    // Write binary data
    {
        let port = Port::open_binary_output_file("/binary.dat", &fs).unwrap();
        port.write_u8(0xFF).unwrap();
        port.write_u8(0x00).unwrap();
        port.write_u8(0x42).unwrap();
        port.close();
    }

    // Read it back
    {
        let port = Port::open_binary_input_file("/binary.dat", &fs).unwrap();
        assert_eq!(port.read_u8().unwrap(), Some(0xFF));
        assert_eq!(port.read_u8().unwrap(), Some(0x00));
        assert_eq!(port.read_u8().unwrap(), Some(0x42));
        assert_eq!(port.read_u8().unwrap(), None); // EOF
    }
}

#[test]
fn test_memory_fs_port_text_peek() {
    use patina_core::Port;
    use patina_core::vfs::MemoryFs;

    let fs = MemoryFs::new();
    fs.add_text_file(std::path::PathBuf::from("/peek.txt"), "abc");

    let port = Port::open_input_file("/peek.txt", &fs).unwrap();
    assert_eq!(port.peek_char().unwrap(), Some('a'));
    assert_eq!(port.peek_char().unwrap(), Some('a')); // still 'a'
    assert_eq!(port.read_char().unwrap(), Some('a'));
    assert_eq!(port.peek_char().unwrap(), Some('b'));
    assert_eq!(port.read_char().unwrap(), Some('b'));
    assert_eq!(port.read_char().unwrap(), Some('c'));
    assert_eq!(port.read_char().unwrap(), None);
}

#[test]
fn test_memory_fs_port_readline() {
    use patina_core::Port;
    use patina_core::vfs::MemoryFs;

    let fs = MemoryFs::new();
    fs.add_text_file(
        std::path::PathBuf::from("/lines.txt"),
        "first\nsecond\nthird",
    );

    let port = Port::open_input_file("/lines.txt", &fs).unwrap();
    assert_eq!(port.read_line().unwrap(), Some("first\n".to_string()));
    assert_eq!(port.read_line().unwrap(), Some("second\n".to_string()));
    assert_eq!(port.read_line().unwrap(), Some("third".to_string()));
    assert_eq!(port.read_line().unwrap(), None);
}

#[test]
fn test_memory_fs_port_bytevector_io() {
    use patina_core::Port;
    use patina_core::vfs::MemoryFs;

    let fs = MemoryFs::new();

    // Write bytevector data
    {
        let port = Port::open_binary_output_file("/bv.dat", &fs).unwrap();
        port.write_bytevector(&[10, 20, 30, 40, 50]).unwrap();
        port.close();
    }

    // Read it back in chunks
    {
        let port = Port::open_binary_input_file("/bv.dat", &fs).unwrap();
        let chunk1 = port.read_bytevector(3).unwrap();
        assert_eq!(chunk1, Some(vec![10, 20, 30]));
        let chunk2 = port.read_bytevector(10).unwrap(); // more than remaining
        assert_eq!(chunk2, Some(vec![40, 50]));
        let chunk3 = port.read_bytevector(1).unwrap();
        assert_eq!(chunk3, None); // EOF
    }
}

#[test]
fn test_memory_fs_port_open_nonexistent_errors() {
    use patina_core::Port;
    use patina_core::vfs::MemoryFs;

    let fs = MemoryFs::new();
    let result = Port::open_input_file("/does_not_exist.txt", &fs);
    assert!(result.is_err());
}

#[test]
fn test_memory_fs_port_unicode() {
    use patina_core::Port;
    use patina_core::vfs::MemoryFs;

    let fs = MemoryFs::new();
    fs.add_text_file(std::path::PathBuf::from("/unicode.txt"), "λ→∀");

    let port = Port::open_input_file("/unicode.txt", &fs).unwrap();
    assert_eq!(port.read_char().unwrap(), Some('λ'));
    assert_eq!(port.read_char().unwrap(), Some('→'));
    assert_eq!(port.read_char().unwrap(), Some('∀'));
    assert_eq!(port.read_char().unwrap(), None);
}

#[test]
fn test_memory_fs_overwrite_file() {
    use patina_core::Port;
    use patina_core::vfs::MemoryFs;

    let fs = MemoryFs::new();

    // Write version 1
    {
        let port = Port::open_output_file("/overwrite.txt", &fs).unwrap();
        port.write_string("version1").unwrap();
        port.close();
    }

    // Overwrite with version 2
    {
        let port = Port::open_output_file("/overwrite.txt", &fs).unwrap();
        port.write_string("version2").unwrap();
        port.close();
    }

    // Read back — should be version 2
    {
        let port = Port::open_input_file("/overwrite.txt", &fs).unwrap();
        let line = port.read_line().unwrap();
        assert_eq!(line, Some("version2".to_string()));
    }
}
