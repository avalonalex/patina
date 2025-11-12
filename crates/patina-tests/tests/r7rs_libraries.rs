//! Tests for loading all R7RS standard libraries

use patina_tree_walker::Evaluator;

#[test]
fn test_can_load_all_r7rs_libraries() {
    let eval = Evaluator::new();

    // All R7RS libraries that chibi tests require
    let libraries = vec![
        vec!["scheme", "base"],
        vec!["scheme", "char"],
        vec!["scheme", "lazy"],
        vec!["scheme", "inexact"],
        vec!["scheme", "complex"],
        vec!["scheme", "time"],
        vec!["scheme", "file"],
        vec!["scheme", "read"],
        vec!["scheme", "write"],
        vec!["scheme", "eval"],
        vec!["scheme", "process-context"],
        vec!["scheme", "case-lambda"],
        vec!["scheme", "r5rs"],
    ];

    for lib_name in libraries {
        let lib_name_str: Vec<String> = lib_name.iter().map(|s| s.to_string()).collect();
        let lib = eval
            .load_library(&lib_name_str)
            .unwrap_or_else(|e| panic!("Failed to load ({}) library: {}", lib_name.join(" "), e));

        assert_eq!(lib.name, lib_name_str);
        println!("✓ ({}) loaded successfully", lib_name.join(" "));
    }
}
