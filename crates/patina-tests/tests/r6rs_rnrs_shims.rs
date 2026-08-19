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

use std::path::PathBuf;

/// The R6RS libraries bundled under both names, as path fragments.
const LIBRARIES: [&str; 17] = [
    "base",
    "unicode",
    "bytevectors",
    "lists",
    "sorting",
    "control",
    "exceptions",
    "hashtables",
    "enums",
    "io/simple",
    "files",
    "programs",
    "arithmetic/fixnums",
    "mutable-pairs",
    "mutable-strings",
    "r5rs",
    "eval",
];

fn lib_dir() -> PathBuf {
    common::repo_root().join("lib")
}

/// Strip `;` line comments and `#| |#` blocks.
fn strip_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ';' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if chars[i] == '#' && chars.get(i + 1) == Some(&'|') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '|' && chars[i + 1] == '#') {
                i += 1;
            }
            i = (i + 2).min(chars.len());
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// The names a `.sld` exports: a bare name, or the external half of a
/// `(rename <internal> <external>)`.
fn exported_names(source: &str) -> Vec<String> {
    let text = strip_comments(source);
    let chars: Vec<char> = text.chars().collect();
    let start = text.find("(export").expect("library has an export clause");
    let start = text[..start].chars().count();

    let mut depth = 0usize;
    let mut end = start;
    for (offset, ch) in chars[start..].iter().enumerate() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = start + offset;
                    break;
                }
            }
            _ => {}
        }
    }

    let clause: Vec<char> = chars[start + "(export".chars().count()..end].to_vec();
    let mut names = Vec::new();
    let mut i = 0;
    while i < clause.len() {
        if clause[i].is_whitespace() {
            i += 1;
        } else if clause[i] == '(' {
            let mut depth = 0usize;
            let open = i;
            while i < clause.len() {
                match clause[i] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            let inner: String = clause[open + 1..i].iter().collect();
            let parts: Vec<&str> = inner.split_whitespace().collect();
            assert_eq!(
                parts.first(),
                Some(&"rename"),
                "only `rename` is expected inside an export clause, got {inner:?}"
            );
            assert_eq!(parts.len(), 3, "malformed rename: {inner:?}");
            names.push(parts[2].to_string());
            i += 1;
        } else {
            let open = i;
            while i < clause.len() && !clause[i].is_whitespace() && clause[i] != ')' {
                i += 1;
            }
            names.push(clause[open..i].iter().collect());
        }
    }
    names
}

fn exports_of(tree: &str, library: &str) -> Vec<String> {
    let path = lib_dir().join(tree).join(format!("{library}.sld"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    exported_names(&source)
}

#[test]
fn every_rnrs_shim_exports_exactly_what_its_r6rs_library_does() {
    for library in LIBRARIES {
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

    let mut expected: Vec<String> = LIBRARIES.iter().map(|s| s.to_string()).collect();
    expected.sort();

    assert_eq!(
        bundled, expected,
        "lib/r6rs/ and this test's LIBRARIES list disagree; a library was added \
         or removed without its (rnrs ...) shim"
    );

    for library in LIBRARIES {
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
    ("(rnrs base)", "(list (div 7 2) (mod 7 2) (inexact 1/2))"),
    (
        "(rnrs unicode)",
        "(list (char-upcase #\\a) (char-general-category #\\a))",
    ),
    (
        "(rnrs bytevectors)",
        "(let ((b (make-bytevector 2 7))) (bytevector-u8-set! b 0 9) (bytevector->u8-list b))",
    ),
    (
        "(rnrs lists)",
        "(list (assp odd? '((2 . a) (3 . b))) (remp even? '(1 2 3)))",
    ),
    ("(rnrs sorting)", "(list-sort < '(3 1 2))"),
    (
        "(rnrs control)",
        "(call-with-values (lambda () (values 1 2)) (lambda (a b) (+ a b)))",
    ),
    (
        "(rnrs exceptions)",
        "(with-exception-handler (lambda (e) 'caught) (lambda () (raise-continuable 'x)))",
    ),
    // Non-fixnum keys on purpose: they are what the inert-cond-expand bug ate.
    (
        "(rnrs hashtables)",
        "(let ((h (make-eqv-hashtable)))
           (hashtable-set! h 2.718 'e)
           (hashtable-set! h 1/2 'half)
           (list (hashtable-ref h 2.718 #f) (hashtable-ref h 1/2 #f)))",
    ),
    (
        "(rnrs enums)",
        "(enum-set->list (make-enumeration '(a b c)))",
    ),
    ("(rnrs io simple)", "(begin (write-char #\\x) 'ok)"),
    ("(rnrs files)", "(file-exists? \"Cargo.toml\")"),
    ("(rnrs programs)", "(list? (command-line))"),
    (
        "(rnrs arithmetic fixnums)",
        "(list (fx+ 1 2) (fx* 3 4) (fxzero? 0))",
    ),
    (
        "(rnrs mutable-pairs)",
        "(let ((p (cons 1 2))) (set-car! p 9) p)",
    ),
    (
        "(rnrs mutable-strings)",
        "(let ((s (string-copy \"abc\"))) (string-set! s 0 #\\z) s)",
    ),
    (
        "(rnrs r5rs)",
        "(list (exact->inexact 1) (inexact->exact 1.0))",
    ),
    ("(rnrs eval)", "(eval '(+ 1 2) (environment '(rnrs base)))"),
];

#[test]
fn every_rnrs_library_can_be_called_into() {
    for (library, expr) in EXERCISES {
        let program = format!("(import {library})\n{expr}");
        for (backend, result) in [
            ("tree-walker", common::eval_program_tree_walker(&program)),
            ("vm", common::eval_program_vm(&program)),
        ] {
            assert!(
                !result.is_empty() && !result.contains("rror"),
                "[{backend}] {library} should be callable, got {result:?}"
            );
        }
    }
}
