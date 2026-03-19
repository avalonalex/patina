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
        (import (scheme file))
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
    assert_program_eval_to(&code, "(#t #t #f #t #f)");
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
        (import (scheme file))
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
