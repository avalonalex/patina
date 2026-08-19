//! `(chibi filesystem)`'s portable half, and the boundary where it stops.
//!
//! Upstream's `cond-expand` has branches for chibi, chicken and sagittarius and
//! no `else`, so before this the library loaded defining nothing and every
//! importer failed on its first export. The `(patina …)` branch implements the
//! directory API and stubs the POSIX layer; see `lib/chibi/PROVENANCE.md`.
//!
//! The directory tests run against an `OverlayFs`, not the real filesystem —
//! that is the point of routing the primitives through the VFS trait, and a
//! test that touched real directories would prove the opposite.

mod common;
use common::*;

fn overlay() -> std::sync::Arc<patina_core::OverlayFs> {
    std::sync::Arc::new(patina_core::OverlayFs::new(std::sync::Arc::new(
        patina_core::NativeFs,
    )))
}

fn eval_on(fs: &std::sync::Arc<patina_core::OverlayFs>, code: &str) -> String {
    let interp = patina_interpreter::TreeWalkInterpreter::new_tree_walker_with_fs(fs.clone());
    let result = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("failed to evaluate:\n{code}\n{e}"));
    interp.display_tagged(result)
}

const IMPORT: &str = "(import (scheme base) (chibi filesystem))";

/// The regression that motivated the branch: importing the library at all.
#[test]
fn test_library_loads_and_defines_its_exports() {
    assert_program_eval_to(
        "(import (scheme base) (chibi filesystem)) (procedure? directory-files)",
        "#t",
    );
}

#[test]
fn test_directory_operations_route_through_the_vfs() {
    let fs = overlay();
    // Nothing here touches the real filesystem: //proj exists only in the overlay.
    assert_eq!(
        eval_on(
            &fs,
            &format!(
                "{IMPORT}
                 (create-directory \"/proj\")
                 (list (file-directory? \"/proj\") (file-directory? \"/absent\"))"
            )
        ),
        "(#t #f)"
    );
}

#[test]
fn test_directory_files_includes_dot_and_dotdot() {
    // chibi's callers filter these by name, so omitting them would silently
    // change every traversal written against the library.
    let fs = overlay();
    fs.overlay()
        .add_text_file(std::path::PathBuf::from("/d/a.txt"), "a");
    let listing = eval_on(
        &fs,
        &format!("{IMPORT} (create-directory \"/d\") (directory-files \"/d\")"),
    );
    assert!(listing.contains('.'), "expected . and .. in {listing}");
    assert!(listing.contains("a.txt"), "expected the file in {listing}");
}

#[test]
fn test_directory_fold_skips_dot_entries() {
    let fs = overlay();
    fs.overlay()
        .add_text_file(std::path::PathBuf::from("/d/a.txt"), "a");
    fs.overlay()
        .add_text_file(std::path::PathBuf::from("/d/b.txt"), "b");
    assert_eq!(
        eval_on(
            &fs,
            &format!(
                "{IMPORT}
                 (create-directory \"/d\")
                 (directory-fold \"/d\" (lambda (f acc) (+ acc 1)) 0)"
            )
        ),
        "2"
    );
}

#[test]
fn test_create_directory_star_makes_parents() {
    let fs = overlay();
    assert_eq!(
        eval_on(
            &fs,
            &format!(
                "{IMPORT}
                 (create-directory* \"/a/b/c\")
                 (list (file-directory? \"/a\") (file-directory? \"/a/b/c\"))"
            )
        ),
        "(#t #t)"
    );
}

#[test]
fn test_with_directory_restores_the_previous_directory() {
    let fs = overlay();
    assert_eq!(
        eval_on(
            &fs,
            &format!(
                "{IMPORT}
                 (create-directory \"/tmpdir\")
                 (let ((before (current-directory)))
                   (let ((inside (with-directory \"/tmpdir\" current-directory)))
                     (list inside (equal? before (current-directory)))))"
            )
        ),
        "(\"/tmpdir\" #t)"
    );
}

/// The other half of the contract: the POSIX layer says so rather than
/// pretending. The marker is what `patina-compat` classifies on, so the exact
/// wording is load-bearing — see `crates/patina-compat/src/run.rs`.
#[test]
fn test_posix_layer_reports_that_it_needs_ffi() {
    for proc in [
        "(duplicate-file-descriptor 1)",
        "(open \"/x\" 0)",
        "(file-owner \"/x\")",
        "(symbolic-link-file \"/a\" \"/b\")",
        "(chmod \"/x\" 0)",
    ] {
        let out = eval_program(&format!(
            "{IMPORT} (guard (e (#t (error-object-message e))) {proc})"
        ));
        assert!(
            out.contains("requires FFI"),
            "{proc} should report needing FFI, got: {out}"
        );
    }
}

/// `open/…` flags exist so callers can combine them, but they are placeholders:
/// the failure must arrive at `open`, not at the `bitwise-ior` before it.
#[test]
fn test_open_flags_are_combinable_and_fail_at_open() {
    let out = eval_program(
        "(import (scheme base) (srfi 151) (chibi filesystem))
         (guard (e (#t (error-object-message e)))
           (open \"/x\" (bitwise-ior open/write open/create open/exclusive)))",
    );
    assert!(out.contains("requires FFI"), "got: {out}");
}

// -- delete-file-hierarchy -----------------------------------------------------
//
// Nothing below can reach a real file: `OverlayFs` routes every removal to the
// in-memory overlay and never to its base, so even a regressed guard deletes
// only overlay entries. That is what makes it safe to point the walk at "/".

/// Upstream refuses `""` and `"/"` *before* touching anything; the hand-written
/// branch used to start deleting immediately, so a computed-empty path was all
/// it took to begin a depth-first walk of the root.
#[test]
fn test_delete_file_hierarchy_refuses_unsafe_directories() {
    for dir in ["", "/"] {
        let fs = overlay();
        fs.overlay()
            .add_text_file(std::path::PathBuf::from("/keep.txt"), "keep");
        let out = eval_on(
            &fs,
            &format!(
                "{IMPORT}
                 (guard (e (#t (error-object-message e)))
                   (delete-file-hierarchy \"{dir}\"))"
            ),
        );
        assert!(
            out.contains("won't delete unsafe directory"),
            "{dir:?} should be refused, got: {out}"
        );
        // The refusal has to precede the walk, not merely survive it.
        assert!(
            fs.overlay()
                .get_text_file(std::path::Path::new("/keep.txt"))
                .is_some(),
            "{dir:?} deleted overlay files before refusing"
        );
    }
}

#[test]
fn test_delete_file_hierarchy_removes_a_tree() {
    let fs = overlay();
    assert_eq!(
        eval_on(
            &fs,
            "(import (scheme base) (scheme file) (chibi filesystem))
             (create-directory \"/tree\")
             (create-directory \"/tree/sub\")
             (call-with-output-file \"/tree/sub/a.txt\" (lambda (p) (write-string \"a\" p)))
             (delete-file-hierarchy \"/tree\")
             (list (file-directory? \"/tree\") (file-exists? \"/tree/sub/a.txt\"))"
        ),
        "(#f #f)"
    );
}

/// `(delete-file-hierarchy dir [ignore-errors?])` — the optional argument was
/// accepted and never read, so a caller asking for tolerance got an exception.
#[test]
fn test_delete_file_hierarchy_honours_ignore_errors() {
    // Every entry below exists only in the base, and removals only reach the
    // overlay — so each one fails, which is exactly the case under test.
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/examples/arithmetic"
    );

    let fs = overlay();
    assert_eq!(
        eval_on(
            &fs,
            &format!(
                "{IMPORT}
                 (guard (e (#t 'raised)) (delete-file-hierarchy \"{dir}\"))"
            )
        ),
        "raised"
    );

    let fs = overlay();
    assert_eq!(
        eval_on(
            &fs,
            &format!("{IMPORT} (delete-file-hierarchy \"{dir}\" #t)")
        ),
        "#t"
    );

    assert!(
        std::path::Path::new(dir).join("basic.scm").exists(),
        "the base filesystem must be untouched"
    );
}
