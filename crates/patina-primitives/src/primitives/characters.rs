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

/// (char-numeric? char) - Returns #t if char is numeric
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
    Ok(TaggedValue::boolean(c.is_numeric()))
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
/// Note: The table below covers Unicode 15.0 Nd ranges. If future Unicode versions
/// add new decimal digit scripts, this table should be updated. Alternative approaches
/// like `unicode-general-category` crate could provide automatic updates but would
/// add a dependency for marginal benefit since decimal digit ranges rarely change.
fn unicode_digit_value(c: char) -> Option<u32> {
    // First try ASCII (fast path)
    if let Some(d) = c.to_digit(10) {
        return Some(d);
    }

    // For non-ASCII, check if it's a decimal digit using Unicode properties
    // A character is a decimal digit if char::is_numeric() returns true AND
    // it's in the Nd (Decimal Number) category.
    //
    // Unicode decimal digit blocks all have 10 consecutive code points.
    // We can find the digit value by checking known decimal digit ranges.
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
        "Returns #t if char is numeric.",
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
}
