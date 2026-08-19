//! `lib/rnrs/` re-exports `lib/r6rs/`, and these tests keep the two in step.
//!
//! Each `(rnrs …)` library is a pure re-export of the `(r6rs …)` one, written
//! out name by name because R7RS has no re-export-everything form. That makes
//! the export lists a copy, and a copy drifts: a name added to `lib/r6rs/`
//! would be reachable under its R6RS-port name and silently missing under the
//! R6RS name that real source actually imports. The check is mechanical, so it
//! belongs to a test rather than to whoever remembers.
//!
//! It also pins the load itself. `lib/r6rs/` is full of `cond-expand` branches
//! that fire on `(library (rnrs …))` being present, which is exactly what these
//! shims make true — one of them had no `(r6rs no-rnrs)` escape and left the
//! library inert, failing at the *importer* with an unbound helper rather than
//! anywhere near the cause. Importing every library and calling into it is what
//! catches that class; see `lib/r6rs/PROVENANCE.md`.

mod common;

use patina_frontend::{ExportSpec, LibraryDefinition, Parser};
use std::path::PathBuf;

/// The R6RS libraries bundled under both names, as path fragments.
///
/// Derived from [`EXERCISES`] so the two cannot drift; that table is the one
/// place a library is listed.
fn libraries() -> Vec<&'static str> {
    EXERCISES.iter().map(|(path, _)| *path).collect()
}

/// `"io/simple"` → `"(rnrs io simple)"`.
fn import_name(path: &str) -> String {
    format!("(rnrs {})", path.replace('/', " "))
}

fn lib_dir() -> PathBuf {
    common::repo_root().join("lib")
}

/// The names a bundled `.sld` exports, read with Patina's own reader.
///
/// Going through `Parser` and `LibraryDefinition` rather than scanning the text
/// is not only shorter: a hand-rolled scanner has to get `#|…|#`, `#;`, a `)`
/// inside a string and `#\(` right to answer correctly, and this file would be
/// the second place in the tree that tries. The frontend already answers it,
/// and answering it differently from the loader is the one way this check could
/// pass while the shipped library disagreed.
fn exports_of(tree: &str, library: &str) -> Vec<String> {
    let path = lib_dir().join(tree).join(format!("{library}.sld"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));

    let mut parser =
        Parser::new(&source).unwrap_or_else(|e| panic!("lexing {}: {e:?}", path.display()));
    let datum = parser
        .parse()
        .unwrap_or_else(|e| panic!("parsing {}: {e:?}", path.display()));
    let definition = LibraryDefinition::from_tagged(datum, parser.heap())
        .unwrap_or_else(|e| panic!("reading the library form in {}: {e:?}", path.display()));

    definition
        .exports
        .iter()
        .map(|export| match export {
            ExportSpec::Identifier(name) => name.clone(),
            ExportSpec::Rename { external, .. } => external.clone(),
        })
        .collect()
}

#[test]
fn every_rnrs_shim_exports_exactly_what_its_r6rs_library_does() {
    for library in libraries() {
        let mut upstream = exports_of("r6rs", library);
        let mut shim = exports_of("rnrs", library);
        upstream.sort();
        shim.sort();

        let missing: Vec<_> = upstream.iter().filter(|n| !shim.contains(n)).collect();
        let extra: Vec<_> = shim.iter().filter(|n| !upstream.contains(n)).collect();

        assert!(
            missing.is_empty() && extra.is_empty(),
            "(rnrs {library}) has drifted from (r6rs {library}).\n\
             missing from the shim: {missing:?}\n\
             not in the r6rs library: {extra:?}\n\
             Regenerate the shim's export list from lib/r6rs/{library}.sld."
        );
    }
}

#[test]
fn a_shim_exists_for_every_bundled_r6rs_library() {
    // `unicode-reference/*` is upstream's internal implementation split, not an
    // R6RS library name, and `no-rnrs` is Patina's marker — neither gets a shim.
    let mut bundled: Vec<String> = common::files_under(&lib_dir().join("r6rs"))
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("sld"))
        .map(|p| {
            p.strip_prefix(lib_dir().join("r6rs"))
                .expect("under lib/r6rs")
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .filter(|name| !name.starts_with("unicode-reference/") && name != "no-rnrs")
        .collect();
    bundled.sort();

    let mut expected: Vec<String> = libraries().iter().map(|s| s.to_string()).collect();
    expected.sort();

    assert_eq!(
        bundled, expected,
        "lib/r6rs/ and this test's LIBRARIES list disagree; a library was added \
         or removed without its (rnrs ...) shim"
    );

    for library in libraries() {
        let shim = lib_dir().join("rnrs").join(format!("{library}.sld"));
        assert!(shim.exists(), "missing shim {}", shim.display());
    }
}

/// One call into each library, under its `(rnrs …)` name.
///
/// A bare import proves only that a file parses. These call something, because
/// the failure mode this tree actually has is a `cond-expand` that leaves the
/// library defined-but-empty and fails at the caller.
const EXERCISES: [(&str, &str); 17] = [
    ("base", "(list (div 7 2) (mod 7 2) (inexact 1/2))"),
    (
        "unicode",
        "(list (char-upcase #\\a) (char-general-category #\\a))",
    ),
    (
        "bytevectors",
        "(let ((b (make-bytevector 2 7))) (bytevector-u8-set! b 0 9) (bytevector->u8-list b))",
    ),
    (
        "lists",
        "(list (assp odd? '((2 . a) (3 . b))) (remp even? '(1 2 3)))",
    ),
    ("sorting", "(list-sort < '(3 1 2))"),
    (
        "control",
        "(call-with-values (lambda () (values 1 2)) (lambda (a b) (+ a b)))",
    ),
    (
        "exceptions",
        "(with-exception-handler (lambda (e) 'caught) (lambda () (raise-continuable 'x)))",
    ),
    // Non-fixnum keys on purpose: they are what the inert-cond-expand bug ate.
    (
        "hashtables",
        "(let ((h (make-eqv-hashtable)))
           (hashtable-set! h 2.718 'e)
           (hashtable-set! h 1/2 'half)
           (list (hashtable-ref h 2.718 #f) (hashtable-ref h 1/2 #f)))",
    ),
    ("enums", "(enum-set->list (make-enumeration '(a b c)))"),
    ("io/simple", "(begin (write-char #\\x) 'ok)"),
    ("files", "(file-exists? \"Cargo.toml\")"),
    ("programs", "(list? (command-line))"),
    (
        "arithmetic/fixnums",
        "(list (fx+ 1 2) (fx* 3 4) (fxzero? 0))",
    ),
    ("mutable-pairs", "(let ((p (cons 1 2))) (set-car! p 9) p)"),
    (
        "mutable-strings",
        "(let ((s (string-copy \"abc\"))) (string-set! s 0 #\\z) s)",
    ),
    ("r5rs", "(list (exact->inexact 1) (inexact->exact 1.0))"),
    ("eval", "(eval '(+ 1 2) (environment '(rnrs base)))"),
];

#[test]
fn every_rnrs_library_can_be_called_into() {
    for (path, expr) in EXERCISES {
        let library = import_name(path);
        // `eval_program` runs both backends, panics with both messages if
        // either errors, and asserts the two agree on the value.
        let result = common::eval_program(&format!("(import {library})\n{expr}"));
        assert!(!result.is_empty(), "{library} produced no value");
    }
}
