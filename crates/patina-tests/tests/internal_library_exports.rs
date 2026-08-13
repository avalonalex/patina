//! Pin the deletion of the continuation-broken higher-order primitives.
//!
//! `(scheme base)`'s `string-map`/`string-for-each`/`vector-map`/
//! `vector-for-each` are Scheme (`lib/scheme/base/higher_order.scm`) because
//! a Rust higher-order frame drops captured continuations. The Rust versions
//! were deleted rather than left as a second, broken implementation — this
//! asserts the internal libraries no longer hand them out (and that
//! `(scheme base)`'s Scheme definitions therefore no longer shadow an
//! imported binding, R7RS 5.6.1). Without this pin, a re-added registration
//! would be caught by nothing.

use patina_interpreter::TreeWalkInterpreter;

#[test]
fn internal_libraries_do_not_export_the_higher_order_names() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let evaluator = interp.backend().evaluator();

    let cases: &[(&[&str], &[&str], &str)] = &[
        (
            &["patina", "internal", "strings"],
            &["string-map", "string-for-each"],
            "string-ref",
        ),
        (
            &["patina", "internal", "vectors"],
            &["vector-map", "vector-for-each"],
            "vector-ref",
        ),
    ];
    for (name, gone, still_there) in cases {
        let name: Vec<String> = name.iter().map(|s| s.to_string()).collect();
        let lib = evaluator
            .load_library(&name)
            .unwrap_or_else(|e| panic!("({}) should load: {e:?}", name.join(" ")));
        for export in *gone {
            assert!(
                !lib.exports_identifier(export),
                "({}) exports {export} again — the live implementation is \
                 lib/scheme/base/higher_order.scm, and the Rust one was \
                 deleted for dropping captured continuations",
                name.join(" ")
            );
        }
        // Guard the guard: an empty or misloaded library would pass the
        // negative checks vacuously.
        assert!(
            lib.exports_identifier(still_there),
            "({}) should still export {still_there}",
            name.join(" ")
        );
    }
}
