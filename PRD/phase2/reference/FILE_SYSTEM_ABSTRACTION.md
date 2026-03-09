# File System Abstraction Design

**Status:** Proposed
**Target:** Phase 2 (VM Backend) / Can be implemented earlier
**Goal:** Abstract file system operations behind a trait to enable testing, WASM support, and embedded stdlib

---

## Table of Contents

1. [Motivation](#motivation)
2. [Current File System Usage](#current-file-system-usage)
3. [Proposed Design](#proposed-design)
4. [Implementation Strategies](#implementation-strategies)
5. [Use Cases & Benefits](#use-cases--benefits)
6. [Integration Plan](#integration-plan)
7. [Open Questions](#open-questions)

---

## Motivation

### Problem Statement

Patina currently uses `std::fs` directly throughout the codebase. This creates several limitations:

1. **Testing Difficulty:** Library loading tests require creating actual files on disk
2. **No WASM Support:** Browser environments have no native file system
3. **No Embedded Distribution:** Cannot create single-binary builds with stdlib included
4. **Tight Coupling:** File I/O logic mixed with business logic

### Goals

- **Testability:** Mock file system for fast, deterministic tests
- **Portability:** Run in WASM/browser with virtual file system
- **Embeddability:** Single-binary distribution with embedded stdlib
- **Backend Sharing:** Same abstraction for tree-walker and VM backends
- **Security:** Sandboxed execution with restricted file access

---

## Current File System Usage

### Summary by Crate

| Crate | Operations | Purpose |
|-------|------------|---------|
| `patina-core` | File open (read/write), delete | Port I/O implementation |
| `patina-repl` | Read file to string | Script execution |
| `patina-runtime` | Path storage | Library search paths |
| `patina-tree-walker` | File exists, read, canonicalize | Library loading, R7RS primitives |
| `patina-tests` | Write files | Test setup for library loading |

### Detailed Breakdown

#### 1. Library Loading (`patina-tree-walker/src/library_support.rs`)

```rust
// Find .sld files in search paths
full_path.exists() && full_path.is_file()

// Read library definition
fs::read_to_string(&path)

// Resolve include paths
path.canonicalize()
path.parent()
```

**Requirements:**
- Path existence checking
- File reading (UTF-8)
- Path canonicalization (for circular dependency detection)
- Parent directory resolution

#### 2. Port I/O (`patina-core/src/port.rs`)

```rust
// R7RS file operations
File::open(path)           // open-input-file
File::create(path)         // open-output-file
// + binary variants
```

**Requirements:**
- Streaming file read/write
- Buffered I/O
- Binary and text modes

#### 3. R7RS Primitives (`patina-tree-walker/src/eval/primitives/io/file.rs`)

```rust
// R7RS procedures
Path::new(&path).exists()  // file-exists?
fs::remove_file(&path)     // delete-file
```

**Requirements:**
- File existence check
- File deletion

#### 4. Script Execution (`patina-repl/src/main.rs`)

```rust
fs::read_to_string(filename)  // Load .scm script
```

**Requirements:**
- Read entire file to string

---

## Proposed Design

### Core Trait

```rust
// Location: patina-runtime/src/fs.rs (or new patina-fs crate)

use std::io::{Read, Write, Result as IoResult, Seek};
use std::path::{Path, PathBuf};

/// Metadata about a file system entry
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub is_file: bool,
    pub is_dir: bool,
    pub len: u64,
    pub readonly: bool,
}

/// Handle for streaming file reads
pub trait FileReader: Read + Seek + Send {
    /// Get current file position
    fn position(&self) -> IoResult<u64>;
}

/// Handle for streaming file writes
pub trait FileWriter: Write + Send {
    /// Sync data to storage
    fn sync(&mut self) -> IoResult<()>;
}

/// Abstract file system operations
///
/// All paths are treated as abstract - implementations decide
/// how to interpret them (real paths, virtual paths, etc.)
pub trait FileSystem: Send + Sync {
    // === Metadata Operations ===

    /// Check if a path exists (file or directory)
    fn exists(&self, path: &Path) -> bool;

    /// Check if path points to a file
    fn is_file(&self, path: &Path) -> bool;

    /// Check if path points to a directory
    fn is_dir(&self, path: &Path) -> bool;

    /// Get metadata for a path
    fn metadata(&self, path: &Path) -> IoResult<FileMetadata>;

    // === Path Operations ===

    /// Canonicalize a path (resolve symlinks, make absolute)
    /// For virtual filesystems, may just normalize the path
    fn canonicalize(&self, path: &Path) -> IoResult<PathBuf>;

    /// Get parent directory of a path
    fn parent(&self, path: &Path) -> Option<PathBuf> {
        path.parent().map(|p| p.to_path_buf())
    }

    /// Join path components
    fn join(&self, base: &Path, component: &Path) -> PathBuf {
        base.join(component)
    }

    // === Read Operations ===

    /// Read entire file as UTF-8 string
    fn read_to_string(&self, path: &Path) -> IoResult<String>;

    /// Read entire file as bytes
    fn read(&self, path: &Path) -> IoResult<Vec<u8>>;

    /// Open file for streaming read
    fn open_read(&self, path: &Path) -> IoResult<Box<dyn FileReader>>;

    // === Write Operations ===

    /// Write string to file (creates or truncates)
    fn write(&self, path: &Path, contents: &str) -> IoResult<()>;

    /// Write bytes to file (creates or truncates)
    fn write_bytes(&self, path: &Path, contents: &[u8]) -> IoResult<()>;

    /// Open file for streaming write
    fn open_write(&self, path: &Path) -> IoResult<Box<dyn FileWriter>>;

    /// Append to existing file
    fn open_append(&self, path: &Path) -> IoResult<Box<dyn FileWriter>>;

    // === Modification Operations ===

    /// Delete a file
    fn remove_file(&self, path: &Path) -> IoResult<()>;

    /// Create directory (including parents)
    fn create_dir_all(&self, path: &Path) -> IoResult<()>;

    /// Remove empty directory
    fn remove_dir(&self, path: &Path) -> IoResult<()>;

    // === Directory Operations ===

    /// List directory contents
    fn read_dir(&self, path: &Path) -> IoResult<Vec<PathBuf>>;
}
```

### Standard Implementations

#### 1. OsFileSystem (Real File System)

```rust
/// Wrapper around std::fs for real file system access
pub struct OsFileSystem;

impl OsFileSystem {
    pub fn new() -> Self {
        OsFileSystem
    }
}

impl FileSystem for OsFileSystem {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_to_string(&self, path: &Path) -> IoResult<String> {
        std::fs::read_to_string(path)
    }

    fn open_read(&self, path: &Path) -> IoResult<Box<dyn FileReader>> {
        let file = std::fs::File::open(path)?;
        Ok(Box::new(OsFileReader(std::io::BufReader::new(file))))
    }

    // ... etc
}
```

#### 2. MemoryFileSystem (In-Memory Virtual FS)

```rust
/// In-memory file system for testing and WASM
pub struct MemoryFileSystem {
    files: RwLock<HashMap<PathBuf, Vec<u8>>>,
    directories: RwLock<HashSet<PathBuf>>,
}

impl MemoryFileSystem {
    pub fn new() -> Self {
        Self {
            files: RwLock::new(HashMap::new()),
            directories: RwLock::new(HashSet::new()),
        }
    }

    /// Add a file (convenience for test setup)
    pub fn add_file(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) {
        let mut files = self.files.write().unwrap();
        files.insert(path.as_ref().to_path_buf(), content.as_ref().to_vec());
    }

    /// Add a text file (convenience for test setup)
    pub fn add_text_file(&self, path: impl AsRef<Path>, content: &str) {
        self.add_file(path, content.as_bytes());
    }
}

impl FileSystem for MemoryFileSystem {
    fn exists(&self, path: &Path) -> bool {
        let files = self.files.read().unwrap();
        let dirs = self.directories.read().unwrap();
        files.contains_key(path) || dirs.contains(path)
    }

    fn read_to_string(&self, path: &Path) -> IoResult<String> {
        let files = self.files.read().unwrap();
        match files.get(path) {
            Some(bytes) => String::from_utf8(bytes.clone())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {:?}", path)
            )),
        }
    }

    // ... etc
}
```

#### 3. EmbeddedFileSystem (Compiled-In Files)

```rust
/// File system with embedded files (for single-binary distribution)
pub struct EmbeddedFileSystem {
    /// Embedded files (compile-time, immutable)
    embedded: HashMap<&'static str, &'static [u8]>,
    /// Optional fallback for non-embedded paths
    fallback: Option<Arc<dyn FileSystem>>,
}

impl EmbeddedFileSystem {
    /// Create with standard library embedded
    pub fn with_stdlib() -> Self {
        let mut fs = Self::new();

        // Embed using include_bytes! macro
        fs.embed("lib/scheme/base.sld", include_bytes!("../../../lib/scheme/base.sld"));
        fs.embed("lib/scheme/base/binding.scm", include_bytes!("../../../lib/scheme/base/binding.scm"));
        fs.embed("lib/scheme/base/conditionals.scm", include_bytes!("../../../lib/scheme/base/conditionals.scm"));
        // ... all stdlib files

        fs
    }

    /// Add fallback for user files
    pub fn with_fallback(mut self, fs: Arc<dyn FileSystem>) -> Self {
        self.fallback = Some(fs);
        self
    }
}

impl FileSystem for EmbeddedFileSystem {
    fn read_to_string(&self, path: &Path) -> IoResult<String> {
        // Check embedded first
        let path_str = path.to_string_lossy();
        if let Some(bytes) = self.embedded.get(path_str.as_ref()) {
            return String::from_utf8(bytes.to_vec())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e));
        }

        // Fall back to real FS
        if let Some(ref fallback) = self.fallback {
            fallback.read_to_string(path)
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "not found"))
        }
    }

    // Write operations fail for embedded files
    fn write(&self, path: &Path, contents: &str) -> IoResult<()> {
        if let Some(ref fallback) = self.fallback {
            fallback.write(path, contents)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "embedded filesystem is read-only"
            ))
        }
    }
}
```

#### 4. LayeredFileSystem (Overlay)

```rust
/// Overlay file system - tries layers in order
pub struct LayeredFileSystem {
    layers: Vec<Arc<dyn FileSystem>>,
}

impl LayeredFileSystem {
    pub fn new(layers: Vec<Arc<dyn FileSystem>>) -> Self {
        Self { layers }
    }
}

impl FileSystem for LayeredFileSystem {
    fn read_to_string(&self, path: &Path) -> IoResult<String> {
        for layer in &self.layers {
            match layer.read_to_string(path) {
                Ok(content) => return Ok(content),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "not found in any layer"))
    }
}
```

#### 5. SandboxedFileSystem (Security)

```rust
/// Restricts file access to allowed paths
pub struct SandboxedFileSystem {
    inner: Arc<dyn FileSystem>,
    allowed_read: Vec<PathBuf>,
    allowed_write: Vec<PathBuf>,
}

impl SandboxedFileSystem {
    pub fn new(inner: Arc<dyn FileSystem>) -> Self {
        Self {
            inner,
            allowed_read: vec![],
            allowed_write: vec![],
        }
    }

    pub fn allow_read(&mut self, path: impl AsRef<Path>) {
        self.allowed_read.push(path.as_ref().to_path_buf());
    }

    pub fn allow_write(&mut self, path: impl AsRef<Path>) {
        self.allowed_write.push(path.as_ref().to_path_buf());
    }

    fn check_read(&self, path: &Path) -> IoResult<()> {
        if self.allowed_read.iter().any(|p| path.starts_with(p)) {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("read access denied: {:?}", path)
            ))
        }
    }
}
```

---

## Implementation Strategies

### Strategy A: Gradual Refactoring (Recommended)

Refactor one component at a time, maintaining backwards compatibility:

```
Phase 1: Define trait + OsFileSystem (no changes to existing code)
    ↓
Phase 2: Refactor SchemeLibraryLoader to use FileSystem
    ↓
Phase 3: Refactor Port to use FileSystem
    ↓
Phase 4: Refactor R7RS primitives (file-exists?, delete-file)
    ↓
Phase 5: Add MemoryFileSystem for tests
    ↓
Phase 6: Add EmbeddedFileSystem for single-binary
```

### Strategy B: Feature Flag Approach

Use Cargo features to toggle between implementations:

```toml
[features]
default = ["os-fs"]
os-fs = []          # Real file system
wasm-fs = []        # In-memory for WASM
embedded-stdlib = [] # Include stdlib in binary
```

### Strategy C: Runtime Selection

Allow runtime switching between file systems:

```rust
pub struct RuntimeConfig {
    pub file_system: Arc<dyn FileSystem>,
    // ... other config
}
```

---

## Use Cases & Benefits

### 1. Testing

**Before:**
```rust
#[test]
fn test_library_loading() {
    // Must create actual files
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("test.sld"), "(define-library ...)");

    let loader = SchemeLibraryLoader::new();
    loader.load(&["test"], &[temp_dir.path().to_path_buf()]);
}
```

**After:**
```rust
#[test]
fn test_library_loading() {
    let fs = MemoryFileSystem::new();
    fs.add_text_file("/lib/test.sld", "(define-library ...)");

    let loader = SchemeLibraryLoader::new(Arc::new(fs));
    loader.load(&["test"], &[PathBuf::from("/lib")]);

    // No cleanup needed, no disk I/O, fast & deterministic
}
```

### 2. WASM/Browser Support

```rust
#[cfg(target_arch = "wasm32")]
pub fn create_interpreter() -> TreeWalkInterpreter {
    // Use in-memory FS with embedded stdlib
    let fs = EmbeddedFileSystem::with_stdlib();
    TreeWalkInterpreter::with_filesystem(Arc::new(fs))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn create_interpreter() -> TreeWalkInterpreter {
    // Use real file system
    TreeWalkInterpreter::with_filesystem(Arc::new(OsFileSystem::new()))
}
```

### 3. Single-Binary Distribution

```rust
// Build with: cargo build --features embedded-stdlib

fn main() {
    let fs = EmbeddedFileSystem::with_stdlib()
        .with_fallback(Arc::new(OsFileSystem::new()));

    let interp = TreeWalkInterpreter::with_filesystem(Arc::new(fs));

    // stdlib loaded from binary, user files from disk
}
```

### 4. Sandboxed Execution

```rust
fn create_sandboxed_interpreter(user_dir: &Path) -> TreeWalkInterpreter {
    let mut fs = SandboxedFileSystem::new(Arc::new(OsFileSystem::new()));
    fs.allow_read(user_dir);
    fs.allow_read("/lib/scheme");  // stdlib
    fs.allow_write(user_dir);
    // No write access to /lib/scheme

    TreeWalkInterpreter::with_filesystem(Arc::new(fs))
}
```

### 5. Notebook/Playground Integration

```rust
/// Virtual FS that syncs with browser storage
pub struct BrowserFileSystem {
    memory: MemoryFileSystem,
    storage_key: String,
}

impl BrowserFileSystem {
    pub fn load_from_localstorage(&mut self) {
        // Load saved files from browser localStorage
    }

    pub fn save_to_localstorage(&self) {
        // Persist to browser localStorage
    }
}
```

### 6. Time-Travel Debugging (Future)

```rust
/// Copy-on-write FS for snapshots
pub struct SnapshotFileSystem {
    base: Arc<dyn FileSystem>,
    overlay: MemoryFileSystem,
    snapshot_id: u64,
}

impl SnapshotFileSystem {
    pub fn snapshot(&self) -> Self {
        // Create COW snapshot
    }

    pub fn restore(&mut self, snapshot_id: u64) {
        // Restore to previous state
    }
}
```

### 7. Remote Library Loading (Future)

```rust
/// Load libraries from HTTP
pub struct HttpFileSystem {
    base_url: String,
    cache: MemoryFileSystem,
}

impl FileSystem for HttpFileSystem {
    fn read_to_string(&self, path: &Path) -> IoResult<String> {
        // Check cache first
        if let Ok(content) = self.cache.read_to_string(path) {
            return Ok(content);
        }

        // Fetch from HTTP
        let url = format!("{}/{}", self.base_url, path.display());
        let content = reqwest::blocking::get(&url)?.text()?;

        // Cache for future use
        self.cache.add_text_file(path, &content);

        Ok(content)
    }
}
```

---

## Integration Plan

### Phase 1: Foundation (Can Start Now)

**Changes:**
1. Create `patina-runtime/src/fs/mod.rs` with trait definition
2. Implement `OsFileSystem`
3. Implement `MemoryFileSystem`
4. Add `Arc<dyn FileSystem>` to `Evaluator` and `LibraryRegistry`

**No breaking changes** - existing code continues to work.

### Phase 2: Library Loading

**Changes:**
1. Update `SchemeLibraryLoader` constructor to take `Arc<dyn FileSystem>`
2. Replace `std::fs` calls with trait methods
3. Update `LibraryLoaderRegistry` to propagate FileSystem

**Tests:** Convert library loading tests to use `MemoryFileSystem`.

### Phase 3: Port I/O

**Changes:**
1. Update `Port::open_input_file()` etc. to use `FileSystem`
2. Pass `FileSystem` through to port creation
3. Store `Arc<dyn FileSystem>` in `Port` or use context

**Complexity:** Medium - needs careful handling of streaming I/O.

### Phase 4: R7RS Primitives

**Changes:**
1. Update `file-exists?`, `delete-file` to use `FileSystem`
2. Pass `FileSystem` to primitive context

### Phase 5: Embedded Stdlib

**Changes:**
1. Create build script to generate embedded file map
2. Implement `EmbeddedFileSystem`
3. Add `--features embedded-stdlib` cargo feature

### Phase 6: WASM Target

**Changes:**
1. Add `wasm32` target support
2. Use `MemoryFileSystem` or `EmbeddedFileSystem` for WASM
3. Test in browser environment

---

## Open Questions

### 1. Streaming vs. Whole-File

**Question:** Should `open_read`/`open_write` return streaming handles, or should we only support whole-file operations?

**Trade-offs:**
- Streaming: Required for large files, R7RS compliance (read-char, etc.)
- Whole-file: Simpler trait, easier to implement for virtual FS

**Recommendation:** Support both. Streaming is needed for proper port semantics.

### 2. Async Support

**Question:** Should the trait be async-ready for future network file systems?

**Options:**
- A) Sync-only trait (simpler, current needs)
- B) Async trait with `async_trait` macro
- C) Separate sync and async traits

**Recommendation:** Start with sync (A), add async later if needed.

### 3. Path Semantics

**Question:** How should paths work in virtual file systems?

**Options:**
- A) Always use `/` separator, Unix-style
- B) Platform-native paths
- C) Custom `VirtualPath` type

**Recommendation:** Use `PathBuf` but treat as abstract. Implementations interpret as needed.

### 4. Crate Location

**Question:** Where should the trait live?

**Options:**
- A) `patina-runtime` (co-located with other runtime infrastructure)
- B) New `patina-fs` crate (clean separation)
- C) `patina-core` if we have one

**Recommendation:** Start in `patina-runtime/src/fs/`, extract to crate if it grows.

### 5. Error Types

**Question:** Use `std::io::Error` or custom error type?

**Options:**
- A) `std::io::Error` (standard, works with `?`)
- B) Custom `FsError` (more control, better messages)
- C) Generic `E: std::error::Error`

**Recommendation:** Use `std::io::Error` for familiarity and ecosystem compatibility.

---

## Appendix: Full File System Usage Inventory

### patina-core/src/port.rs
- `File::open()` - textual input (line 187, 215)
- `File::create()` - textual output (line 201, 229)
- `BufReader`/`BufWriter` wrapping

### patina-repl/src/main.rs
- `fs::read_to_string()` - script loading (line 52)

### patina-tree-walker/src/library_support.rs
- `path.exists()`, `path.is_file()` - find .sld files (line 79-80)
- `fs::read_to_string()` - read library definitions (line 101)
- `path.canonicalize()` - circular dependency detection (line 143)
- `path.parent()` - include resolution (line 138)

### patina-tree-walker/src/eval/primitives/io/file.rs
- `Port::open_*_file()` - R7RS file operations (lines 31, 53, 78, 103)
- `Path::exists()` - `file-exists?` (line 125)
- `fs::remove_file()` - `delete-file` (line 144)

### patina-tests/tests/sld_file_loading.rs
- `fs::write()` - test setup

---

## References

- [virtual-fs (Rust crate)](https://docs.rs/virtual-fs) - Similar abstraction for virtual file systems
- [cap-std](https://docs.rs/cap-std) - Capability-based file system access
- [WASI](https://wasi.dev/) - WebAssembly System Interface filesystem
- [include_dir (Rust crate)](https://docs.rs/include_dir) - Embed directory trees at compile time
