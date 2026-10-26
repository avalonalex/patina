//! The library loader collects and parses again when one of its reads runs
//! out of file descriptors (#783), as an open does since #607.
//!
//! The filesystem here is the native one, but the first calls of one kind
//! on one named file fail, once or twice: `EMFILE`, which
//! `patina_core::port::out_of_descriptors` recognizes, or
//! `PermissionDenied`, which it does not. Both backends run under
//! `GcMode::Off`, so that only what a test asks for collects and the counts
//! are exact whatever a lane's variables say (#626); the retry's collection
//! runs in every mode. Each library loads through a bare `import`, where the
//! load may collect. The calls are the five a load can run out of
//! descriptors at: the read of the `.sld` file, and the path resolution
//! (`canonicalize`) and the read of an `include` and an
//! `include-library-declarations` file. The subprocess tests in
//! `crates/patina-repl/tests/file_descriptors.rs` run out of real
//! descriptors, under `ulimit -n`.
//!
//! Unix only, as the retry is: elsewhere `out_of_descriptors` tells no
//! failure apart.

#![cfg(unix)]

mod common;

use patina_core::GcMode;
use patina_interpreter::{Backend, Interpreter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Which of the loader's calls fails.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Call {
    Canonicalize,
    Read,
}

/// What the failing call answers.
#[derive(Clone, Copy, Debug)]
enum Failure {
    /// `EMFILE`: 24 on Linux and on macOS.
    OutOfDescriptors,
    PermissionDenied,
}

/// The native filesystem, but the first `times` `call`s on a file named
/// `name` fail with `failure`.
struct FailFirst {
    name: &'static str,
    call: Call,
    failure: Failure,
    left: AtomicUsize,
}

impl FailFirst {
    fn new(name: &'static str, call: Call, failure: Failure, times: usize) -> Arc<Self> {
        Arc::new(Self {
            name,
            call,
            failure,
            left: AtomicUsize::new(times),
        })
    }

    fn check(&self, path: &Path, call: Call) -> std::io::Result<()> {
        if call == self.call
            && path.file_name().is_some_and(|name| name == self.name)
            && self
                .left
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
        {
            return Err(match self.failure {
                Failure::OutOfDescriptors => {
                    let full = std::io::Error::from_raw_os_error(24);
                    assert!(patina_core::port::out_of_descriptors(&full));
                    full
                }
                Failure::PermissionDenied => std::io::ErrorKind::PermissionDenied.into(),
            });
        }
        Ok(())
    }
}

impl patina_core::FileSystem for FailFirst {
    fn open_read(&self, path: &Path) -> std::io::Result<Box<dyn patina_core::vfs::ReadPort>> {
        patina_core::NativeFs.open_read(path)
    }

    fn open_write(&self, path: &Path) -> std::io::Result<Box<dyn patina_core::vfs::WritePort>> {
        patina_core::NativeFs.open_write(path)
    }

    fn file_exists(&self, path: &Path) -> bool {
        patina_core::NativeFs.file_exists(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        patina_core::NativeFs.is_file(path)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        patina_core::NativeFs.remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        self.check(path, Call::Read)?;
        patina_core::NativeFs.read_to_string(path)
    }

    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.check(path, Call::Canonicalize)?;
        patina_core::NativeFs.canonicalize(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        patina_core::NativeFs.is_dir(path)
    }

    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<String>> {
        patina_core::NativeFs.read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        patina_core::NativeFs.create_dir(path)
    }

    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        patina_core::NativeFs.create_dir_all(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        patina_core::NativeFs.remove_dir(path)
    }

    fn current_dir(&self) -> std::io::Result<PathBuf> {
        patina_core::NativeFs.current_dir()
    }

    fn set_current_dir(&self, path: &Path) -> std::io::Result<()> {
        patina_core::NativeFs.set_current_dir(path)
    }
}

/// Both backends name the method alike, but not through `Backend`.
trait SearchPath {
    fn add_search_path(&self, path: PathBuf);
}

impl SearchPath for patina_vm::VmBackend {
    fn add_search_path(&self, path: PathBuf) {
        self.add_library_search_path(path);
    }
}

impl SearchPath for patina_interpreter::TreeWalker {
    fn add_search_path(&self, path: PathBuf) {
        self.add_library_search_path(path);
    }
}

/// `(foo inc)` includes its body, and `(foo decl)` includes its
/// declarations.
const LIBRARIES: [(&str, &str); 4] = [
    (
        "foo/inc.sld",
        "(define-library (foo inc) (import (scheme base)) (export inc-value) (include \"inc-body.scm\"))",
    ),
    ("foo/inc-body.scm", "(define inc-value 42)"),
    (
        "foo/decl.sld",
        "(define-library (foo decl) (import (scheme base)) (include-library-declarations \"decl-decls.scm\"))",
    ),
    (
        "foo/decl-decls.scm",
        "(export decl-value) (begin (define decl-value 43))",
    ),
];

/// `program`'s answer, or its error, on `interp`, with the collections it
/// ran. `(scheme base)` is loaded first, so that the count is the load's.
fn load<B: Backend + SearchPath>(
    interp: Interpreter<B>,
    dir: &Path,
    program: &str,
) -> (Result<String, String>, u64) {
    interp.backend().add_search_path(dir.to_path_buf());
    interp
        .eval_program_owned("(import (scheme base))")
        .expect("(scheme base) loads");
    let collections = || interp.heap().borrow().gc_collections();
    let before = collections();
    let answer = interp
        .eval_program_owned(program)
        .map(|value| interp.display_tagged(&value))
        .map_err(|e| e.to_string());
    (answer, collections() - before)
}

/// `program` on each backend under `GcMode::Off`, each with a filesystem of
/// its own that fails as `fs` says.
fn on_both(
    fs: impl Fn() -> Arc<FailFirst>,
    dir: &Path,
    program: &str,
) -> [(Result<String, String>, u64); 2] {
    let vm = Interpreter::new(patina_vm::VmBackend::with_fs_and_gc_mode(fs(), GcMode::Off));
    let tw = Interpreter::new(patina_interpreter::TreeWalker::with_fs_and_gc_mode(
        fs(),
        GcMode::Off,
    ));
    [load(vm, dir, program), load(tw, dir, program)]
}

/// Each call the loader can run out of descriptors at collects once and
/// parses again, and the library loads, on both backends: one collection,
/// where the same load with nothing failing runs none. A read that fails
/// otherwise raises at once, with no collection.
#[test]
fn each_read_the_loader_makes_collects_and_reads_again() {
    let dir = tempfile::TempDir::new().unwrap();
    for (path, text) in LIBRARIES {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    for (library, value, file, call) in [
        ("(foo inc)", "inc-value", "inc.sld", Call::Read),
        ("(foo inc)", "inc-value", "inc-body.scm", Call::Canonicalize),
        ("(foo inc)", "inc-value", "inc-body.scm", Call::Read),
        (
            "(foo decl)",
            "decl-value",
            "decl-decls.scm",
            Call::Canonicalize,
        ),
        ("(foo decl)", "decl-value", "decl-decls.scm", Call::Read),
    ] {
        let program = format!("(import {library}) {value}");
        let answer = if value == "inc-value" { "42" } else { "43" };
        // Nothing fails: the load's own collections.
        let unfailing = on_both(
            || FailFirst::new("no-such-file", call, Failure::OutOfDescriptors, 1),
            dir.path(),
            &program,
        );
        let out_of_descriptors = on_both(
            || FailFirst::new(file, call, Failure::OutOfDescriptors, 1),
            dir.path(),
            &program,
        );
        let denied = on_both(
            || FailFirst::new(file, call, Failure::PermissionDenied, 1),
            dir.path(),
            &program,
        );
        for (backend, ((plain, plain_n), ((retried, retried_n), (refused, refused_n)))) in
            ["VM", "tree-walker"].into_iter().zip(
                unfailing
                    .into_iter()
                    .zip(out_of_descriptors.into_iter().zip(denied)),
            )
        {
            let case = format!("{backend}: {call:?} of {file}");
            assert_eq!(plain.as_deref(), Ok(answer), "{case}, unfailing");
            assert_eq!(retried.as_deref(), Ok(answer), "{case}, out of descriptors");
            // Under `GcMode::Off` nothing else collects.
            assert_eq!(
                (plain_n, retried_n),
                (0, 1),
                "{case}: one collection for the retry"
            );
            let refused = refused.expect_err(&case);
            assert!(refused.contains("ermission denied"), "{case}: {refused}");
            assert_eq!(refused_n, 0, "{case}: no collection for a refused read");
        }
    }
}

/// One collection, one more parse: a read that runs out of descriptors
/// again after the collection raises the read's error, with that one
/// collection run, on both backends.
#[test]
fn a_second_failure_after_the_collection_raises() {
    let dir = tempfile::TempDir::new().unwrap();
    for (path, text) in LIBRARIES {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    for (file, call, message) in [
        ("inc.sld", Call::Read, "Failed to read"),
        ("inc-body.scm", Call::Read, "Failed to read include file"),
        ("inc-body.scm", Call::Canonicalize, "Failed to resolve path"),
    ] {
        let results = on_both(
            || FailFirst::new(file, call, Failure::OutOfDescriptors, 2),
            dir.path(),
            "(import (foo inc)) inc-value",
        );
        for (backend, (answer, collections)) in ["VM", "tree-walker"].into_iter().zip(results) {
            let case = format!("{backend}: {call:?} of {file}, twice");
            let error = answer.expect_err(&case);
            assert!(
                error.contains(message) && error.contains("os error 24"),
                "{case}: {error}"
            );
            assert_eq!(collections, 1, "{case}");
        }
    }
}
