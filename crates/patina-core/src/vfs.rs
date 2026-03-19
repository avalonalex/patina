//! Virtual File System abstraction
//!
//! Provides a `FileSystem` trait that abstracts over filesystem operations,
//! enabling testable file I/O and future WASM compilation.
//!
//! Two implementations are provided:
//! - `NativeFs`: Wraps `std::fs` for production use
//! - `MemoryFs`: In-memory filesystem for testing

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Abstraction over filesystem operations.
///
/// All paths are logical — the implementation decides how to resolve them.
/// Implementations must be cheaply cloneable (wrap state in `Arc`).
pub trait FileSystem: Send + Sync + 'static {
    /// Open a file for reading. Returns a buffered reader.
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn ReadPort>>;

    /// Open/create a file for writing. Returns a buffered writer.
    fn open_write(&self, path: &Path) -> io::Result<Box<dyn WritePort>>;

    /// Check whether a path exists and is a file.
    fn file_exists(&self, path: &Path) -> bool;

    /// Check if path is a file (as opposed to directory).
    fn is_file(&self, path: &Path) -> bool {
        self.file_exists(path)
    }

    /// Delete a file.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Read entire file contents as a string (convenience for library loading).
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let mut reader = self.open_read(path)?;
        let mut buf = String::new();
        reader.read_to_string(&mut buf)?;
        Ok(buf)
    }

    /// Canonicalize a path (resolve symlinks, normalize).
    /// In-memory implementations can return the path unchanged.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

/// Trait for readable port streams (file or in-memory).
/// Combines `Read` and `BufRead` for peek/fill_buf support.
pub trait ReadPort: Read + BufRead + Send {}
impl<T: Read + BufRead + Send> ReadPort for T {}

/// Trait for writable port streams (file or in-memory).
pub trait WritePort: Write + Send {
    /// Flush and finalize writes. Called when the port is closed.
    /// Default implementation just flushes.
    fn finalize(&mut self) -> io::Result<()> {
        self.flush()
    }
}

// Blanket impl for types that don't need special finalize behavior
impl WritePort for BufWriter<std::fs::File> {}
impl WritePort for io::Stdout {}
impl WritePort for io::Stderr {}

// =============================================================================
// NativeFs — production filesystem
// =============================================================================

/// Native filesystem implementation wrapping `std::fs`.
#[derive(Clone, Debug)]
pub struct NativeFs;

impl FileSystem for NativeFs {
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn ReadPort>> {
        let file = std::fs::File::open(path)?;
        Ok(Box::new(BufReader::new(file)))
    }

    fn open_write(&self, path: &Path) -> io::Result<Box<dyn WritePort>> {
        let file = std::fs::File::create(path)?;
        Ok(Box::new(BufWriter::new(file)))
    }

    fn file_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

// =============================================================================
// MemoryFs — in-memory filesystem for testing and WASM
// =============================================================================

/// In-memory filesystem for testing and WASM targets.
///
/// Files are stored as `Vec<u8>` in a shared `HashMap`.
/// Thread-safe via `Arc<Mutex<...>>`.
#[derive(Clone, Debug)]
pub struct MemoryFs {
    files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
}

impl Default for MemoryFs {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryFs {
    pub fn new() -> Self {
        MemoryFs {
            files: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Pre-populate a file (for test setup or WASM bundling).
    pub fn add_file(&self, path: impl Into<PathBuf>, content: impl Into<Vec<u8>>) {
        self.files
            .lock()
            .unwrap()
            .insert(path.into(), content.into());
    }

    /// Add a text file (convenience).
    pub fn add_text_file(&self, path: impl Into<PathBuf>, content: &str) {
        self.add_file(path, content.as_bytes().to_vec());
    }

    /// Read back a file's contents (for test assertions).
    pub fn get_file(&self, path: &Path) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(path).cloned()
    }

    /// Read back a text file (for test assertions).
    pub fn get_text_file(&self, path: &Path) -> Option<String> {
        self.get_file(path)
            .map(|bytes| String::from_utf8(bytes).unwrap())
    }
}

/// A writer that captures bytes and writes them back to MemoryFs on finalize.
struct MemoryWriter {
    path: PathBuf,
    files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    buffer: Vec<u8>,
}

impl Write for MemoryWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl WritePort for MemoryWriter {
    fn finalize(&mut self) -> io::Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(self.path.clone(), std::mem::take(&mut self.buffer));
        Ok(())
    }
}

impl Drop for MemoryWriter {
    fn drop(&mut self) {
        // Ensure data is written even if finalize wasn't called explicitly
        if !self.buffer.is_empty() {
            let _ = self.finalize();
        }
    }
}

impl FileSystem for MemoryFs {
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn ReadPort>> {
        let files = self.files.lock().unwrap();
        let data = files.get(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("file not found: {}", path.display()),
            )
        })?;
        Ok(Box::new(Cursor::new(data.clone())))
    }

    fn open_write(&self, path: &Path) -> io::Result<Box<dyn WritePort>> {
        Ok(Box::new(MemoryWriter {
            path: path.to_path_buf(),
            files: self.files.clone(),
            buffer: Vec::new(),
        }))
    }

    fn file_exists(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files
            .lock()
            .unwrap()
            .remove(path)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("file not found: {}", path.display()),
                )
            })
            .map(|_| ())
    }
}

// =============================================================================
// OverlayFs — in-memory overlay on top of a base filesystem
// =============================================================================

/// A layered filesystem: checks an in-memory overlay first, then falls back
/// to a base filesystem (typically `NativeFs`).
///
/// Writes always go to the overlay. Reads check the overlay first, then the base.
/// This is the recommended setup for testing and WASM: bundle standard library
/// files in the base (or pre-populate the overlay), while user file I/O hits
/// the overlay.
#[derive(Clone)]
pub struct OverlayFs {
    overlay: MemoryFs,
    base: Arc<dyn FileSystem>,
}

impl OverlayFs {
    /// Create an overlay filesystem with the given base.
    pub fn new(base: Arc<dyn FileSystem>) -> Self {
        Self {
            overlay: MemoryFs::new(),
            base,
        }
    }

    /// Get a reference to the overlay (for pre-populating files or reading back writes).
    pub fn overlay(&self) -> &MemoryFs {
        &self.overlay
    }
}

impl FileSystem for OverlayFs {
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn ReadPort>> {
        // Try overlay first
        if self.overlay.file_exists(path) {
            return self.overlay.open_read(path);
        }
        self.base.open_read(path)
    }

    fn open_write(&self, path: &Path) -> io::Result<Box<dyn WritePort>> {
        // Writes always go to overlay
        self.overlay.open_write(path)
    }

    fn file_exists(&self, path: &Path) -> bool {
        self.overlay.file_exists(path) || self.base.file_exists(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.overlay.file_exists(path) || self.base.is_file(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        // Remove from overlay (doesn't affect base)
        self.overlay.remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if self.overlay.file_exists(path) {
            return self.overlay.read_to_string(path);
        }
        self.base.read_to_string(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        if self.overlay.file_exists(path) {
            return self.overlay.canonicalize(path);
        }
        self.base.canonicalize(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_fs_nonexistent() {
        let fs = NativeFs;
        assert!(!fs.file_exists(Path::new("/nonexistent/file.txt")));
        assert!(fs.open_read(Path::new("/nonexistent/file.txt")).is_err());
    }

    #[test]
    fn test_memory_fs_read_write() {
        let fs = MemoryFs::new();

        // Write a file
        {
            let mut writer = fs.open_write(Path::new("/test.txt")).unwrap();
            writer.write_all(b"hello world").unwrap();
            writer.finalize().unwrap();
        }

        // Read it back
        {
            let mut reader = fs.open_read(Path::new("/test.txt")).unwrap();
            let mut buf = String::new();
            reader.read_to_string(&mut buf).unwrap();
            assert_eq!(buf, "hello world");
        }

        assert!(fs.file_exists(Path::new("/test.txt")));
        assert!(!fs.file_exists(Path::new("/other.txt")));
    }

    #[test]
    fn test_memory_fs_add_file() {
        let fs = MemoryFs::new();
        fs.add_text_file(PathBuf::from("/lib.sld"), "(define-library (test))");

        let content = fs.read_to_string(Path::new("/lib.sld")).unwrap();
        assert_eq!(content, "(define-library (test))");
    }

    #[test]
    fn test_memory_fs_remove() {
        let fs = MemoryFs::new();
        fs.add_file(PathBuf::from("/tmp.txt"), b"data".to_vec());
        assert!(fs.file_exists(Path::new("/tmp.txt")));

        fs.remove_file(Path::new("/tmp.txt")).unwrap();
        assert!(!fs.file_exists(Path::new("/tmp.txt")));

        // Removing again should error
        assert!(fs.remove_file(Path::new("/tmp.txt")).is_err());
    }

    #[test]
    fn test_memory_fs_drop_writes() {
        let fs = MemoryFs::new();

        // Writer dropped without explicit finalize — should still write
        {
            let mut writer = fs.open_write(Path::new("/auto.txt")).unwrap();
            writer.write_all(b"auto-saved").unwrap();
            // No finalize(), just drop
        }

        let content = fs.get_text_file(Path::new("/auto.txt")).unwrap();
        assert_eq!(content, "auto-saved");
    }

    #[test]
    fn test_memory_fs_buffered_read() {
        let fs = MemoryFs::new();
        fs.add_text_file(PathBuf::from("/lines.txt"), "line1\nline2\nline3");

        let mut reader = fs.open_read(Path::new("/lines.txt")).unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "line1\n");
    }
}
