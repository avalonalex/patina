# Virtual File System Abstraction

**Status:** Proposed
**Created:** 2026-03-19
**Motivation:** Enable testable file I/O, and prepare for WASM compilation by abstracting all filesystem access behind a trait.

## Problem Statement

1. **All file operations use `std::fs` directly.** `Port::open_input_file` calls `File::open()`, `file-exists?` calls `Path::exists()`, library loading calls `fs::read_to_string()`, etc. There is no seam for injecting alternative implementations.

2. **File-related changes are hard to test in isolation.** Testing `open-input-file`, `delete-file`, `include`, or library loading requires creating real files on disk. There is no way to run file I/O tests in-memory.

3. **WASM has no filesystem.** Compiling patina to `wasm32-wasi` or `wasm32-unknown-unknown` requires replacing `std::fs` with either a WASI shim or an in-memory filesystem. Without an abstraction boundary, this would require invasive changes across many crates.

## Current Touchpoints

Every place that calls into `std::fs` or `std::io::stdin/stdout/stderr`:

| Operation | Call site | Crate |
|-----------|-----------|-------|
| `File::open()` | `Port::open_input_file`, `open_binary_input_file` | patina-core |
| `File::create()` | `Port::open_output_file`, `open_binary_output_file` | patina-core |
| `Path::exists()` | `file-exists?` primitive | patina-primitives |
| `fs::remove_file()` | `delete-file` primitive | patina-primitives |
| `fs::read_to_string()` | `load` primitive | patina-primitives |
| `fs::read_to_string()` | `include` / `include-ci` desugaring | patina-frontend |
| `fs::read_to_string()` | `.sld` file loading | patina-frontend |
| `fs::read_to_string()` | `-extras.scm` loading | patina-tree-walker |
| `Path::exists()`, `is_file()` | Library search (`find_sld_file`) | patina-frontend |
| `Path::canonicalize()` | Circular-include detection | patina-frontend |
| `fs::read_to_string()` | Workspace root detection (`Cargo.toml`) | patina-runtime |
| `io::stdin/stdout/stderr` | Default current ports | patina-primitives |
| `BufReader<File>` / `BufWriter<File>` | `FileHandle` in `PortData::File` | patina-core |

## Proposed Design

### Core Trait

Define a `FileSystem` trait in `patina-core` (since `Port` lives there and needs it):

```rust
// crates/patina-core/src/vfs.rs

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// A readable stream (file or in-memory buffer).
pub trait VfsRead: Read + Send {}
impl<T: Read + Send> VfsRead for T {}

/// A writable stream (file or in-memory buffer).
pub trait VfsWrite: Write + Send {}
impl<T: Write + Send> VfsWrite for T {}

/// Abstraction over filesystem operations.
///
/// All paths are logical — the implementation decides how to resolve them.
/// Implementations must be cheaply cloneable (e.g., wrap state in Arc).
pub trait FileSystem: Send + Sync + 'static {
    /// Open a file for reading. Returns a buffered reader.
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn VfsRead>>;

    /// Open/create a file for writing. Returns a buffered writer.
    fn open_write(&self, path: &Path) -> io::Result<Box<dyn VfsWrite>>;

    /// Check whether a path exists and is a file.
    fn file_exists(&self, path: &Path) -> bool;

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

    /// Check if path is a file (as opposed to directory). Default: same as file_exists.
    fn is_file(&self, path: &Path) -> bool {
        self.file_exists(path)
    }
}
```

### Native Implementation

```rust
// crates/patina-core/src/vfs.rs (or vfs/native.rs)

#[derive(Clone)]
pub struct NativeFs;

impl FileSystem for NativeFs {
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn VfsRead>> {
        let file = std::fs::File::open(path)?;
        Ok(Box::new(std::io::BufReader::new(file)))
    }

    fn open_write(&self, path: &Path) -> io::Result<Box<dyn VfsWrite>> {
        let file = std::fs::File::create(path)?;
        Ok(Box::new(std::io::BufWriter::new(file)))
    }

    fn file_exists(&self, path: &Path) -> bool {
        path.exists()
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

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
}
```

### In-Memory Implementation (for testing and WASM)

```rust
// crates/patina-core/src/vfs.rs (or vfs/memory.rs)

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub struct MemoryFs {
    files: Arc<RwLock<HashMap<PathBuf, Vec<u8>>>>,
}

impl MemoryFs {
    pub fn new() -> Self { Self::default() }

    /// Pre-populate a file (for test setup).
    pub fn add_file(&self, path: impl Into<PathBuf>, content: impl Into<Vec<u8>>) {
        self.files.write().unwrap().insert(path.into(), content.into());
    }

    /// Read back a file written during a test (for assertions).
    pub fn get_file(&self, path: &Path) -> Option<Vec<u8>> {
        self.files.read().unwrap().get(path).cloned()
    }
}

impl FileSystem for MemoryFs {
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn VfsRead>> {
        let files = self.files.read().unwrap();
        let data = files.get(path).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("{}", path.display()))
        })?;
        Ok(Box::new(std::io::Cursor::new(data.clone())))
    }

    fn open_write(&self, path: &Path) -> io::Result<Box<dyn VfsWrite>> {
        // Return a writer that captures bytes into the map on drop/flush.
        // (Implementation detail — could use a MemoryWriter wrapper.)
        todo!()
    }

    fn file_exists(&self, path: &Path) -> bool {
        self.files.read().unwrap().contains_key(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files.write().unwrap().remove(path).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("{}", path.display()))
        })?;
        Ok(())
    }
}
```

### Stdio Abstraction

For WASM and testing, stdin/stdout/stderr also need abstraction. Rather than a separate trait, extend `FileSystem`:

```rust
pub trait FileSystem: Send + Sync + 'static {
    // ... file methods above ...

    /// Standard input stream. Default: real stdin.
    fn stdin(&self) -> Box<dyn VfsRead> {
        Box::new(std::io::stdin())
    }

    /// Standard output stream. Default: real stdout.
    fn stdout(&self) -> Box<dyn VfsWrite> {
        Box::new(std::io::stdout())
    }

    /// Standard error stream. Default: real stderr.
    fn stderr(&self) -> Box<dyn VfsWrite> {
        Box::new(std::io::stderr())
    }
}
```

For WASM, these can be redirected to in-memory buffers or JS callbacks.

### Threading the VFS Through the System

The `FileSystem` instance must be available at every call site listed in the touchpoints table. The key question is how to plumb it.

**Approach: `Arc<dyn FileSystem>` stored on shared context objects.**

The VFS handle propagates through existing structures that are already shared:

```
Interpreter<B>
  ├── fs: Arc<dyn FileSystem>          (stored at construction)
  ├── Heap (already shared via Rc<RefCell<Heap>>)
  │   └── (no change — heap doesn't do I/O)
  ├── StandardPipeline
  │   ├── Desugarer                    (needs fs for `include`)
  │   └── SchemeLibraryLoader          (needs fs for .sld loading)
  ├── LibraryRegistry                  (needs fs for search paths)
  ├── Backend (tree-walker or VM)
  │   └── (needs fs for `-extras.scm` loading)
  └── Port construction               (needs fs for open-input/output-file)
```

Concrete changes:

1. **`Port`** — `Port::open_input_file` gains an `fs: &dyn FileSystem` parameter. `FileHandle` changes from `BufReader<File>` to `Box<dyn VfsRead>` (and same for write). This is the most impactful change.

2. **`ApplyContext`** — Add `fn fs(&self) -> &Arc<dyn FileSystem>` method. All I/O primitives in `patina-primitives` access the VFS through context.

3. **`StandardPipeline` / `Desugarer`** — Accept `Arc<dyn FileSystem>` at construction, use it for `include` and `.sld` loading.

4. **`LibraryRegistry`** — Accept `Arc<dyn FileSystem>` at construction, use for search-path resolution.

5. **Current ports** — `Port::stdin()` / `stdout()` / `stderr()` become `Port::stdin(fs)` etc., calling `fs.stdin()`.

### Port Type Changes

```rust
// Before:
pub enum FileHandle {
    Input(BufReader<File>),
    Output(BufWriter<File>),
}

// After:
pub enum FileHandle {
    Input(Box<dyn VfsRead>),
    Output(Box<dyn VfsWrite>),
}
```

Since `Port` already wraps `PortData` in `Rc<RefCell<...>>`, the boxing is not an additional indirection concern. The `Read`/`Write` trait methods on `Port` already go through `RefCell` borrow + match on `PortData` — they just call different concrete types.

## Implementation Plan

### Phase 1: Define trait and native impl (low risk)

1. Add `vfs.rs` to `patina-core` with `FileSystem` trait, `NativeFs`, and `MemoryFs`.
2. No other changes. Everything still works as before.

### Phase 2: Thread VFS into Port (medium risk, most impactful)

1. Change `FileHandle` to use `Box<dyn VfsRead/VfsWrite>`.
2. Change `Port::open_*_file()` to accept `&dyn FileSystem`.
3. Update `file.rs` primitives to get VFS from `ApplyContext`.
4. Add `fn fs(&self) -> &Arc<dyn FileSystem>` to `ApplyContext`.
5. Both backend implementations (`TreeWalkerApplyContext`, `VmApplyContext`) provide the VFS.

### Phase 3: Thread VFS into library loading (medium risk)

1. `SchemeLibraryLoader::parse_sld_file_*` → use `fs.read_to_string()`.
2. `Desugarer` include handling → use `fs.read_to_string()`.
3. `LibraryRegistry` → use `fs.file_exists()` for search paths.
4. Tree-walker extras loading → use `fs.read_to_string()`.

### Phase 4: Thread VFS into stdio (low risk)

1. Change `Port::stdin/stdout/stderr` to use `fs.stdin/stdout/stderr()`.
2. Remove thread-local current ports in favor of interpreter-scoped ports (optional, can defer).

### Phase 5: MemoryFs writer + test suite (low risk)

1. Implement `MemoryFs::open_write` with a capture wrapper.
2. Add integration tests exercising file I/O against `MemoryFs`.
3. Add tests for `include`, library loading, and `load` against `MemoryFs`.

## Design Decisions

### Why a trait (not generics)?

Using `Arc<dyn FileSystem>` rather than `Interpreter<B, F: FileSystem>` avoids a generic parameter explosion. The VFS is called infrequently (file opens, not per-instruction), so the virtual dispatch overhead is negligible.

### Why in patina-core?

`Port` lives in `patina-core` and needs the trait to change `FileHandle`. Placing the trait in `patina-core` avoids a dependency cycle. The `NativeFs` impl can also live there since it only depends on `std`.

### Why not a third-party crate (e.g., `vfs`, `virtual-fs`)?

Patina's needs are simple — open, read, write, exists, delete. A bespoke trait avoids external dependencies, keeps the API minimal, and allows Scheme-specific affordances (like `read_to_string` as a first-class method for source loading).

### What about directory operations?

R7RS-small does not expose directory operations. The only directory-like operation is library search path traversal, which is already just a sequence of `file_exists` checks on known path patterns. No `readdir` or `mkdir` is needed.

## WASM Considerations

For `wasm32-unknown-unknown`:
- `NativeFs` would be behind `#[cfg(not(target_arch = "wasm32"))]`.
- A `WasmFs` implementation could back onto JS via `wasm-bindgen`, or use `MemoryFs` with pre-loaded library files.
- Stdio would route to JS `console.log` / a text area / etc.
- Library `.sld` and `.scm` files would be bundled into `MemoryFs` at initialization.

For `wasm32-wasi`:
- WASI provides a filesystem API; `NativeFs` may work with minor adjustments, or a thin `WasiFs` wrapper.

## Files to Modify

| File | Change |
|------|--------|
| `crates/patina-core/src/vfs.rs` | **New.** Trait + NativeFs + MemoryFs |
| `crates/patina-core/src/lib.rs` | Export `vfs` module |
| `crates/patina-core/src/port.rs` | `FileHandle` → `Box<dyn VfsRead/Write>`, `open_*` take `&dyn FileSystem` |
| `crates/patina-primitives/src/apply_context.rs` | Add `fn fs()` method |
| `crates/patina-primitives/src/primitives/io/file.rs` | Use `ctx.fs()` instead of `std::fs` |
| `crates/patina-primitives/src/primitives/eval.rs` | `load` uses `ctx.fs().read_to_string()` |
| `crates/patina-primitives/src/primitives/io/ports.rs` | `stdin/stdout/stderr` from VFS |
| `crates/patina-frontend/src/desugarer/mod.rs` | `include` uses VFS |
| `crates/patina-frontend/src/library_support.rs` | `.sld` loading uses VFS |
| `crates/patina-runtime/src/library_registry.rs` | Search paths use VFS |
| `crates/patina-tree-walker/src/eval/mod.rs` | Extras loading uses VFS |
| `crates/patina-interpreter/src/lib.rs` | `Interpreter` stores `Arc<dyn FileSystem>` |
