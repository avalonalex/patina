//! Character primitive operations (R7RS Section 6.6)
//!
//! Implements character operations including:
//! - Character comparisons (char=?, char<?, etc.)
//! - Character type predicates (char-alphabetic?, char-numeric?, etc.)
//! - Character case conversion (char-upcase, char-downcase, char-foldcase)
//! - Character/integer conversion (char->integer, integer->char)
//! - Digit value extraction (digit-value)

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_core::TaggedValue;

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
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
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
pub(super) fn char_lt(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
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
pub(super) fn char_gt(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
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
pub(super) fn char_le(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
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
pub(super) fn char_ge(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
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

/// (char-ci=? char1 char2 ...) - Case-insensitive character equality
pub(super) fn char_ci_equal(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char-ci=?")?
            .to_lowercase()
            .to_string();
        let c2 = get_char(args[i + 1], &heap_ref, "char-ci=?")?
            .to_lowercase()
            .to_string();
        if c1 != c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char-ci<? char1 char2 ...) - Case-insensitive character less than
pub(super) fn char_ci_lt(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char-ci<?")?
            .to_lowercase()
            .to_string();
        let c2 = get_char(args[i + 1], &heap_ref, "char-ci<?")?
            .to_lowercase()
            .to_string();
        if c1 >= c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char-ci>? char1 char2 ...) - Case-insensitive character greater than
pub(super) fn char_ci_gt(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char-ci>?")?
            .to_lowercase()
            .to_string();
        let c2 = get_char(args[i + 1], &heap_ref, "char-ci>?")?
            .to_lowercase()
            .to_string();
        if c1 <= c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char-ci<=? char1 char2 ...) - Case-insensitive character less than or equal
pub(super) fn char_ci_le(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char-ci<=?")?
            .to_lowercase()
            .to_string();
        let c2 = get_char(args[i + 1], &heap_ref, "char-ci<=?")?
            .to_lowercase()
            .to_string();
        if c1 > c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

/// (char-ci>=? char1 char2 ...) - Case-insensitive character greater than or equal
pub(super) fn char_ci_ge(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let c1 = get_char(args[i], &heap_ref, "char-ci>=?")?
            .to_lowercase()
            .to_string();
        let c2 = get_char(args[i + 1], &heap_ref, "char-ci>=?")?
            .to_lowercase()
            .to_string();
        if c1 < c2 {
            return Ok(TaggedValue::FALSE);
        }
    }
    Ok(TaggedValue::TRUE)
}

// ========== Character Type Predicates ==========

/// (char-alphabetic? char) - Returns #t if char is alphabetic
pub(super) fn char_alphabetic_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-alphabetic?")?;
    Ok(TaggedValue::boolean(c.is_alphabetic()))
}

/// (char-numeric? char) - Returns #t if char is numeric
pub(super) fn char_numeric_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-numeric?")?;
    Ok(TaggedValue::boolean(c.is_numeric()))
}

/// (char-whitespace? char) - Returns #t if char is whitespace
pub(super) fn char_whitespace_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-whitespace?")?;
    Ok(TaggedValue::boolean(c.is_whitespace()))
}

/// (char-upper-case? char) - Returns #t if char is uppercase
pub(super) fn char_upper_case_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-upper-case?")?;
    // R7RS: uppercase means the character has the Unicode Uppercase property
    Ok(TaggedValue::boolean(c.is_uppercase()))
}

/// (char-lower-case? char) - Returns #t if char is lowercase
pub(super) fn char_lower_case_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-lower-case?")?;
    // R7RS: lowercase means the character has the Unicode Lowercase property
    Ok(TaggedValue::boolean(c.is_lowercase()))
}

// ========== Character Case Conversion ==========

/// (char-upcase char) - Convert character to uppercase
pub(super) fn char_upcase(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-upcase")?;

    // R7RS: Returns the uppercase version, using full Unicode case mapping
    // If multiple chars result, we use the first (per R7RS simple case mapping)
    let upper: Vec<char> = c.to_uppercase().collect();
    Ok(TaggedValue::character(upper[0]))
}

/// (char-downcase char) - Convert character to lowercase
pub(super) fn char_downcase(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-downcase")?;

    // R7RS: Returns the lowercase version, using full Unicode case mapping
    let lower: Vec<char> = c.to_lowercase().collect();
    Ok(TaggedValue::character(lower[0]))
}

/// (char-foldcase char) - Case-folding (for case-insensitive comparison)
///
/// R7RS: Returns the case-folded character. Note that full case folding
/// can map one character to multiple (e.g., ß → ss), but char-foldcase
/// must return a single character. We use simple case folding here.
pub(super) fn char_foldcase(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char-foldcase")?;

    use unicode_casefold::UnicodeCaseFold;

    // For char-foldcase, we need to return a single character.
    // Full case folding can expand (ß → ss), so we take the first character.
    // This matches R7RS which says char-foldcase returns "a" character.
    let folded: String = c.case_fold().collect();
    let first_char = folded.chars().next().unwrap_or(c);
    Ok(TaggedValue::character(first_char))
}

// ========== Character/Integer Conversion ==========

/// (char->integer char) - Convert character to Unicode code point
pub(super) fn char_to_integer(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let c = get_char(args[0], &heap_ref, "char->integer")?;
    Ok(TaggedValue::fixnum(c as u32 as i64))
}

/// (integer->char n) - Convert Unicode code point to character
pub(super) fn integer_to_char(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
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
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
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
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Character comparisons
    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char=?",
        Arity::Min(2),
        "Returns #t if all characters are equal.",
        |eval, args, _tail| char_equal(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char<?",
        Arity::Min(2),
        "Returns #t if characters are monotonically increasing.",
        |eval, args, _tail| char_lt(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char>?",
        Arity::Min(2),
        "Returns #t if characters are monotonically decreasing.",
        |eval, args, _tail| char_gt(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char<=?",
        Arity::Min(2),
        "Returns #t if characters are monotonically non-decreasing.",
        |eval, args, _tail| char_le(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char>=?",
        Arity::Min(2),
        "Returns #t if characters are monotonically non-increasing.",
        |eval, args, _tail| char_ge(eval, args).map(EvalResult::Tagged),
    ));

    // Case-insensitive comparisons
    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-ci=?",
        Arity::Min(2),
        "Returns #t if all characters are equal, ignoring case.",
        |eval, args, _tail| char_ci_equal(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-ci<?",
        Arity::Min(2),
        "Case-insensitive character less than.",
        |eval, args, _tail| char_ci_lt(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-ci>?",
        Arity::Min(2),
        "Case-insensitive character greater than.",
        |eval, args, _tail| char_ci_gt(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-ci<=?",
        Arity::Min(2),
        "Case-insensitive character less than or equal.",
        |eval, args, _tail| char_ci_le(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-ci>=?",
        Arity::Min(2),
        "Case-insensitive character greater than or equal.",
        |eval, args, _tail| char_ci_ge(eval, args).map(EvalResult::Tagged),
    ));

    // Type predicates
    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-alphabetic?",
        Arity::Exact(1),
        "Returns #t if char is alphabetic.",
        |eval, args, _tail| char_alphabetic_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-numeric?",
        Arity::Exact(1),
        "Returns #t if char is numeric.",
        |eval, args, _tail| char_numeric_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-whitespace?",
        Arity::Exact(1),
        "Returns #t if char is whitespace.",
        |eval, args, _tail| char_whitespace_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-upper-case?",
        Arity::Exact(1),
        "Returns #t if char is uppercase.",
        |eval, args, _tail| char_upper_case_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-lower-case?",
        Arity::Exact(1),
        "Returns #t if char is lowercase.",
        |eval, args, _tail| char_lower_case_p(eval, args).map(EvalResult::Tagged),
    ));

    // Case conversion
    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-upcase",
        Arity::Exact(1),
        "Returns the uppercase version of char.",
        |eval, args, _tail| char_upcase(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-downcase",
        Arity::Exact(1),
        "Returns the lowercase version of char.",
        |eval, args, _tail| char_downcase(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char-foldcase",
        Arity::Exact(1),
        "Returns the case-folded version of char.",
        |eval, args, _tail| char_foldcase(eval, args).map(EvalResult::Tagged),
    ));

    // Character/integer conversion
    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "char->integer",
        Arity::Exact(1),
        "Returns the Unicode code point of char.",
        |eval, args, _tail| char_to_integer(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "integer->char",
        Arity::Exact(1),
        "Returns the character with the given Unicode code point.",
        |eval, args, _tail| integer_to_char(eval, args).map(EvalResult::Tagged),
    ));

    // Digit value
    registry.register(PrimitiveFn::new_tagged(
        "scheme.char",
        "digit-value",
        Arity::Exact(1),
        "Returns the numeric value of a digit character, or #f.",
        |eval, args, _tail| digit_value(eval, args).map(EvalResult::Tagged),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unicode_digit_value_ascii() {
        assert_eq!(unicode_digit_value('0'), Some(0));
        assert_eq!(unicode_digit_value('5'), Some(5));
        assert_eq!(unicode_digit_value('9'), Some(9));
    }

    #[test]
    fn test_unicode_digit_value_arabic_indic() {
        // Arabic-Indic digits ٠-٩ (U+0660-U+0669)
        assert_eq!(unicode_digit_value('٠'), Some(0)); // U+0660
        assert_eq!(unicode_digit_value('٤'), Some(4)); // U+0664
        assert_eq!(unicode_digit_value('٩'), Some(9)); // U+0669
    }

    #[test]
    fn test_unicode_digit_value_gujarati() {
        // Gujarati digits ૦-૯ (U+0AE6-U+0AEF)
        assert_eq!(unicode_digit_value('૦'), Some(0)); // U+0AE6
        assert_eq!(unicode_digit_value('૫'), Some(5)); // U+0AEB
        assert_eq!(unicode_digit_value('૯'), Some(9)); // U+0AEF
    }

    #[test]
    fn test_unicode_digit_value_devanagari() {
        // Devanagari digits ०-९ (U+0966-U+096F)
        assert_eq!(unicode_digit_value('०'), Some(0)); // U+0966
        assert_eq!(unicode_digit_value('५'), Some(5)); // U+096B
        assert_eq!(unicode_digit_value('९'), Some(9)); // U+096F
    }

    #[test]
    fn test_unicode_digit_value_thai() {
        // Thai digits ๐-๙ (U+0E50-U+0E59)
        assert_eq!(unicode_digit_value('๐'), Some(0)); // U+0E50
        assert_eq!(unicode_digit_value('๕'), Some(5)); // U+0E55
        assert_eq!(unicode_digit_value('๙'), Some(9)); // U+0E59
    }

    #[test]
    fn test_unicode_digit_value_fullwidth() {
        // Fullwidth digits ０-９ (U+FF10-U+FF19)
        assert_eq!(unicode_digit_value('０'), Some(0)); // U+FF10
        assert_eq!(unicode_digit_value('５'), Some(5)); // U+FF15
        assert_eq!(unicode_digit_value('９'), Some(9)); // U+FF19
    }

    #[test]
    fn test_unicode_digit_value_non_digits() {
        assert_eq!(unicode_digit_value('a'), None);
        assert_eq!(unicode_digit_value('.'), None);
        assert_eq!(unicode_digit_value('-'), None);
        assert_eq!(unicode_digit_value(' '), None);
        // Roman numerals are NOT decimal digits
        assert_eq!(unicode_digit_value('Ⅳ'), None); // U+2163 Roman numeral four
    }
}
