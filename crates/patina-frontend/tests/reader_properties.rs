//! #366: bounded, reproducible reader differential tests. The same invariant
//! is driven by arbitrary bytes in fuzz/fuzz_targets/reader.rs.

#[path = "support/reader_checks.rs"]
mod reader_checks;

use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use reader_checks::check_reader;

fn config() -> ProptestConfig {
    ProptestConfig {
        rng_seed: RngSeed::Fixed(366),
        max_shrink_iters: 4096,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn fragments() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "(",
            ")",
            "#(",
            "#u8(",
            "[",
            "]",
            ".",
            "'",
            "`",
            ",",
            ",@",
            "#;",
            "#|",
            "|#",
            "#0=",
            "#0#",
            "#1=",
            "#1#",
            "#t",
            "#false",
            "#t#f",
            "1",
            "0",
            "+",
            "-",
            "1/2",
            "#xFF",
            "#e",
            "#i",
            "1_000",
            "1__2",
            "1.5e-2",
            "+i",
            "-inf.0",
            "+nan.0",
            "1+2i",
            "1e-2+3e+4i",
            "1/0+2i",
            "1+2/0i",
            "1@1/0",
            "#\\λ",
            "#\\newline",
            "#\\x10ffff",
            "#\\x110000",
            "#\\",
            "#\\x",
            "\"",
            "|",
            "\\",
            "\\x41;",
            "\\x110000;",
            "\\n",
            "\\\r\n",
            "λ",
            "Straße",
            "ABC",
            "|Straße|",
            "#!fold-case",
            "#!no-fold-case",
            "#!r6rs",
            "#!r7rs",
            "#!unknown",
            "#!/bin/scheme\n",
            ";comment\r\n",
            " ",
            "\t",
            "\r",
            "\n",
            "\r\n",
            "\0",
        ]),
        0..64,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn arbitrary_unicode_agrees(text in prop::collection::vec(any::<char>(), 0..256), chunks in prop::collection::vec(any::<u8>(), 0..32)) {
        check_reader(&text.into_iter().collect::<String>(), &chunks);
    }

    #[test]
    fn syntax_fragments_agree(text in fragments(), chunks in prop::collection::vec(any::<u8>(), 0..32)) {
        check_reader(&text, &chunks);
    }
}

#[test]
fn every_boundary_in_representative_data() {
    for text in [
        "#!fold-case Straße |MiXeD| #!no-fold-case MiXeD",
        "\"a\\x1f642;\\\r\n b\" |a\\x0;b| #\\λ #\\x10ffff",
        "#;#;(ignored) 1 (#|outer #|inner|#|# 2 . 3)",
        "#0=(a . #0#) #(#1=(b) #1#) #(#2# #2=(c))",
        "#u8(0 255) +inf.0 -0.0 +nan.0 1/3 2+3i 1_000",
        "1e-2+3e+4i #e1.2_5e-2-3.5e+2i #x1e-fi #o7@1 #b+inf.0",
        "1e+2 1/0+2i",
        "1@1/0",
        "#e#x1_0 #x#x1",
        "12λ",
        "1 #t#f",
        "2 (",
        "3 \"unterminated",
        "4 #!unknown",
        "5 #99#",
    ] {
        // [split, huge] feeds at every possible character boundary, then
        // one-character chunks exercise trial EOF and buffer compaction.
        for split in 0..text.chars().count() {
            check_reader(text, &[split as u8, u8::MAX]);
        }
        check_reader(text, &[0]);
    }
}

#[test]
fn label_aliases_resolve_to_data_or_raise_read_errors() {
    use patina_core::{Port, new_shared_heap};
    use patina_frontend::Parser;

    // The first input is the six-byte libFuzzer reduction (#565). Chibi and
    // Gauche reject it and the mutually recursive form too.
    for text in ["#0=#0#", "(#0=#1# #1=#0#)", "#(#0=#1=#0#)"] {
        check_reader(text, &[0]);
        let mut parser = Parser::new(text).unwrap();
        let err = parser.parse_next().unwrap_err();
        assert!(err.to_string().contains("without a datum"), "{err}");
        assert!(err.span().is_some());
        assert!(
            Parser::read_from_port(Port::new_input_string(text.into()), new_shared_heap()).is_err()
        );
    }

    // Forward references are an existing Patina extension. Every alias must
    // name the same pair/vector, even when that container is itself cyclic.
    for tail in ["(x)", "#(x)", "(x . #0#)", "#(#0#)"] {
        let text = format!("#(#0=#1# #1=#2# #2={tail})");
        check_reader(&text, &[0, 2, 7]);
        let heap = new_shared_heap();
        let value = Parser::new_with_heap(&text, heap.clone())
            .unwrap()
            .parse()
            .unwrap();
        let h = heap.borrow();
        let first = h.vector_ref(value, 0);
        assert!(first.is_pair() || first.is_vector());
        assert_eq!(first, h.vector_ref(value, 1));
        assert_eq!(first, h.vector_ref(value, 2));
        if tail == "(x . #0#)" {
            assert_eq!(h.cdr(first), first);
        }
        if tail == "#(#0#)" {
            assert_eq!(h.vector_ref(first, 0), first);
        }
    }
}

#[test]
fn long_forward_alias_chain_is_fully_resolved() {
    use patina_core::new_shared_heap;
    use patina_frontend::Parser;
    use std::fmt::Write;

    let mut text = "#(".to_string();
    for label in 0..20_000 {
        write!(text, "#{label}=#{}# ", label + 1).unwrap();
    }
    text.push_str("#20000=(x))");
    let heap = new_shared_heap();
    let value = Parser::new_with_heap(&text, heap.clone())
        .unwrap()
        .parse()
        .unwrap();
    let h = heap.borrow();
    let first = h.vector_ref(value, 0);
    assert!(first.is_pair());
    assert!(h.vector_slice(value).iter().all(|&v| v == first));
}
