//! Character primitive operations (R7RS Section 6.6)
//!
//! Implements character operations including:
//! - Character comparisons (char=?, char<?, etc.)
//! - Character type predicates (char-alphabetic?, char-numeric?, etc.)
//! - Character case conversion (char-upcase, char-downcase, char-foldcase)
//! - Character/integer conversion (char->integer, integer->char)
//! - Digit value extraction (digit-value)
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// ========== TaggedValue Extraction Helpers ==========

/// Extract a character from a TaggedValue
fn get_char(
    tv: TaggedValue,
    _heap: &std::cell::Ref<'_, patina_core::Heap>,
    fn_name: &str,
) -> Result<char, EvalError> {
    if tv.is_char() {
        return Ok(tv.as_char_unchecked());
    }
    Err(EvalError::TypeError(format!(
        "{}: requires character",
        fn_name
    )))
}

/// Extract an integer from a TaggedValue
fn get_integer(
    tv: TaggedValue,
    _heap: &std::cell::Ref<'_, patina_core::Heap>,
    fn_name: &str,
) -> Result<i64, EvalError> {
    if tv.is_fixnum() {
        return Ok(tv.as_fixnum_unchecked());
    }
    Err(EvalError::TypeError(format!(
        "{}: requires exact integer",
        fn_name
    )))
}

// ========== Character Comparisons ==========

/// (char=? char1 char2 ...) - Character equality
pub(super) fn char_equal(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char=?")?;
        let c2 = get_char(args[i + 1], &heap_ref, "char=?")?;
        if c1 != c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char<? char1 char2 ...) - Character less than
pub(super) fn char_lt(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char<?")?;
        let c2 = get_char(args[i + 1], &heap_ref, "char<?")?;
        if c1 >= c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char>? char1 char2 ...) - Character greater than
pub(super) fn char_gt(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char>?")?;
        let c2 = get_char(args[i + 1], &heap_ref, "char>?")?;
        if c1 <= c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char<=? char1 char2 ...) - Character less than or equal
pub(super) fn char_le(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char<=?")?;
        let c2 = get_char(args[i + 1], &heap_ref, "char<=?")?;
        if c1 > c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char>=? char1 char2 ...) - Character greater than or equal
pub(super) fn char_ge(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char>=?")?;
        let c2 = get_char(args[i + 1], &heap_ref, "char>=?")?;
        if c1 < c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

// ========== Case-Insensitive Character Comparisons ==========

// ========== Simple case mapping ==========
//
// R7RS 6.6 defines char-upcase / char-downcase / char-foldcase as the Unicode
// *simple* case mappings (UnicodeData's one-to-one fields), and char-ci=? and
// friends as comparison "as if by char-foldcase". Rust's std gives the *full*
// mappings (SpecialCasing: ß → "SS", ﬁ → "FI"); the two agree wherever the
// full mapping is a single character, and differ on about a hundred
// characters. Taking the first character of an expansion (`#\S` for ß) is a
// different letter, and returning the character itself misses the cases
// that *do* have a simple mapping (İ → i, ᾀ → ᾈ) — so the table below
// supplies the simple mapping exactly where the full one expands.

// Generated from UnicodeData.txt / SpecialCasing.txt (Unicode 17.0, SpecialCasing dated 2025-07-31):
// characters whose full case mapping is not a single character, with the
// simple (UnicodeData field 12/13) mapping to use instead. Identity means
// there is no simple mapping either (ß has no upper-case character).
const SIMPLE_UPPER_WHEN_FULL_EXPANDS: &[(char, char)] = &[
    ('\u{1F80}', '\u{1F88}'),
    ('\u{1F81}', '\u{1F89}'),
    ('\u{1F82}', '\u{1F8A}'),
    ('\u{1F83}', '\u{1F8B}'),
    ('\u{1F84}', '\u{1F8C}'),
    ('\u{1F85}', '\u{1F8D}'),
    ('\u{1F86}', '\u{1F8E}'),
    ('\u{1F87}', '\u{1F8F}'),
    ('\u{1F90}', '\u{1F98}'),
    ('\u{1F91}', '\u{1F99}'),
    ('\u{1F92}', '\u{1F9A}'),
    ('\u{1F93}', '\u{1F9B}'),
    ('\u{1F94}', '\u{1F9C}'),
    ('\u{1F95}', '\u{1F9D}'),
    ('\u{1F96}', '\u{1F9E}'),
    ('\u{1F97}', '\u{1F9F}'),
    ('\u{1FA0}', '\u{1FA8}'),
    ('\u{1FA1}', '\u{1FA9}'),
    ('\u{1FA2}', '\u{1FAA}'),
    ('\u{1FA3}', '\u{1FAB}'),
    ('\u{1FA4}', '\u{1FAC}'),
    ('\u{1FA5}', '\u{1FAD}'),
    ('\u{1FA6}', '\u{1FAE}'),
    ('\u{1FA7}', '\u{1FAF}'),
    ('\u{1FB3}', '\u{1FBC}'),
    ('\u{1FC3}', '\u{1FCC}'),
    ('\u{1FF3}', '\u{1FFC}'),
];

const SIMPLE_LOWER_WHEN_FULL_EXPANDS: &[(char, char)] = &[('\u{0130}', '\u{0069}')];

/// `c`'s simple case mapping, given std's full one: the full mapping when it
/// is a single character, the tabled simple mapping when it expands, else the
/// character itself (ß has no upper-case character at all).
fn simple_mapping(
    c: char,
    full: impl Iterator<Item = char>,
    when_full_expands: &[(char, char)],
) -> char {
    let mut it = full;
    match (it.next(), it.next()) {
        (Some(m), None) => m,
        _ => when_full_expands
            .iter()
            .find(|(from, _)| *from == c)
            .map_or(c, |(_, to)| *to),
    }
}

/// Unicode simple case folding — one character to one character, which is
/// what `char-foldcase` returns and what `char-ci=?` compares. Lower-casing
/// is not the same thing: final sigma lower-cases to itself but folds to
/// sigma, so `(char-ci=? #\ς #\σ)` came out false.
fn fold_char(c: char) -> char {
    use unicode_casefold::{Locale, UnicodeCaseFold, Variant};
    c.case_fold_with(Variant::Simple, Locale::NonTurkic)
        .next()
        .unwrap_or(c)
}

/// (char-ci=? char1 char2 ...) - Case-insensitive character equality
pub(super) fn char_ci_equal(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = fold_char(get_char(args[i], &heap_ref, "char-ci=?")?);
        let c2 = fold_char(get_char(args[i + 1], &heap_ref, "char-ci=?")?);
        if c1 != c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char-ci<? char1 char2 ...) - Case-insensitive character less than
pub(super) fn char_ci_lt(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = fold_char(get_char(args[i], &heap_ref, "char-ci<?")?);
        let c2 = fold_char(get_char(args[i + 1], &heap_ref, "char-ci<?")?);
        if c1 >= c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char-ci>? char1 char2 ...) - Case-insensitive character greater than
pub(super) fn char_ci_gt(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = fold_char(get_char(args[i], &heap_ref, "char-ci>?")?);
        let c2 = fold_char(get_char(args[i + 1], &heap_ref, "char-ci>?")?);
        if c1 <= c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char-ci<=? char1 char2 ...) - Case-insensitive character less than or equal
pub(super) fn char_ci_le(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = fold_char(get_char(args[i], &heap_ref, "char-ci<=?")?);
        let c2 = fold_char(get_char(args[i + 1], &heap_ref, "char-ci<=?")?);
        if c1 > c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char-ci>=? char1 char2 ...) - Case-insensitive character greater than or equal
pub(super) fn char_ci_ge(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = fold_char(get_char(args[i], &heap_ref, "char-ci>=?")?);
        let c2 = fold_char(get_char(args[i + 1], &heap_ref, "char-ci>=?")?);
        if c1 < c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

// ========== Character Type Predicates ==========

/// (char-alphabetic? char) - Returns #t if char is alphabetic
pub(super) fn char_alphabetic_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-alphabetic?")?;
    Ok(TaggedValue::boolean(c.is_alphabetic()))
}

/// (char-numeric? char) - Returns #t for Unicode decimal digits
pub(super) fn char_numeric_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-numeric?")?;
    // R7RS requires Numeric_Type=Decimal. Rust's is_numeric() also accepts
    // other numbers, such as superscripts, fractions and Roman numerals.
    Ok(TaggedValue::boolean(unicode_digit_value(c).is_some()))
}

/// (char-whitespace? char) - Returns #t if char is whitespace
pub(super) fn char_whitespace_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-whitespace?")?;
    Ok(TaggedValue::boolean(c.is_whitespace()))
}

/// (char-upper-case? char) - Returns #t if char is uppercase
pub(super) fn char_upper_case_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-upper-case?")?;
    // R7RS: uppercase means the character has the Unicode Uppercase property
    Ok(TaggedValue::boolean(c.is_uppercase()))
}

/// (char-lower-case? char) - Returns #t if char is lowercase
pub(super) fn char_lower_case_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-lower-case?")?;
    // R7RS: lowercase means the character has the Unicode Lowercase property
    Ok(TaggedValue::boolean(c.is_lowercase()))
}

// ========== Character Case Conversion ==========

/// (char-upcase char) - Convert character to uppercase
pub(super) fn char_upcase(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-upcase")?;

    Ok(TaggedValue::character(simple_mapping(
        c,
        c.to_uppercase(),
        SIMPLE_UPPER_WHEN_FULL_EXPANDS,
    )))
}

/// (char-downcase char) - Convert character to lowercase
pub(super) fn char_downcase(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-downcase")?;

    Ok(TaggedValue::character(simple_mapping(
        c,
        c.to_lowercase(),
        SIMPLE_LOWER_WHEN_FULL_EXPANDS,
    )))
}

/// (char-foldcase char) - Case-folding (for case-insensitive comparison)
///
/// R7RS: Returns the case-folded character — Unicode *simple* folding, so
/// ß stays ß (its full folding "ss" is not a character) while ẞ folds to ß.
pub(super) fn char_foldcase(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-foldcase")?;

    Ok(TaggedValue::character(fold_char(c)))
}

// ========== Character/Integer Conversion ==========

/// (char->integer char) - Convert character to Unicode code point
pub(super) fn char_to_integer(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char->integer")?;
    Ok(TaggedValue::fixnum(c as u32 as i64))
}

/// (integer->char n) - Convert Unicode code point to character
pub(super) fn integer_to_char(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();
    let n = get_integer(args[0], &heap_ref, "integer->char")?;

    if n < 0 {
        return Err(EvalError::TypeError(format!(
            "integer->char: requires non-negative integer, got {}",
            n
        )));
    }

    let n = n as u32;
    match char::from_u32(n) {
        Some(c) => Ok(TaggedValue::character(c)),
        None => Err(EvalError::TypeError(format!(
            "integer->char: {} is not a valid Unicode code point",
            n
        ))),
    }
}

// ========== Digit Value ==========

/// (digit-value char) - Returns the numeric value of a digit character
/// Returns #f if the character is not a digit
///
/// R7RS requires this to work for all Unicode decimal digits (Nd category),
/// not just ASCII 0-9. Unicode decimal digits are organized in blocks where
/// each block contains 10 consecutive code points for 0-9.
pub(super) fn digit_value(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "digit-value")?;

    // R7RS: Returns the numeric value 0-9 for digit characters, #f otherwise
    // Must handle all Unicode decimal digits, not just ASCII
    match unicode_digit_value(c) {
        Some(d) => Ok(TaggedValue::fixnum(d as i64)),
        None => Ok(TaggedValue::FALSE),
    }
}

/// Returns the numeric value (0-9) of a Unicode decimal digit character,
/// or None if the character is not a decimal digit.
///
/// Unicode decimal digits (category Nd) are organized in blocks where
/// each script's digits 0-9 are consecutive code points. We can calculate
/// the digit value by finding the offset from the block's zero character.
///
/// Shared by `char-numeric?` and `digit-value` so their Unicode coverage agrees.
/// The table covers Unicode 15.0's 680 Nd characters (68 blocks), checked against
/// https://www.unicode.org/Public/15.0.0/ucd/UnicodeData.txt.
/// Update both procedures together by updating this table for a newer version.
fn unicode_digit_value(c: char) -> Option<u32> {
    // First try ASCII (fast path)
    if let Some(d) = c.to_digit(10) {
        return Some(d);
    }

    // For non-ASCII, find the digit's offset in a decimal digit block.
    let cp = c as u32;

    // List of Unicode decimal digit zero code points (Nd category)
    // Each entry is the code point for digit '0' in that script
    const DECIMAL_DIGIT_ZEROS: &[u32] = &[
        0x0030, // ASCII: 0-9
        0x0660, // Arabic-Indic: ٠-٩
        0x06F0, // Extended Arabic-Indic: ۰-۹
        0x07C0, // NKo: ߀-߉
        0x0966, // Devanagari: ०-९
        0x09E6, // Bengali: ০-৯
        0x0A66, // Gurmukhi: ੦-੯
        0x0AE6, // Gujarati: ૦-૯
        0x0B66, // Oriya: ୦-୯
        0x0BE6, // Tamil: ௦-௯
        0x0C66, // Telugu: ౦-౯
        0x0CE6, // Kannada: ೦-೯
        0x0D66, // Malayalam: ൦-൯
        0x0DE6, // Sinhala Lith: ෦-෯
        0x0E50, // Thai: ๐-๙
        0x0ED0, // Lao: ໐-໙
        0x0F20, // Tibetan: ༠-༩
        0x1040, // Myanmar: ၀-၉
        0x1090, // Myanmar Shan: ႐-႙
        0x17E0, // Khmer: ០-៩
        0x1810, // Mongolian: ᠐-᠙
        0x1946, // Limbu: ᥆-᥏
        0x19D0, // New Tai Lue: ᧐-᧙
        0x1A80, // Tai Tham Hora: ᪀-᪉
        0x1A90, // Tai Tham Tham: ᪐-᪙
        0x1B50, // Balinese: ᭐-᭙
        0x1BB0, // Sundanese: ᮰-᮹
        0x1C40, // Lepcha: ᱀-᱉
        0x1C50, // Ol Chiki: ᱐-᱙
        0xA620, // Vai: ꘠-꘩
        0xA8D0, // Saurashtra: ꣐-꣙
        0xA900, // Kayah Li: ꤀-꤉
        0xA9D0, // Javanese: ꧐-꧙
        0xA9F0, // Myanmar Tai Laing: ꧰-꧹
        0xAA50, // Cham: ꩐-꩙
        0xABF0, // Meetei Mayek: ꯰-꯹
        0xFF10, // Fullwidth: ０-９
        // Supplementary planes
        0x104A0, // Osmanya: 𐒠-𐒩
        0x10D30, // Hanifi Rohingya: 𐴰-𐴹
        0x11066, // Brahmi: 𑁦-𑁯
        0x110F0, // Sora Sompeng: 𑃰-𑃹
        0x11136, // Chakma: 𑄶-𑄿
        0x111D0, // Sharada: 𑇐-𑇙
        0x112F0, // Khudawadi: 𑋰-𑋹
        0x11450, // Newa: 𑑐-𑑙
        0x114D0, // Tirhuta: 𑓐-𑓙
        0x11650, // Modi: 𑙐-𑙙
        0x116C0, // Takri: 𑛀-𑛉
        0x11730, // Ahom: 𑜰-𑜹
        0x118E0, // Warang Citi: 𑣠-𑣩
        0x11950, // Dives Akuru: 𑥐-𑥙
        0x11C50, // Bhaiksuki: 𑱐-𑱙
        0x11D50, // Masaram Gondi: 𑵐-𑵙
        0x11DA0, // Gunjala Gondi: 𑶠-𑶩
        0x11F50, // Kawi: U+11F50-U+11F59
        0x16A60, // Mro: 𖩠-𖩩
        0x16AC0, // Tangsa: 𖫰-𖫹
        0x16B50, // Pahawh Hmong: 𖭐-𖭙
        0x1D7CE, // Mathematical Bold: 𝟎-𝟗
        0x1D7D8, // Mathematical Double-Struck: 𝟘-𝟡
        0x1D7E2, // Mathematical Sans-Serif: 𝟢-𝟫
        0x1D7EC, // Mathematical Sans-Serif Bold: 𝟬-𝟵
        0x1D7F6, // Mathematical Monospace: 𝟶-𝟿
        0x1E140, // Nyiakeng Puachue Hmong: 𞅀-𞅉
        0x1E2F0, // Wancho: 𞋰-𞋹
        0x1E4F0, // Nag Mundari: 𞓰-𞓹
        0x1E950, // Adlam: 𞥐-𞥙
        0x1FBF0, // Segmented Display: 🯰-🯹
    ];

    for &zero in DECIMAL_DIGIT_ZEROS {
        if cp >= zero && cp < zero + 10 {
            return Some(cp - zero);
        }
    }

    None
}

// ========== Unicode Character Classes as Ranges ==========

// `(srfi 14)` needs whole Unicode classes as sets, not one membership test at
// a time. Deriving them in Scheme costs a predicate call per scalar value:
// 1.1M calls per class, measured at 0.10s on the VM and 1.8s on the
// tree-walker for one class, and SRFI 14 has ten of them. So each class is
// scanned here once and handed to Scheme as ranges — every class is under 900
// of them, covering 1112064 code points.
//
// Two sources, deliberately at the same Unicode version. The properties `std`
// exposes (Alphabetic, Lowercase, Uppercase, White_Space, Cc) come from the
// same `char::is_*` methods that back `(scheme char)`'s predicates, so a
// char-set constant and its matching predicate cannot disagree. The general
// categories `std` has no table for come from `unicode-properties`, pinned to
// a version whose UCD matches the toolchain's; `unicode_versions_agree` below
// is the test that keeps that true across a bump.

/// The Unicode classes `char-set-unicode-ranges` can be asked for, each named
/// as SRFI 14 names the constant it backs.
///
/// One table rather than a `match`, so the set of classes can be enumerated:
/// `class_predicate` looks up here, and the tests iterate the same slice. A
/// class added to this table is therefore covered by them automatically, and
/// one cannot be added anywhere else.
#[allow(clippy::type_complexity)]
const CLASSES: &[(&str, fn(char) -> bool)] = &[
    // From std, so these agree with (scheme char)'s predicates exactly.
    ("alphabetic", |c| c.is_alphabetic()),
    ("lower-case", |c| c.is_lowercase()),
    ("upper-case", |c| c.is_uppercase()),
    ("whitespace", |c| c.is_whitespace()),
    ("iso-control", |c| c.is_control()),
    // Nd, shared with char-numeric? and digit-value so all three agree.
    ("numeric", |c| unicode_digit_value(c).is_some()),
    // General categories, which std has no table for.
    ("title-case", is_title_case),
    ("punctuation", is_punctuation),
    ("symbol", is_symbol),
    // SRFI 14's graphic is "a character that would put ink on paper":
    // everything except the separators and the non-printing categories.
    // Unassigned code points are not graphic, which is the distinction std
    // cannot make.
    ("graphic", is_graphic),
];

/// Unicode general category Lt.
fn is_title_case(c: char) -> bool {
    use unicode_properties::GeneralCategory as G;
    use unicode_properties::UnicodeGeneralCategory;
    c.general_category() == G::TitlecaseLetter
}

/// Unicode general categories P* — connector, dash, open, close, initial,
/// final and other punctuation.
fn is_punctuation(c: char) -> bool {
    use unicode_properties::GeneralCategory as G;
    use unicode_properties::UnicodeGeneralCategory;
    matches!(
        c.general_category(),
        G::ConnectorPunctuation
            | G::DashPunctuation
            | G::OpenPunctuation
            | G::ClosePunctuation
            | G::InitialPunctuation
            | G::FinalPunctuation
            | G::OtherPunctuation
    )
}

/// Unicode general categories S* — math, currency, modifier and other symbols.
fn is_symbol(c: char) -> bool {
    use unicode_properties::GeneralCategory as G;
    use unicode_properties::UnicodeGeneralCategory;
    matches!(
        c.general_category(),
        G::MathSymbol | G::CurrencySymbol | G::ModifierSymbol | G::OtherSymbol
    )
}

/// A character that would put ink on paper: not a separator, and not one of
/// the non-printing categories.
fn is_graphic(c: char) -> bool {
    use unicode_properties::GeneralCategory as G;
    use unicode_properties::UnicodeGeneralCategory;
    !matches!(
        c.general_category(),
        G::Control
            | G::Format
            | G::Surrogate
            | G::PrivateUse
            | G::Unassigned
            | G::SpaceSeparator
            | G::LineSeparator
            | G::ParagraphSeparator
    )
}

fn class_predicate(name: &str) -> Option<fn(char) -> bool> {
    CLASSES
        .iter()
        .find(|(class, _)| *class == name)
        .map(|(_, pred)| *pred)
}

/// (char-set-unicode-ranges class) - the code-point ranges of a Unicode class
///
/// `class` is a symbol naming one of the classes in `class_predicate`.
/// Returns a vector of alternating inclusive bounds — `#(lo hi lo hi ...)`,
/// ascending and disjoint — covering every scalar value in the class.
/// Surrogates are never members: they are not characters, `integer->char`
/// rejects them, and so a char-set can never be asked about one.
///
/// A vector rather than a list of pairs, because the largest class is 761
/// ranges and `(srfi 14)` reads the result exactly once, to build its own
/// representation from it.
pub(super) fn char_set_unicode_ranges(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Scoped so the shared borrow ends before the allocation below takes a
    // mutable one.
    let class = {
        let heap_ref = heap.borrow();
        match heap_ref.get_symbol_name(args[0]) {
            Some(name) => name.to_string(),
            None => {
                return Err(EvalError::TypeError(
                    "char-set-unicode-ranges: requires a symbol naming a Unicode class".to_string(),
                ));
            }
        }
    };

    let pred = class_predicate(&class).ok_or_else(|| {
        EvalError::TypeError(format!(
            "char-set-unicode-ranges: unknown Unicode class `{}`",
            class
        ))
    })?;

    let bounds: Vec<TaggedValue> = cached_class_ranges(&class, pred)
        .iter()
        .flat_map(|&(lo, hi)| {
            [
                TaggedValue::fixnum(lo as i64),
                TaggedValue::fixnum(hi as i64),
            ]
        })
        .collect();

    Ok(heap.borrow_mut().alloc_vector(bounds))
}

/// One class's coalesced ranges, shared between every ask for that class.
type ClassRanges = Arc<Vec<(u32, u32)>>;

/// The scan is 1.1M predicate calls, and the answer never changes for a given
/// class within a process, so each class is scanned at most once. `(srfi 14)`
/// asks for ten classes at import, and a REPL session or a test binary that
/// imports it more than once would otherwise pay for every one again.
fn cached_class_ranges(class: &str, pred: fn(char) -> bool) -> ClassRanges {
    static CACHE: OnceLock<Mutex<HashMap<String, ClassRanges>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    // Scoped so the lock is not held across the scan: a poisoned lock here
    // would take the whole library down, and the scan is the slow part.
    if let Ok(map) = cache.lock()
        && let Some(hit) = map.get(class)
    {
        return Arc::clone(hit);
    }

    let ranges = Arc::new(unicode_class_ranges(pred));
    if let Ok(mut map) = cache.lock() {
        map.insert(class.to_string(), Arc::clone(&ranges));
    }
    ranges
}

/// Scans the scalar values once, coalescing `pred`'s members into ascending
/// inclusive ranges. A surrogate breaks a range: it is not a character, so it
/// cannot be a member.
fn unicode_class_ranges(pred: fn(char) -> bool) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    let mut open: Option<(u32, u32)> = None;

    for cp in 0..=0x10FFFFu32 {
        let member = char::from_u32(cp).is_some_and(pred);
        match (member, open) {
            (true, None) => open = Some((cp, cp)),
            (true, Some((lo, _))) => open = Some((lo, cp)),
            (false, Some(range)) => {
                ranges.push(range);
                open = None;
            }
            (false, None) => {}
        }
    }
    if let Some(range) = open {
        ranges.push(range);
    }
    ranges
}

// ========== Registration ==========

/// Register all character primitives with the registry
pub(super) fn register(registry: &mut crate::registry::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Character comparisons
    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char=?",
        Arity::Min(2),
        "Returns #t if all characters are equal.",
        char_equal,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char<?",
        Arity::Min(2),
        "Returns #t if characters are monotonically increasing.",
        char_lt,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char>?",
        Arity::Min(2),
        "Returns #t if characters are monotonically decreasing.",
        char_gt,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char<=?",
        Arity::Min(2),
        "Returns #t if characters are monotonically non-decreasing.",
        char_le,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char>=?",
        Arity::Min(2),
        "Returns #t if characters are monotonically non-increasing.",
        char_ge,
    ));

    // Case-insensitive comparisons
    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-ci=?",
        Arity::Min(2),
        "Returns #t if all characters are equal, ignoring case.",
        char_ci_equal,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-ci<?",
        Arity::Min(2),
        "Case-insensitive character less than.",
        char_ci_lt,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-ci>?",
        Arity::Min(2),
        "Case-insensitive character greater than.",
        char_ci_gt,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-ci<=?",
        Arity::Min(2),
        "Case-insensitive character less than or equal.",
        char_ci_le,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-ci>=?",
        Arity::Min(2),
        "Case-insensitive character greater than or equal.",
        char_ci_ge,
    ));

    // Type predicates
    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-alphabetic?",
        Arity::Exact(1),
        "Returns #t if char is alphabetic.",
        char_alphabetic_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-numeric?",
        Arity::Exact(1),
        "Returns #t if char is a Unicode decimal digit.",
        char_numeric_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-whitespace?",
        Arity::Exact(1),
        "Returns #t if char is whitespace.",
        char_whitespace_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-upper-case?",
        Arity::Exact(1),
        "Returns #t if char is uppercase.",
        char_upper_case_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-lower-case?",
        Arity::Exact(1),
        "Returns #t if char is lowercase.",
        char_lower_case_p,
    ));

    // Case conversion
    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-upcase",
        Arity::Exact(1),
        "Returns the uppercase version of char.",
        char_upcase,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-downcase",
        Arity::Exact(1),
        "Returns the lowercase version of char.",
        char_downcase,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-foldcase",
        Arity::Exact(1),
        "Returns the case-folded version of char.",
        char_foldcase,
    ));

    // Character/integer conversion
    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char->integer",
        Arity::Exact(1),
        "Returns the Unicode code point of char.",
        char_to_integer,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "integer->char",
        Arity::Exact(1),
        "Returns the character with the given Unicode code point.",
        integer_to_char,
    ));

    // Digit value
    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "digit-value",
        Arity::Exact(1),
        "Returns the numeric value of a digit character, or #f.",
        digit_value,
    ));

    // Unicode character classes, for (srfi 14)'s constants
    registry.register(PrimitiveFn::new_heap(
        "scheme.char",
        "char-set-unicode-ranges",
        Arity::Exact(1),
        "Returns a vector of alternating inclusive code-point bounds for a named Unicode class.",
        char_set_unicode_ranges,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two Unicode sources this module reads must describe the same
    /// Unicode, or `char-set:letter` (std, through `char-alphabetic?`) and
    /// `char-set:punctuation` (the category table) would disagree about
    /// characters one version added and the other has not.
    ///
    /// If a toolchain bump trips this, bump `unicode-properties` to the
    /// version with the matching UCD in the same change.
    #[test]
    fn unicode_versions_agree() {
        let (sa, sb, sc) = char::UNICODE_VERSION;
        let (ca, cb, cc) = unicode_properties::UNICODE_VERSION;
        assert_eq!(
            (sa as u64, sb as u64, sc as u64),
            (ca, cb, cc),
            "std and unicode-properties disagree on the Unicode version"
        );
    }

    #[test]
    fn ranges_are_ascending_disjoint_and_non_adjacent() {
        for &(class, pred) in CLASSES {
            let ranges = unicode_class_ranges(pred);
            assert!(!ranges.is_empty(), "{class} is empty");
            for (i, &(lo, hi)) in ranges.iter().enumerate() {
                assert!(lo <= hi, "{class} range {i} is inverted");
                assert!(hi <= 0x10FFFF, "{class} range {i} is out of range");
                if i > 0 {
                    let prev_hi = ranges[i - 1].1;
                    // Strictly greater than prev_hi + 1: adjacent ranges
                    // would mean the scan failed to coalesce.
                    assert!(
                        lo > prev_hi + 1,
                        "{class} ranges {} and {i} are adjacent or overlap",
                        i - 1
                    );
                }
            }
        }
    }

    /// Every range member satisfies the class predicate, and no code point
    /// between two ranges does. This is the property `(srfi 14)` relies on:
    /// the ranges are the class, exactly.
    #[test]
    fn ranges_hold_exactly_the_class() {
        // A subset by name rather than all of `CLASSES`: this check walks
        // every member and then rescans the whole scalar range, so the big
        // classes would make it minutes rather than seconds. The cheap
        // invariants above cover all ten.
        for class in ["title-case", "whitespace", "iso-control", "numeric"] {
            let pred = class_predicate(class).expect("class is known");
            let ranges = unicode_class_ranges(pred);
            let mut in_ranges = 0u32;
            for &(lo, hi) in &ranges {
                for cp in lo..=hi {
                    if let Some(c) = char::from_u32(cp) {
                        assert!(
                            pred(c),
                            "{class}: U+{cp:04X} is in a range but not in the class"
                        );
                        in_ranges += 1;
                    }
                }
            }
            let scanned = (0..=0x10FFFFu32)
                .filter(|&cp| char::from_u32(cp).is_some_and(pred))
                .count() as u32;
            assert_eq!(in_ranges, scanned, "{class}: ranges miss members");
        }
    }

    /// Surrogates are not characters, so no class contains one. This is the
    /// invariant `(srfi 14)`'s `%class` relies on to skip its clipping step,
    /// so every class is checked, not just one.
    #[test]
    fn surrogates_are_never_members() {
        for &(class, pred) in CLASSES {
            for &(lo, hi) in &unicode_class_ranges(pred) {
                assert!(
                    hi < 0xD800 || lo > 0xDFFF,
                    "{class}: a range spans the surrogates: U+{lo:04X}..U+{hi:04X}"
                );
            }
        }
    }

    #[test]
    fn unknown_class_is_rejected() {
        assert!(class_predicate("no-such-class").is_none());
    }

    /// `CLASSES` is the only place a class is defined, so the tests above
    /// cover every one by construction. What is left to check is that
    /// `class_predicate` reaches all of them — a duplicate name would shadow
    /// the later entry and leave it unreachable — and that the count is what
    /// `lib/srfi/14.scm` asks for and the cost argument above quotes.
    #[test]
    fn every_class_is_reachable_and_named_once() {
        assert_eq!(CLASSES.len(), 10, "the comments quote ten classes");
        let mut names: Vec<&str> = CLASSES.iter().map(|(class, _)| *class).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "a class is named twice in CLASSES");
        for (class, _) in CLASSES {
            assert!(
                class_predicate(class).is_some(),
                "{class} is in CLASSES but class_predicate cannot reach it"
            );
        }
    }

    /// The cache must answer with the same ranges the scan does, and the
    /// second ask must not rescan — which is only observable as the same
    /// allocation coming back.
    #[test]
    fn the_cache_answers_with_the_scan() {
        let pred = class_predicate("title-case").expect("class is known");
        let first = cached_class_ranges("title-case", pred);
        let second = cached_class_ranges("title-case", pred);
        assert_eq!(*first, unicode_class_ranges(pred));
        assert!(Arc::ptr_eq(&first, &second), "the second ask rescanned");
    }

    /// Lt has 31 members in Unicode 17.0, which is what chibi and Gauche both
    /// report for `char-set:title-case`. The reference implementation this
    /// replaced had it empty.
    #[test]
    fn title_case_matches_the_references() {
        let pred = class_predicate("title-case").expect("class is known");
        let members: u32 = unicode_class_ranges(pred)
            .iter()
            .map(|(lo, hi)| hi - lo + 1)
            .sum();
        assert_eq!(members, 31);
    }
}
