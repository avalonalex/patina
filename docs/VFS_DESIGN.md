# Filesystem and VFS

**Status:** Implemented filesystem abstraction and public directory API.
**Updated:** 2026-09-30

The VFS foundation landed in [#138](https://github.com/avalonalex/patina/pull/138).
This document replaces the March 2026 proposal with the current implementation.
The public Scheme import closes [#205](https://github.com/avalonalex/patina/issues/205).

## Scheme API

`(patina filesystem)` ships with Patina and requires no external library path:

```scheme
(import (scheme base) (scheme write) (patina filesystem))
(write (directory-files "."))
(newline)
```

It exports exactly seven procedures. Path arguments are strings, passed to the
interpreter's configured filesystem.

| Procedure | Behavior |
|-----------|----------|
| `(directory-files path)` | Returns entry names as strings, with `"."` and `".."` first, followed by the other names in sorted order. These are names, not full paths. |
| `(create-directory path [mode])` | Creates one directory; missing parents cause an error. Returns `#t` on success. The optional mode is accepted and ignored: the VFS has no permission model. |
| `(delete-directory path)` | Removes an empty directory; returns `#t` on success. |
| `(current-directory)` | Returns the configured filesystem's current directory as a string. |
| `(change-directory path)` | Sets its current directory; returns `#t` on success. |
| `(file-directory? path)` | Returns whether the filesystem reports a directory at the path. |
| `(file-regular? path)` | Returns whether the filesystem reports a file, excluding directories. |

I/O failures from listing, creating, deleting, or getting/changing the current
directory raise catchable error objects. These extensions do not consistently
classify errors with `file-error?`; use `guard` to handle failures. The predicates
return booleans and do not expose the reason a path could not be classified.
`create-directory` does not create parents recursively, and `delete-directory`
does not remove contents.

Use `(scheme file)` for standard file operations such as `open-input-file`,
`file-exists?`, and `delete-file`. The directory procedures are Patina extensions
and are exported under Patina's namespace, consistent with the
[bundling policy](README.md#library-bundling-policy).

The external `(chibi filesystem)` adaptation also uses these underlying
primitives, and adds Scheme traversal helpers and explicit POSIX stubs. Its
[provenance](../test-lib/chibi/PROVENANCE.md) records the different failure
conventions: upstream Chibi returns `#f` for failed mutations and an empty list
for failed directory listing; Patina raises errors.

## Rust integration

[`FileSystem`](../crates/patina-core/src/vfs.rs) is a `Send + Sync` trait in
`patina-core`. Both backends accept an `Arc<dyn FileSystem>`:

- `TreeWalkInterpreter::new_tree_walker_with_fs(fs)`
- `Interpreter::new(VmBackend::with_fs(fs))`

Default constructors use `NativeFs`. The same filesystem reaches the primitive
`ApplyContext`, file ports, `load`, the desugarer's includes, and Scheme library
loading/search. File ports use `ReadPort` and `WritePort` streams; a writer's
`finalize` method completes output when the port closes.

| Implementation | Storage and behavior |
|----------------|----------------------|
| `NativeFs` | Delegates to the host filesystem. Changing its current directory changes the process-wide working directory. |
| `MemoryFs` | Keeps file bytes, explicitly created directories, and a current-directory value in shared memory. Writes are committed by flush/finalize or writer drop. |
| `OverlayFs` | Reads overlay files first, then the base; directory listings merge both. Writes and removals affect the in-memory overlay. It does not hide or delete base entries. |

An overlay over `NativeFs` lets tests read the shipped libraries while keeping
program writes in memory. It is not a security sandbox: base files remain
readable. Paths are interpreted by the selected implementation. In particular,
`MemoryFs` stores the supplied path keys without resolving them against its
current-directory value; `OverlayFs` inherits that behavior on its memory side.
Use explicit paths in tests, rather than assuming native path normalization or
relative-path behavior.

The trait supplies file reads/writes, existence and type checks, canonicalization,
directory listing/creation/removal, and current-directory access. Its directory
listing excludes `.` and `..`; the Scheme primitive adds them. Rust also has
`create_dir_all`, which is not exported by `(patina filesystem)`.

## Boundaries and deferred ideas

The retired proposals mixed the implemented filesystem work with possibilities
that have not shipped. They are not completion claims or a scheduled backlog:

- WASM/browser or WASI integration, JS I/O adapters, and a compiled-in standard
  library remain future integration work.
- A restricted filesystem for sandboxed execution is not implemented.
- Standard streams do not come from `FileSystem`; current ports are managed by
  the I/O/control machinery. CLI script acquisition and workspace discovery also
  still perform host I/O. The VFS does not abstract every process I/O operation.
- POSIX descriptors, stat metadata, links, pipes, and permission controls are
  outside this portable directory API; see the [FFI design](../PRD/FFI_DESIGN.md).
- Async/remote filesystems and snapshot/time-travel facilities were exploratory
  ideas, not implemented parts of this abstraction.

The [earlier design](../PRD/ARCHIVE/vfs_2026_03/FILE_SYSTEM_ABSTRACTION.md)
preserves the historical alternatives. New work should be scoped in GitHub
issues before implementation.

## Verification

- [`patina_filesystem.rs`](../crates/patina-tests/tests/patina_filesystem.rs)
  exercises all seven public operations and catchable errors through an overlay
  on both backends.
- [`cli_options.rs`](../crates/patina-repl/tests/cli_options.rs) verifies the
  public library resolves with isolated library lookup, without `-A` or `-I`.
- [`primitives_reachable_by_import.rs`](../crates/patina-tests/tests/primitives_reachable_by_import.rs)
  requires the directory primitives to be reachable through shipped libraries;
  the internal I/O library is no longer an escape hatch for missing exports.
- [`chibi_filesystem.rs`](../crates/patina-tests/tests/chibi_filesystem.rs)
  covers the external adaptation's helpers and POSIX boundary.
