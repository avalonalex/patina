//! Tests for number->string and string->number conversion primitives
//!
//! R7RS Reference: Section 6.2.6 - Numerical operations

mod common;
use common::*;

// ============================================================================
// number->string Tests
// ============================================================================

#[test]
fn test_number_to_string_decimal() {
    assert_eval_to("(number->string 100)", "\"100\"");
    assert_eval_to("(number->string 0)", "\"0\"");
    assert_eval_to("(number->string -42)", "\"-42\"");
}

#[test]
fn test_number_to_string_binary() {
    assert_eval_to("(number->string 12 2)", "\"1100\"");
    assert_eval_to("(number->string 255 2)", "\"11111111\"");
    assert_eval_to("(number->string 0 2)", "\"0\"");
}

#[test]
fn test_number_to_string_octal() {
    assert_eval_to("(number->string 64 8)", "\"100\"");
    assert_eval_to("(number->string 255 8)", "\"377\"");
    assert_eval_to("(number->string 8 8)", "\"10\"");
}

#[test]
fn test_number_to_string_hexadecimal() {
    assert_eval_to("(number->string 255 16)", "\"ff\"");
    assert_eval_to("(number->string 256 16)", "\"100\"");
    assert_eval_to("(number->string 4095 16)", "\"fff\"");
}

#[test]
fn test_number_to_string_negative_numbers() {
    assert_eval_to("(number->string -10)", "\"-10\"");
    assert_eval_to("(number->string -255 16)", "\"-ff\"");
    assert_eval_to("(number->string -12 2)", "\"-1100\"");
}

#[test]
fn test_number_to_string_real_numbers() {
    assert_eval_to("(number->string 3.14)", "\"3.14\"");
    assert_eval_to("(number->string 100.0)", "\"100.0\"");
    assert_eval_to("(number->string -2.5)", "\"-2.5\"");
}

#[test]
fn test_number_to_string_special_floats() {
    // Test infinity
    assert_program_eval_to("(number->string (/ 1.0 0.0))", "\"+inf.0\"");
    assert_program_eval_to("(number->string (/ -1.0 0.0))", "\"-inf.0\"");

    // Test NaN
    assert_program_eval_to("(number->string (/ 0.0 0.0))", "\"+nan.0\"");
}

#[test]
fn test_number_to_string_rational() {
    assert_eval_to("(number->string 1/2)", "\"1/2\"");
    assert_eval_to("(number->string 3/4)", "\"3/4\"");
    assert_eval_to("(number->string -2/3)", "\"-2/3\"");
}

#[test]
fn test_number_to_string_rational_with_radix() {
    // Rational numbers with non-decimal radix
    assert_eval_to("(number->string 15/16 16)", "\"f/10\"");
    assert_eval_to("(number->string 7/8 2)", "\"111/1000\"");
}

#[test]
fn test_number_to_string_complex() {
    // Complex numbers now preserve exactness: 3+4i has exact integer parts
    assert_eval_to("(number->string 3+4i)", "\"3+4i\"");
    // Inexact complex: use 3.0+4.0i
    assert_eval_to("(number->string 3.0+4.0i)", "\"3.0+4.0i\"");
    assert_eval_to("(number->string 0+1i)", "\"+i\"");
    assert_eval_to("(number->string 0-1i)", "\"-i\"");
    // 5+0i displays as 5 (exact zero imaginary)
    assert_eval_to("(number->string 5+0i)", "\"5\"");
}

#[test]
fn test_number_to_string_big_integers() {
    assert_eval_to(
        "(number->string 99999999999999999999)",
        "\"99999999999999999999\"",
    );
    assert_eval_to(
        "(number->string 99999999999999999999 16)",
        "\"56bc75e2d630fffff\"",
    );
}

#[test]
fn test_number_to_string_invalid_radix() {
    assert_eval_error("(number->string 100 3)");
    assert_eval_error("(number->string 100 17)");
    assert_eval_error("(number->string 100 0)");
}

#[test]
fn test_number_to_string_real_with_non_decimal_radix() {
    // Real numbers can only be converted with radix 10
    assert_eval_error("(number->string 3.14 16)");
    assert_eval_error("(number->string 100.0 2)");
}

#[test]
fn test_number_to_string_complex_with_non_decimal_radix() {
    // Complex numbers can only be converted with radix 10
    assert_eval_error("(number->string 3+4i 16)");
}

#[test]
fn test_number_to_string_type_error() {
    assert_eval_error("(number->string \"not a number\")");
    assert_eval_error("(number->string #t)");
    assert_eval_error("(number->string '())");
}

#[test]
fn test_number_to_string_arity() {
    assert_eval_error("(number->string)");
    assert_eval_error("(number->string 1 2 3)");
}

// ============================================================================
// string->number Tests
// ============================================================================

#[test]
fn test_string_to_number_decimal() {
    assert_eval_to("(string->number \"100\")", "100");
    assert_eval_to("(string->number \"0\")", "0");
    assert_eval_to("(string->number \"-42\")", "-42");
}

#[test]
fn test_string_to_number_with_radix() {
    assert_eval_to("(string->number \"100\" 16)", "256");
    assert_eval_to("(string->number \"1100\" 2)", "12");
    assert_eval_to("(string->number \"100\" 8)", "64");
}

#[test]
fn test_string_to_number_hexadecimal() {
    assert_eval_to("(string->number \"ff\" 16)", "255");
    assert_eval_to("(string->number \"FF\" 16)", "255");
    assert_eval_to("(string->number \"100\" 16)", "256");
}

#[test]
fn test_string_to_number_binary() {
    assert_eval_to("(string->number \"1100\" 2)", "12");
    assert_eval_to("(string->number \"11111111\" 2)", "255");
}

#[test]
fn test_string_to_number_octal() {
    assert_eval_to("(string->number \"100\" 8)", "64");
    assert_eval_to("(string->number \"377\" 8)", "255");
}

#[test]
fn test_string_to_number_real() {
    assert_eval_to("(string->number \"3.14\")", "3.14");
    assert_eval_to("(string->number \"1e2\")", "100.0");
    assert_eval_to("(string->number \"1E2\")", "100.0");
    assert_eval_to("(string->number \"-2.5\")", "-2.5");
}

#[test]
fn test_string_to_number_scientific_notation() {
    assert_eval_to("(string->number \"1e2\")", "100.0");
    assert_eval_to("(string->number \"1.5e2\")", "150.0");
    assert_eval_to("(string->number \"2e-1\")", "0.2");
}

#[test]
fn test_string_to_number_rational() {
    assert_eval_to("(string->number \"1/2\")", "1/2");
    assert_eval_to("(string->number \"3/4\")", "3/4");
    assert_eval_to("(string->number \"-2/3\")", "-2/3");
}

#[test]
fn test_string_to_number_with_radix_prefix() {
    assert_eval_to("(string->number \"#xff\")", "255");
    assert_eval_to("(string->number \"#xFF\")", "255");
    assert_eval_to("(string->number \"#b1100\")", "12");
    assert_eval_to("(string->number \"#o100\")", "64");
    assert_eval_to("(string->number \"#d100\")", "100");
}

#[test]
fn test_string_to_number_radix_prefix_overrides() {
    // Explicit prefix overrides radix parameter
    assert_eval_to("(string->number \"#xff\" 2)", "255");
    assert_eval_to("(string->number \"#b1100\" 16)", "12");
}

#[test]
fn test_string_to_number_exactness_prefix() {
    assert_eval_to("(string->number \"#e100\")", "100");
    assert_eval_to("(string->number \"#i100\")", "100.0");
    assert_eval_to("(string->number \"#e1.5\")", "3/2");
}

#[test]
fn test_string_to_number_combined_prefixes() {
    assert_eval_to("(string->number \"#x#iff\")", "255.0");
    assert_eval_to("(string->number \"#i#xff\")", "255.0");
    assert_eval_to("(string->number \"#e#x10\")", "16");
}

#[test]
fn test_string_to_number_invalid() {
    assert_eval_to("(string->number \"not a number\")", "#f");
    assert_eval_to("(string->number \"\")", "#f");
    assert_eval_to("(string->number \"abc\")", "#f");
    assert_eval_to("(string->number \"12 34\")", "#f");
    assert_eval_to("(string->number \"1 2\")", "#f");
}

/// Whitespace is not number syntax (R7RS 6.2.7), so a padded string is
/// not a number. Gauche and Chez answer #f; chibi tolerates *leading*
/// whitespace only. This used to assert the trimmed answer.
#[test]
fn test_string_to_number_whitespace() {
    assert_eval_to("(string->number \"  100  \")", "#f");
    assert_eval_to("(string->number \"\\t42\\n\")", "#f");
    assert_eval_to("(string->number \"100\")", "100");
}

#[test]
fn test_string_to_number_big_integers() {
    assert_eval_to(
        "(string->number \"99999999999999999999\")",
        "99999999999999999999",
    );
}

#[test]
fn test_string_to_number_zero() {
    assert_eval_to("(string->number \"0\")", "0");
    assert_eval_to("(string->number \"0.0\")", "0.0");
    assert_eval_to("(string->number \"#b0\")", "0");
}

#[test]
fn test_string_to_number_negative() {
    assert_eval_to("(string->number \"-100\")", "-100");
    assert_eval_to("(string->number \"-3.14\")", "-3.14");
    assert_eval_to("(string->number \"-1/2\")", "-1/2");
}

#[test]
fn test_string_to_number_invalid_radix() {
    assert_eval_error("(string->number \"100\" 3)");
    assert_eval_error("(string->number \"100\" 17)");
}

#[test]
fn test_string_to_number_type_error() {
    assert_eval_error("(string->number 100)");
    assert_eval_error("(string->number #t)");
}

#[test]
fn test_string_to_number_arity() {
    assert_eval_error("(string->number)");
    assert_eval_error("(string->number \"1\" 2 3)");
}

// ============================================================================
// Round-trip Tests (R7RS compliance)
// ============================================================================

#[test]
fn test_round_trip_integer() {
    assert_program_eval_to(
        "(let ((n 12345))
           (equal? n (string->number (number->string n))))",
        "#t",
    );
}

#[test]
fn test_round_trip_with_radix() {
    assert_program_eval_to(
        "(let ((n 255))
           (equal? n (string->number (number->string n 16) 16)))",
        "#t",
    );
}

#[test]
fn test_round_trip_real() {
    assert_program_eval_to(
        "(let ((n 3.14))
           (equal? n (string->number (number->string n))))",
        "#t",
    );
}

#[test]
fn test_round_trip_rational() {
    assert_program_eval_to(
        "(let ((n 3/4))
           (equal? n (string->number (number->string n))))",
        "#t",
    );
}

#[test]
fn test_round_trip_negative() {
    assert_program_eval_to(
        "(let ((n -42))
           (equal? n (string->number (number->string n))))",
        "#t",
    );
}

#[test]
fn test_round_trip_binary() {
    assert_program_eval_to(
        "(let ((n 12))
           (equal? n (string->number (number->string n 2) 2)))",
        "#t",
    );
}

#[test]
fn test_round_trip_hex() {
    assert_program_eval_to(
        "(let ((n 4095))
           (equal? n (string->number (number->string n 16) 16)))",
        "#t",
    );
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_conversion_in_expressions() {
    assert_program_eval_to("(+ (string->number \"10\") (string->number \"20\"))", "30");
}

#[test]
fn test_conversion_with_map() {
    assert_program_eval_to(
        "(map number->string '(1 2 3 4 5))",
        "(\"1\" \"2\" \"3\" \"4\" \"5\")",
    );
}

#[test]
fn test_conversion_with_map_radix() {
    assert_program_eval_to(
        "(define (to-hex n) (number->string n 16))
         (map to-hex '(10 11 12 13 14 15))",
        "(\"a\" \"b\" \"c\" \"d\" \"e\" \"f\")",
    );
}

#[test]
fn test_string_to_number_with_predicates() {
    // Test that string->number results work with predicates
    assert_program_eval_to("(number? (string->number \"123\"))", "#t");
    assert_program_eval_to("(not (number? (string->number \"abc\")))", "#t");
}

#[test]
fn test_mixed_radix_conversion() {
    assert_program_eval_to(
        "(list (number->string 255 2)
               (number->string 255 8)
               (number->string 255 10)
               (number->string 255 16))",
        "(\"11111111\" \"377\" \"255\" \"ff\")",
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_number_to_string_zero_in_all_radices() {
    assert_eval_to("(number->string 0 2)", "\"0\"");
    assert_eval_to("(number->string 0 8)", "\"0\"");
    assert_eval_to("(number->string 0 10)", "\"0\"");
    assert_eval_to("(number->string 0 16)", "\"0\"");
}

#[test]
fn test_string_to_number_case_insensitive() {
    assert_eval_to("(string->number \"#XFF\")", "255");
    assert_eval_to("(string->number \"#xFf\")", "255");
    assert_eval_to("(string->number \"#B1100\")", "12");
}

#[test]
fn test_conversion_preserves_exactness() {
    assert_program_eval_to("(exact? (string->number \"100\"))", "#t");
    assert_program_eval_to("(inexact? (string->number \"100.0\"))", "#t");
}
