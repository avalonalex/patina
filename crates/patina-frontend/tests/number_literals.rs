//! #369: grammar, conversions and positions, independently of evaluation.

use num_bigint::BigInt;
use num_rational::BigRational;
use patina_core::{Port, SharedHeap, TaggedValue, new_shared_heap};
use patina_frontend::{Parser, Reader};

fn read(text: &str, heap: &SharedHeap) -> TaggedValue {
    Parser::new_with_heap(text, heap.clone())
        .unwrap()
        .parse()
        .unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

#[test]
fn prefixes_radices_and_separators_use_the_same_grammar_in_every_entry_point() {
    for (text, expected) in [
        ("1_000", 1000),
        ("+1_000", 1000),
        ("-1_000", -1000),
        ("#b1_0", 2),
        ("#o7_0", 56),
        ("#Xf_F", 255),
        ("#e#xA_F", 175),
        ("#x#E-a_f", -175),
        ("#D#e10", 10),
        ("#e1.2_5e+0_2", 125),
        ("#e.5e1", 5),
        ("#e1.e1", 10),
        ("#e1_0/0_2", 5),
        ("#xA_0/1_0", 10),
        ("#e1_0@0_0", 10),
        ("#X1_0@0", 16),
    ] {
        let heap = new_shared_heap();
        let direct = read(text, &heap);
        let string = Parser::number_from_str(text, 10, heap.clone()).unwrap();
        let port = Parser::read_from_port(Port::new_input_string(text.into()), heap.clone())
            .unwrap()
            .unwrap();
        let mut reader = Reader::new(false);
        for ch in text.chars() {
            reader.feed(&ch.to_string());
        }
        reader.no_more_text();
        let streamed = reader.next_datum(&heap, |p| p).unwrap().unwrap();
        for value in [direct, string, port, streamed] {
            assert_eq!(value.as_fixnum(), Some(expected), "{text}");
        }
    }
    // An explicit prefix overrides the argument; hexadecimal letters do not
    // need the synthetic prefix or token classification string->number used.
    for (text, radix, expected) in [
        ("f_f", 16, 255),
        ("#e-ff", 16, -255),
        ("#d10", 2, 10),
        ("#x10", 8, 16),
        ("#E#B10", 16, 2),
    ] {
        assert_eq!(
            Parser::number_from_str(text, radix, new_shared_heap())
                .unwrap()
                .as_fixnum(),
            Some(expected)
        );
    }
}

#[test]
fn errors_name_the_original_character_and_locate_it_in_reader_coordinates() {
    for (text, offset, reason) in [
        ("12abc", 2, "end of number"),
        ("1_000abc", 5, "end of number"),
        ("1__000", 1, "underscore"),
        ("#x#e_ff", 4, "underscore"),
        ("1_.0", 1, "underscore"),
        ("1._0", 2, "underscore"),
        ("1e_2", 2, "underscore"),
        ("#x1_e_f_", 7, "underscore"),
        ("#b102", 4, "end of number"),
        ("#x1.0", 3, "end of number"),
        ("#x#b1", 3, "duplicate radix"),
        ("#e#i1", 3, "duplicate exactness"),
        ("#e#q1", 3, "prefix"),
        ("1/-2", 2, "digit"),
        ("1/+2", 2, "digit"),
        ("1/0", 2, "nonzero"),
        ("1/0+2i", 2, "nonzero"),
        ("1+2/0i", 4, "nonzero"),
        ("1@1/0", 4, "nonzero"),
        ("#e+inf.0", 2, "no exact representation"),
        ("1e+-2", 3, "digit"),
        ("1e2.3", 3, "end of number"),
        ("1i", 1, "requires a sign"),
        ("1+2", 3, "expected i"),
        ("1+2ii", 4, "end of number"),
        ("1@2@3", 3, "end of number"),
        ("1@+i", 4, "inf.0"),
        ("12λ", 2, "end of number"),
        ("1e", 2, "digit"),
        ("1/", 2, "digit"),
        ("#e", 2, "digit"),
    ] {
        let program = format!("λ\r\n  {text}");
        let mut parser = Parser::new(&program).unwrap();
        parser.parse_next().unwrap();
        let error = parser.parse_next().unwrap_err();
        let span = error.span().unwrap();
        assert_eq!(
            (span.start.line, span.start.column, span.start.offset),
            (2, 3 + offset as u32, 5 + offset),
            "{text}: {error}"
        );
        let message = error.to_string();
        assert!(
            message.contains(text) && message.contains(reason),
            "{message}"
        );
        assert!(
            message.contains(&format!("character {}", offset + 1)),
            "{message}"
        );
        let found = text
            .chars()
            .nth(offset)
            .map_or_else(|| "end of number".into(), |c| format!("{c:?}"));
        assert!(message.contains(&format!("found {found}")), "{message}");
        assert!(
            Parser::number_from_str(text, 10, new_shared_heap()).is_none(),
            "{text}"
        );
        // Token replay and port reads must carry the same error position.
        let heap = new_shared_heap();
        let port = Port::new_input_string(program.clone());
        Parser::read_from_port(port.clone(), heap.clone()).unwrap();
        let port_error = Parser::read_from_port(port, heap.clone()).unwrap_err();
        assert_eq!(port_error.span().unwrap().start, span.start, "{text}");
        let mut reader = Reader::new(false);
        for ch in program.chars() {
            reader.feed(&ch.to_string());
        }
        reader.no_more_text();
        reader.next_datum(&heap, |p| p).unwrap().unwrap();
        let replay_error = reader.next_datum(&heap, |p| p).unwrap().unwrap_err();
        assert_eq!(replay_error.span().unwrap().start, span.start, "{text}");
    }
}

#[test]
fn string_conversion_requires_the_entire_string_to_be_a_number() {
    for text in [
        "",
        " 1",
        "1 ",
        "1\n",
        "1 2",
        "1;comment",
        "#|c|#1",
        "#;2 1",
        "#!fold-case 1",
        "+",
        "-",
        "i",
        "+inf",
        "nan",
        "inf",
        "|1|",
        "１",
        "#t",
        "1\0",
    ] {
        assert!(
            Parser::number_from_str(text, 10, new_shared_heap()).is_none(),
            "{text:?}"
        );
    }
    for radix in [0, 1, 3, 36, u32::MAX] {
        assert!(Parser::number_from_str("#d1", radix, new_shared_heap()).is_none());
    }
    for text in ["+inf", "+nan.0tail", "+infinity", "_1000", "+name_1"] {
        let heap = new_shared_heap();
        let value = read(text, &heap);
        assert_eq!(heap.borrow().get_symbol_name(value), Some(text));
    }
}

#[test]
fn exact_decimals_and_integer_limits_never_pass_through_a_float() {
    let heap = new_shared_heap();
    for text in ["#e.1", "#e1.e-1", "#e1_0e-0_2", "#e0.1S+0"] {
        let value = read(text, &heap);
        assert_eq!(
            heap.borrow().get_rational(value),
            Some(&BigRational::new(1.into(), 10.into()))
        );
    }
    let large = read("#e1e400", &heap);
    assert_eq!(
        heap.borrow().get_bigint(large),
        Some(&BigInt::from(10).pow(400))
    );
    for n in [i64::MIN, i64::MAX] {
        let value = read(&n.to_string(), &heap);
        assert_eq!(heap.borrow().get_bigint(value), Some(&BigInt::from(n)));
        let complex = read(&format!("{n}+{n}i").replace("+-", "-"), &heap);
        let h = heap.borrow();
        let (re, im) = h.get_complex(complex).unwrap();
        assert_eq!(h.get_bigint(re), Some(&BigInt::from(n)));
        assert_eq!(h.get_bigint(im), Some(&BigInt::from(n)));
    }
    for text in [
        "#e1e20001",
        "#e1e-20001",
        "#e1e9223372036854775808",
        "#e1e-9223372036854775808",
        "#e1e-9223372036854775807",
        "#e1e99999999999999999999999999",
    ] {
        assert!(
            Parser::number_from_str(text, 10, heap.clone()).is_none(),
            "{text}"
        );
    }
}

#[test]
fn float_conversion_keeps_correct_rounding_extremes_and_negative_zero() {
    let heap = new_shared_heap();
    for text in [
        "0.1",
        "1.00000000000000011102230246251565404236316680908203125",
        "1.00000000000000011102230246251565404236316680908203126",
        "2.2250738585072014e-308",
        "4.9406564584124654e-324",
        "1e-400",
        "-1e-400",
        "1e400",
        "-1e400",
        "-0.0",
        "1e999999999999999999999",
    ] {
        let value = read(text, &heap);
        assert_eq!(
            heap.borrow().get_real(value).unwrap().to_bits(),
            text.parse::<f64>().unwrap().to_bits(),
            "{text}"
        );
    }
    for text in ["#i-0", "#i#x-0", "-0.0@0", "#i-0@0", "#i#x-0@0"] {
        let value = read(text, &heap);
        assert_eq!(
            heap.borrow().get_real(value).unwrap().to_bits(),
            (-0.0f64).to_bits(),
            "{text}"
        );
    }
}
