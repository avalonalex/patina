//! Tests for the R7RS-large `(scheme ...)` alias libraries.
//!
//! R7RS-large names its libraries `(scheme list)`, `(scheme sort)`, and so on;
//! each is an existing SRFI under its standard-track name. The alias libraries
//! in `lib/scheme/` are pure re-exports of the corresponding `(srfi n)`, so
//! these tests check two things: that each alias loads, and that a binding
//! reached through the alias is the *same* binding as through the SRFI.

use patina_interpreter::TreeWalkInterpreter;
use patina_tree_walker::Evaluator;

/// (alias library, backing SRFI) pairs, per PRD/phase2/R7RS_LARGE_STATUS.md.
const ALIASES: &[(&str, u32)] = &[
    ("list", 1),         // Red
    ("box", 111),        // Red
    ("set", 113),        // Red
    ("comparator", 128), // Red
    ("sort", 132),       // Red
    ("vector", 133),     // Red
    ("generator", 158),  // Tangerine
    ("hash-table", 125), // Red
    ("charset", 14),     // Red
    ("stream", 41),      // Red
];

#[test]
fn test_all_alias_libraries_load() {
    let eval = Evaluator::new();
    for (name, srfi) in ALIASES {
        let lib_name = vec!["scheme".to_string(), name.to_string()];
        let lib = eval
            .load_library(&lib_name)
            .unwrap_or_else(|e| panic!("failed to load (scheme {name}) [SRFI {srfi}]: {e}"));
        assert_eq!(lib.name, lib_name);
    }
}

/// An alias must re-export exactly what its backing SRFI exports -- no more,
/// no fewer. A mismatch means the hand-listed export set has drifted from the
/// source library, which is the failure mode this whole approach invites.
#[test]
fn test_alias_exports_match_backing_srfi() {
    let eval = Evaluator::new();
    for (name, srfi) in ALIASES {
        let alias = eval
            .load_library(&["scheme".to_string(), name.to_string()])
            .unwrap_or_else(|e| panic!("(scheme {name}): {e}"));
        let source = eval
            .load_library(&["srfi".to_string(), srfi.to_string()])
            .unwrap_or_else(|e| panic!("(srfi {srfi}): {e}"));

        let mut a: Vec<_> = alias.exports.keys().cloned().collect();
        let mut s: Vec<_> = source.exports.keys().cloned().collect();
        a.sort();
        s.sort();

        let missing: Vec<_> = s.iter().filter(|k| !a.contains(k)).collect();
        let extra: Vec<_> = a.iter().filter(|k| !s.contains(k)).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "(scheme {name}) drifted from (srfi {srfi}): missing {missing:?}, extra {extra:?}"
        );
    }
}

fn eval_to_string(src: &str) -> String {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let value = interp
        .eval_program(src)
        .unwrap_or_else(|e| panic!("eval failed for {src}: {e:?}"));
    interp.display_tagged(value)
}

/// Headline forms reached through the alias name, not the SRFI name.
#[test]
fn test_alias_bindings_are_usable() {
    let cases = [
        ("(import (scheme list)) (fold + 0 '(1 2 3 4))", "10"),
        (
            "(import (scheme list)) (length (delete-duplicates '(1 2 1 3 2)))",
            "3",
        ),
        ("(import (scheme box)) (unbox (box 42))", "42"),
        (
            "(import (scheme set) (scheme comparator)) \
             (set-size (set (make-default-comparator) 1 2 3 2))",
            "3",
        ),
        (
            "(import (scheme comparator)) (comparator? (make-default-comparator))",
            "#t",
        ),
        ("(import (scheme sort)) (car (list-sort < '(3 1 2)))", "1"),
        (
            "(import (scheme vector)) (vector-count even? #(1 2 3 4))",
            "2",
        ),
        (
            "(import (scheme generator)) (length (generator->list (make-iota-generator 4)))",
            "4",
        ),
    ];
    for (src, expected) in cases {
        assert_eq!(eval_to_string(src), expected, "for: {src}");
    }
}

/// The alias and the SRFI must denote the same binding, not two copies.
#[test]
fn test_alias_and_srfi_share_bindings() {
    assert_eq!(
        eval_to_string("(import (scheme box) (srfi 111)) (unbox (box 7))"),
        "7",
        "importing an alias and its backing SRFI together must not conflict"
    );
}
