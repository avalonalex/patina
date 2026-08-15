//! SRFI 130 — cursor-based string library, plus the `(chibi string)` and
//! `(srfi 14)` chain it is written against.
//!
//! Bundled because it was the largest measured gap in the vendored corpus at
//! in-degree 6. Conformance is not this file's job: upstream's own 219-assertion
//! suite runs in `upstream_srfi_suites.rs`, and hand-written tests beside a
//! ported library check only what the porter thought of. What is left for here
//! is what upstream cannot check — that the three-library chain resolves from
//! `lib/` with no search-path help — and the one place the port deviates.

mod common;
use common::eval_program as eval;

fn srfi130(expr: &str) -> String {
    eval(&format!(
        "(import (scheme base) (scheme char) (srfi 130)) {expr}"
    ))
}

/// Importing `(srfi 130)` alone must pull in `(chibi string)` and `(srfi 14)`
/// from the bundled tree. Nothing else here would fail differently if the
/// chain were unresolvable — every test would — but this one says why.
#[test]
fn test_the_bundled_chain_resolves_unaided() {
    assert_eq!(srfi130("(string-null? \"\")"), "#t");
    assert_eq!(
        eval("(import (scheme base) (chibi string)) (string-count \"aab\" #\\a)"),
        "2"
    );
    assert_eq!(
        eval("(import (scheme base) (srfi 14)) (char-set-contains? char-set:digit #\\7)"),
        "#t"
    );
}

/// The reason the SRFI exists: searching returns a *cursor*, not an index. A
/// port that quietly returned indices would pass most string tests and break
/// every caller that then compares or advances the result.
#[test]
fn test_search_returns_cursors() {
    assert_eq!(
        srfi130("(string-cursor->index \"hello\" (string-contains \"hello\" \"ll\"))"),
        "2"
    );
    assert_eq!(srfi130("(string-contains \"hello\" \"zz\")"), "#f");
    assert_eq!(
        srfi130(
            "(string-cursor-diff \"hello\" (string-cursor-start \"hello\") (string-cursor-end \"hello\"))"
        ),
        "5"
    );
    assert_eq!(
        srfi130("(string-cursor? (string-cursor-start \"abc\"))"),
        "#t"
    );
}

/// `string-drop` carries this tree's one local edit — upstream calls
/// `(substring str n)`, which R7RS does not allow. The boundaries are where a
/// wrong end argument would show.
#[test]
fn test_string_drop_boundaries() {
    assert_eq!(srfi130("(string-drop \"hello\" 0)"), "\"hello\"");
    assert_eq!(srfi130("(string-drop \"hello\" 5)"), "\"\"");
    assert_eq!(srfi130("(string-drop \"hello\" 3)"), "\"lo\"");
}

/// Headline forms, one assertion each, as the smoke test the L1 acceptance
/// criterion asks for.
#[test]
fn test_headline_forms() {
    assert_eq!(
        srfi130("(string-join '(\"a\" \"b\" \"c\") \"-\")"),
        "\"a-b-c\""
    );
    assert_eq!(
        srfi130("(string-split \"a,b,c\" \",\")"),
        "(\"a\" \"b\" \"c\")"
    );
    assert_eq!(srfi130("(string-take \"hello\" 3)"), "\"hel\"");
    assert_eq!(srfi130("(string-take-right \"hello\" 2)"), "\"lo\"");
    assert_eq!(srfi130("(string-pad \"7\" 3)"), "\"  7\"");
    assert_eq!(srfi130("(string-reverse \"abc\")"), "\"cba\"");
    assert_eq!(srfi130("(string-prefix? \"hell\" \"hello\")"), "#t");
    assert_eq!(
        srfi130("(string-count \"hello world\" char-alphabetic?)"),
        "10"
    );
}
