//! R7RS Section 6.7 - Strings
//!
//! Tests for string operations with comprehensive UTF-8 support
//! Reference: ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

use super::common::*;

// ===== Basic String Operations =====

#[test]
fn test_string_predicate() {
    assert_eval_to("(string? \"hello\")", "#t");
    assert_eval_to("(string? \"\")", "#t");
    assert_eval_to("(string? 'hello)", "#f");
    assert_eval_to("(string? 42)", "#f");
    assert_eval_to("(string? #\\a)", "#f");
}

#[test]
fn test_string_length() {
    assert_eval_to("(string-length \"\")", "0");
    assert_eval_to("(string-length \"hello\")", "5");
    assert_eval_to("(string-length \"hello world\")", "11");
}

#[test]
fn test_string_length_utf8() {
    // UTF-8: Chinese characters
    assert_eval_to("(string-length \"你好\")", "2");
    assert_eval_to("(string-length \"Hello 世界\")", "8");

    // UTF-8: Emoji
    assert_eval_to("(string-length \"Hello 🌍\")", "7");

    // UTF-8: Mixed
    assert_eval_to("(string-length \"Café\")", "4");
}

#[test]
fn test_string_ref() {
    assert_eval_to("(string-ref \"hello\" 0)", "#\\h");
    assert_eval_to("(string-ref \"hello\" 4)", "#\\o");
    assert_eval_to("(string-ref \"world\" 2)", "#\\r");
}

#[test]
fn test_string_ref_utf8() {
    // UTF-8: Chinese
    assert_eval_to("(string-ref \"你好\" 0)", "#\\你");
    assert_eval_to("(string-ref \"你好\" 1)", "#\\好");
    assert_eval_to("(string-ref \"Hello 世界\" 6)", "#\\世");
    assert_eval_to("(string-ref \"Hello 世界\" 7)", "#\\界");

    // UTF-8: Accented characters
    assert_eval_to("(string-ref \"Café\" 3)", "#\\é");
}

#[test]
fn test_string_ref_errors() {
    assert_eval_error("(string-ref \"hello\" -1)");
    assert_eval_error("(string-ref \"hello\" 5)");
    assert_eval_error("(string-ref \"hello\" 100)");
}

// ===== String Construction =====

#[test]
fn test_make_string() {
    assert_eval_to("(make-string 0)", "\"\"");
    assert_eval_to("(make-string 5 #\\a)", "\"aaaaa\"");
    assert_eval_to("(make-string 3 #\\*)", "\"***\"");
    assert_eval_to("(string-length (make-string 10 #\\x))", "10");
}

#[test]
fn test_make_string_utf8() {
    assert_eval_to("(make-string 3 #\\世)", "\"世世世\"");
    assert_eval_to("(string-length (make-string 3 #\\世))", "3");
    assert_eval_to("(make-string 5 #\\い)", "\"いいいいい\"");
}

#[test]
fn test_string_constructor() {
    assert_eval_to("(string)", "\"\"");
    assert_eval_to("(string #\\h)", "\"h\"");
    assert_eval_to("(string #\\h #\\e #\\l #\\l #\\o)", "\"hello\"");
}

#[test]
fn test_string_constructor_utf8() {
    assert_eval_to("(string #\\你 #\\好)", "\"你好\"");
    assert_eval_to("(string #\\C #\\a #\\f #\\é)", "\"Café\"");
    assert_eval_to("(string #\\💡)", "\"💡\"");
}

#[test]
fn test_character_literals_r7rs_named() {
    // R7RS Section 6.6 - Required named characters
    assert_eval_to("(char? #\\alarm)", "#t");
    assert_eval_to("(char? #\\backspace)", "#t");
    assert_eval_to("(char? #\\delete)", "#t");
    assert_eval_to("(char? #\\escape)", "#t");
    assert_eval_to("(char? #\\null)", "#t");
    assert_eval_to("(char? #\\return)", "#t");
    assert_eval_to("(char? #\\space)", "#t");
    assert_eval_to("(char? #\\tab)", "#t");
    assert_eval_to("(char? #\\newline)", "#t");
}

#[test]
fn test_character_literals_hex() {
    // R7RS hex scalar value notation
    assert_eval_to("(char? #\\x03BB)", "#t"); // λ (lambda)
    assert_eval_to("(string #\\x03BB)", "\"λ\"");
    assert_eval_to("(string #\\x03B1)", "\"α\""); // alpha
    assert_eval_to("(string #\\x4E16)", "\"世\""); // CJK character
}

// ===== String Mutation =====

#[test]
fn test_string_set() {
    assert_program_eval_to(
        "(define s (make-string 5 #\\a))
         (string-set! s 2 #\\X)
         s",
        "\"aaXaa\"",
    );

    assert_program_eval_to(
        "(define s \"hello\")
         (string-set! s 0 #\\H)
         s",
        "\"Hello\"",
    );
}

#[test]
fn test_string_set_utf8() {
    assert_program_eval_to(
        "(define s \"Hello World\")
         (string-set! s 6 #\\世)
         s",
        "\"Hello 世orld\"",
    );

    assert_program_eval_to(
        "(define s (make-string 3 #\\a))
         (string-set! s 1 #\\你)
         s",
        "\"a你a\"",
    );
}

#[test]
fn test_string_set_errors() {
    assert_eval_error("(string-set! \"hello\" -1 #\\x)");
    assert_eval_error("(string-set! \"hello\" 5 #\\x)");
    assert_eval_error("(string-set! \"hello\" 0 42)");
}

// ===== String Comparison =====

#[test]
fn test_string_equal() {
    assert_eval_to("(string=? \"hello\" \"hello\")", "#t");
    assert_eval_to("(string=? \"hello\" \"world\")", "#f");
    assert_eval_to("(string=? \"\" \"\")", "#t");
    assert_eval_to("(string=? \"hello\" \"Hello\")", "#f");

    // Variadic
    assert_eval_to("(string=? \"a\" \"a\" \"a\")", "#t");
    assert_eval_to("(string=? \"a\" \"a\" \"b\")", "#f");
}

#[test]
fn test_string_equal_utf8() {
    assert_eval_to("(string=? \"你好\" \"你好\")", "#t");
    assert_eval_to("(string=? \"你好\" \"世界\")", "#f");
    assert_eval_to("(string=? \"Café\" \"Café\")", "#t");
}

#[test]
fn test_string_less() {
    assert_eval_to("(string<? \"a\" \"b\")", "#t");
    assert_eval_to("(string<? \"b\" \"a\")", "#f");
    assert_eval_to("(string<? \"apple\" \"banana\")", "#t");
    assert_eval_to("(string<? \"hello\" \"hello\")", "#f");

    // Variadic
    assert_eval_to("(string<? \"a\" \"b\" \"c\")", "#t");
    assert_eval_to("(string<? \"a\" \"c\" \"b\")", "#f");
}

#[test]
fn test_string_greater() {
    assert_eval_to("(string>? \"b\" \"a\")", "#t");
    assert_eval_to("(string>? \"a\" \"b\")", "#f");
    assert_eval_to("(string>? \"banana\" \"apple\")", "#t");
}

#[test]
fn test_string_less_equal() {
    assert_eval_to("(string<=? \"a\" \"b\")", "#t");
    assert_eval_to("(string<=? \"a\" \"a\")", "#t");
    assert_eval_to("(string<=? \"b\" \"a\")", "#f");
}

#[test]
fn test_string_greater_equal() {
    assert_eval_to("(string>=? \"b\" \"a\")", "#t");
    assert_eval_to("(string>=? \"a\" \"a\")", "#t");
    assert_eval_to("(string>=? \"a\" \"b\")", "#f");
}

// ===== Case-Insensitive Comparison =====
// Note: string-ci* functions are in (scheme char), not (scheme base)

#[test]
fn test_string_ci_equal() {
    assert_eval_with_scheme_char("(string-ci=? \"Hello\" \"hello\")", "#t");
    assert_eval_with_scheme_char("(string-ci=? \"HELLO\" \"hello\")", "#t");
    assert_eval_with_scheme_char("(string-ci=? \"hello\" \"world\")", "#f");

    // Variadic
    assert_eval_with_scheme_char("(string-ci=? \"ABC\" \"abc\" \"AbC\")", "#t");
}

#[test]
fn test_string_ci_less() {
    assert_eval_with_scheme_char("(string-ci<? \"abc\" \"ABC\")", "#f");
    assert_eval_with_scheme_char("(string-ci<? \"abc\" \"BCD\")", "#t");
    assert_eval_with_scheme_char("(string-ci<? \"Apple\" \"BANANA\")", "#t");
}

#[test]
fn test_string_ci_greater() {
    assert_eval_with_scheme_char("(string-ci>? \"BCD\" \"abc\")", "#t");
    assert_eval_with_scheme_char("(string-ci>? \"abc\" \"ABC\")", "#f");
}

#[test]
fn test_string_ci_less_equal() {
    assert_eval_with_scheme_char("(string-ci<=? \"abc\" \"ABC\")", "#t");
    assert_eval_with_scheme_char("(string-ci<=? \"abc\" \"BCD\")", "#t");
}

#[test]
fn test_string_ci_greater_equal() {
    assert_eval_with_scheme_char("(string-ci>=? \"ABC\" \"abc\")", "#t");
    assert_eval_with_scheme_char("(string-ci>=? \"BCD\" \"abc\")", "#t");
}

// ===== String Manipulation =====

#[test]
fn test_string_append() {
    assert_eval_to("(string-append)", "\"\"");
    assert_eval_to("(string-append \"hello\")", "\"hello\"");
    assert_eval_to(
        "(string-append \"hello\" \" \" \"world\")",
        "\"hello world\"",
    );
    assert_eval_to("(string-append \"a\" \"b\" \"c\" \"d\")", "\"abcd\"");
}

#[test]
fn test_string_append_utf8() {
    assert_eval_to("(string-append \"你\" \"好\")", "\"你好\"");
    assert_eval_to("(string-append \"Hello \" \"世\" \"界\")", "\"Hello 世界\"");
}

#[test]
fn test_substring() {
    assert_eval_to("(substring \"hello\" 0 5)", "\"hello\"");
    assert_eval_to("(substring \"hello\" 1 4)", "\"ell\"");
    assert_eval_to("(substring \"hello world\" 0 5)", "\"hello\"");
    assert_eval_to("(substring \"hello world\" 6 11)", "\"world\"");
    assert_eval_to("(substring \"hello\" 0 0)", "\"\"");
}

#[test]
fn test_substring_utf8() {
    assert_eval_to("(substring \"你好世界\" 0 2)", "\"你好\"");
    assert_eval_to("(substring \"你好世界\" 2 4)", "\"世界\"");
    assert_eval_to("(substring \"Hello 世界\" 0 5)", "\"Hello\"");
    assert_eval_to("(substring \"Hello 世界\" 6 8)", "\"世界\"");
}

#[test]
fn test_substring_errors() {
    assert_eval_error("(substring \"hello\" -1 3)");
    assert_eval_error("(substring \"hello\" 0 6)");
    assert_eval_error("(substring \"hello\" 3 2)");
}

// ===== String Conversions =====

#[test]
fn test_string_to_list() {
    assert_eval_to("(string->list \"\")", "()");
    assert_eval_to("(string->list \"abc\")", "(#\\a #\\b #\\c)");
    assert_eval_to("(string->list \"hello\")", "(#\\h #\\e #\\l #\\l #\\o)");
}

#[test]
fn test_string_to_list_utf8() {
    assert_eval_to("(string->list \"你好\")", "(#\\你 #\\好)");
    assert_eval_to("(string->list \"Café\")", "(#\\C #\\a #\\f #\\é)");
}

#[test]
fn test_string_to_list_range() {
    assert_eval_to("(string->list \"hello\" 1)", "(#\\e #\\l #\\l #\\o)");
    assert_eval_to("(string->list \"hello\" 1 4)", "(#\\e #\\l #\\l)");
    assert_eval_to("(string->list \"hello\" 0 0)", "()");
}

#[test]
fn test_list_to_string() {
    assert_eval_to("(list->string '())", "\"\"");
    assert_eval_to("(list->string '(#\\a #\\b #\\c))", "\"abc\"");
    assert_eval_to("(list->string '(#\\h #\\e #\\l #\\l #\\o))", "\"hello\"");
}

#[test]
fn test_list_to_string_utf8() {
    assert_eval_to("(list->string '(#\\你 #\\好))", "\"你好\"");
    assert_eval_to("(list->string '(#\\C #\\a #\\f #\\é))", "\"Café\"");
    assert_eval_to("(list->string '(#\\世 #\\界))", "\"世界\"");
}

#[test]
fn test_string_list_roundtrip() {
    assert_eval_to("(list->string (string->list \"hello\"))", "\"hello\"");
    assert_eval_to("(list->string (string->list \"你好\"))", "\"你好\"");
    assert_eval_to("(string->list (list->string '(#\\a #\\b)))", "(#\\a #\\b)");
}

#[test]
fn test_string_copy() {
    assert_eval_to("(string-copy \"hello\")", "\"hello\"");
    assert_eval_to("(string-copy \"\")", "\"\"");

    // Verify it's a copy (different identity)
    assert_program_eval_to(
        "(define s1 \"hello\")
         (define s2 (string-copy s1))
         (string-set! s2 0 #\\H)
         s1",
        "\"hello\"",
    );
}

#[test]
fn test_string_copy_range() {
    assert_eval_to("(string-copy \"hello\" 1)", "\"ello\"");
    assert_eval_to("(string-copy \"hello\" 1 4)", "\"ell\"");
    assert_eval_to("(string-copy \"hello\" 0 5)", "\"hello\"");
}

#[test]
fn test_string_copy_utf8() {
    assert_eval_to("(string-copy \"你好世界\" 1 3)", "\"好世\"");
}

// ===== Integration Tests =====

#[test]
fn test_string_operations_combined() {
    // Build a string from parts
    assert_program_eval_to(
        "(define greeting (string-append \"Hello\" \" \" \"World\"))
         (string-length greeting)",
        "11",
    );

    // Extract and modify
    assert_program_eval_to(
        "(define s (substring \"hello world\" 0 5))
         (string-set! s 0 #\\H)
         s",
        "\"Hello\"",
    );
}

#[test]
fn test_string_mutation_independence() {
    // Strings should be independent after copy
    assert_program_eval_to(
        "(define s1 (make-string 3 #\\a))
         (define s2 (string-copy s1))
         (string-set! s1 0 #\\X)
         (string-set! s2 0 #\\Y)
         (string-append s1 \" \" s2)",
        "\"Xaa Yaa\"",
    );
}

#[test]
fn test_string_comparison_ordering() {
    // Lexicographic ordering
    assert_eval_to("(string<? \"\" \"a\")", "#t");
    assert_eval_to("(string<? \"a\" \"aa\")", "#t");
    assert_eval_to("(string<? \"apple\" \"application\")", "#t");

    // Case sensitivity
    assert_eval_to("(string<? \"A\" \"a\")", "#t"); // ASCII: uppercase < lowercase
    assert_eval_with_scheme_char("(string-ci<? \"A\" \"a\")", "#f"); // Case-insensitive: equal (requires scheme char)
}

#[test]
fn test_empty_string_edge_cases() {
    assert_eval_to("(string-length \"\")", "0");
    assert_eval_to("(string->list \"\")", "()");
    assert_eval_to("(list->string '())", "\"\"");
    assert_eval_to("(string-append \"\")", "\"\"");
    assert_eval_to("(string-append \"\" \"\" \"\")", "\"\"");
    assert_eval_to("(substring \"hello\" 2 2)", "\"\"");
    assert_eval_to("(string-copy \"\")", "\"\"");
}

#[test]
fn test_utf8_mixed_content() {
    // Mixed ASCII and Unicode
    assert_eval_to("(string-length \"Hello 世界 World\")", "14");

    assert_program_eval_to(
        "(define s \"Café 咖啡\")
         (string-ref s 3)",
        "#\\é",
    );

    assert_program_eval_to(
        "(define s \"Café 咖啡\")
         (string-ref s 5)",
        "#\\咖",
    );
}
