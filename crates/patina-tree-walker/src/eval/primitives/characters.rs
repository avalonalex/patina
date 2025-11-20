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
use patina_runtime::value::Value;

// ========== Character Comparisons ==========

/// (char=? char1 char2 ...) - Character equality
pub(super) fn char_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char=?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char=?")?;
        let c2 = get_char(&args[i + 1], "char=?")?;
        if c1 != c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// (char<? char1 char2 ...) - Character less than
pub(super) fn char_lt(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char<?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char<?")?;
        let c2 = get_char(&args[i + 1], "char<?")?;
        if c1 >= c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// (char>? char1 char2 ...) - Character greater than
pub(super) fn char_gt(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char>?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char>?")?;
        let c2 = get_char(&args[i + 1], "char>?")?;
        if c1 <= c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// (char<=? char1 char2 ...) - Character less than or equal
pub(super) fn char_le(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char<=?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char<=?")?;
        let c2 = get_char(&args[i + 1], "char<=?")?;
        if c1 > c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// (char>=? char1 char2 ...) - Character greater than or equal
pub(super) fn char_ge(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char>=?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char>=?")?;
        let c2 = get_char(&args[i + 1], "char>=?")?;
        if c1 < c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

// ========== Case-Insensitive Character Comparisons ==========

/// (char-ci=? char1 char2 ...) - Case-insensitive character equality
pub(super) fn char_ci_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char-ci=?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char-ci=?")?.to_lowercase().to_string();
        let c2 = get_char(&args[i + 1], "char-ci=?")?
            .to_lowercase()
            .to_string();
        if c1 != c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// (char-ci<? char1 char2 ...) - Case-insensitive character less than
pub(super) fn char_ci_lt(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char-ci<?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char-ci<?")?.to_lowercase().to_string();
        let c2 = get_char(&args[i + 1], "char-ci<?")?
            .to_lowercase()
            .to_string();
        if c1 >= c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// (char-ci>? char1 char2 ...) - Case-insensitive character greater than
pub(super) fn char_ci_gt(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char-ci>?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char-ci>?")?.to_lowercase().to_string();
        let c2 = get_char(&args[i + 1], "char-ci>?")?
            .to_lowercase()
            .to_string();
        if c1 <= c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// (char-ci<=? char1 char2 ...) - Case-insensitive character less than or equal
pub(super) fn char_ci_le(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char-ci<=?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char-ci<=?")?.to_lowercase().to_string();
        let c2 = get_char(&args[i + 1], "char-ci<=?")?
            .to_lowercase()
            .to_string();
        if c1 > c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// (char-ci>=? char1 char2 ...) - Case-insensitive character greater than or equal
pub(super) fn char_ci_ge(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "char-ci>=?")?;

    for i in 0..args.len() - 1 {
        let c1 = get_char(&args[i], "char-ci>=?")?.to_lowercase().to_string();
        let c2 = get_char(&args[i + 1], "char-ci>=?")?
            .to_lowercase()
            .to_string();
        if c1 < c2 {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

// ========== Character Type Predicates ==========

/// (char-alphabetic? char) - Returns #t if char is alphabetic
pub(super) fn char_alphabetic_p(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "char-alphabetic?")?;
    let c = get_char(&args[0], "char-alphabetic?")?;
    Ok(Value::Boolean(c.is_alphabetic()))
}

/// (char-numeric? char) - Returns #t if char is numeric
pub(super) fn char_numeric_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "char-numeric?")?;
    let c = get_char(&args[0], "char-numeric?")?;
    Ok(Value::Boolean(c.is_numeric()))
}

/// (char-whitespace? char) - Returns #t if char is whitespace
pub(super) fn char_whitespace_p(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "char-whitespace?")?;
    let c = get_char(&args[0], "char-whitespace?")?;
    Ok(Value::Boolean(c.is_whitespace()))
}

/// (char-upper-case? char) - Returns #t if char is uppercase
pub(super) fn char_upper_case_p(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "char-upper-case?")?;
    let c = get_char(&args[0], "char-upper-case?")?;
    // R7RS: uppercase means the character has the Unicode Uppercase property
    Ok(Value::Boolean(c.is_uppercase()))
}

/// (char-lower-case? char) - Returns #t if char is lowercase
pub(super) fn char_lower_case_p(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "char-lower-case?")?;
    let c = get_char(&args[0], "char-lower-case?")?;
    // R7RS: lowercase means the character has the Unicode Lowercase property
    Ok(Value::Boolean(c.is_lowercase()))
}

// ========== Character Case Conversion ==========

/// (char-upcase char) - Convert character to uppercase
pub(super) fn char_upcase(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "char-upcase")?;
    let c = get_char(&args[0], "char-upcase")?;

    // R7RS: Returns the uppercase version, using full Unicode case mapping
    // If multiple chars result, we use the first (per R7RS simple case mapping)
    let upper: Vec<char> = c.to_uppercase().collect();
    Ok(Value::Character(upper[0]))
}

/// (char-downcase char) - Convert character to lowercase
pub(super) fn char_downcase(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "char-downcase")?;
    let c = get_char(&args[0], "char-downcase")?;

    // R7RS: Returns the lowercase version, using full Unicode case mapping
    let lower: Vec<char> = c.to_lowercase().collect();
    Ok(Value::Character(lower[0]))
}

/// (char-foldcase char) - Case-folding (for case-insensitive comparison)
pub(super) fn char_foldcase(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "char-foldcase")?;
    let c = get_char(&args[0], "char-foldcase")?;

    // R7RS: Case folding is used for case-insensitive string comparison
    // In Rust, we can approximate this with to_lowercase() which handles most cases
    let folded: Vec<char> = c.to_lowercase().collect();
    Ok(Value::Character(folded[0]))
}

// ========== Character/Integer Conversion ==========

/// (char->integer char) - Convert character to Unicode code point
pub(super) fn char_to_integer(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "char->integer")?;
    let c = get_char(&args[0], "char->integer")?;
    Ok(Value::Integer(c as u32 as i64))
}

/// (integer->char n) - Convert Unicode code point to character
pub(super) fn integer_to_char(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "integer->char")?;

    let n = match &args[0] {
        Value::Integer(n) if *n >= 0 => *n as u32,
        Value::Integer(n) => {
            return Err(EvalError::TypeError(format!(
                "integer->char: requires non-negative integer, got {}",
                n
            )));
        }
        other => {
            return Err(EvalError::TypeError(format!(
                "integer->char: requires integer, got {}",
                other.type_name()
            )));
        }
    };

    match char::from_u32(n) {
        Some(c) => Ok(Value::Character(c)),
        None => Err(EvalError::TypeError(format!(
            "integer->char: {} is not a valid Unicode code point",
            n
        ))),
    }
}

// ========== Digit Value ==========

/// (digit-value char) - Returns the numeric value of a digit character
/// Returns #f if the character is not a digit
pub(super) fn digit_value(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "digit-value")?;
    let c = get_char(&args[0], "digit-value")?;

    // R7RS: Returns the numeric value 0-9 for digit characters, #f otherwise
    match c.to_digit(10) {
        Some(d) => Ok(Value::Integer(d as i64)),
        None => Ok(Value::Boolean(false)),
    }
}

// ========== Helper Functions ==========

/// Extract a character from a Value, or return an error
fn get_char(val: &Value, func_name: &str) -> Result<char, EvalError> {
    match val {
        Value::Character(c) => Ok(*c),
        other => Err(EvalError::TypeError(format!(
            "{}: requires character, got {}",
            func_name,
            other.type_name()
        ))),
    }
}

// ========== Registration ==========

/// Register all character primitives with the registry
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Character comparisons
    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char=?",
        Arity::Min(2),
        "Returns #t if all characters are equal.",
        |eval, args, _tail| char_equal(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char<?",
        Arity::Min(2),
        "Returns #t if characters are monotonically increasing.",
        |eval, args, _tail| char_lt(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char>?",
        Arity::Min(2),
        "Returns #t if characters are monotonically decreasing.",
        |eval, args, _tail| char_gt(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char<=?",
        Arity::Min(2),
        "Returns #t if characters are monotonically non-decreasing.",
        |eval, args, _tail| char_le(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char>=?",
        Arity::Min(2),
        "Returns #t if characters are monotonically non-increasing.",
        |eval, args, _tail| char_ge(eval, args).map(EvalResult::Value),
    ));

    // Case-insensitive comparisons
    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-ci=?",
        Arity::Min(2),
        "Returns #t if all characters are equal, ignoring case.",
        |eval, args, _tail| char_ci_equal(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-ci<?",
        Arity::Min(2),
        "Case-insensitive character less than.",
        |eval, args, _tail| char_ci_lt(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-ci>?",
        Arity::Min(2),
        "Case-insensitive character greater than.",
        |eval, args, _tail| char_ci_gt(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-ci<=?",
        Arity::Min(2),
        "Case-insensitive character less than or equal.",
        |eval, args, _tail| char_ci_le(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-ci>=?",
        Arity::Min(2),
        "Case-insensitive character greater than or equal.",
        |eval, args, _tail| char_ci_ge(eval, args).map(EvalResult::Value),
    ));

    // Type predicates
    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-alphabetic?",
        Arity::Exact(1),
        "Returns #t if char is alphabetic.",
        |eval, args, _tail| char_alphabetic_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-numeric?",
        Arity::Exact(1),
        "Returns #t if char is numeric.",
        |eval, args, _tail| char_numeric_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-whitespace?",
        Arity::Exact(1),
        "Returns #t if char is whitespace.",
        |eval, args, _tail| char_whitespace_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-upper-case?",
        Arity::Exact(1),
        "Returns #t if char is uppercase.",
        |eval, args, _tail| char_upper_case_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-lower-case?",
        Arity::Exact(1),
        "Returns #t if char is lowercase.",
        |eval, args, _tail| char_lower_case_p(eval, args).map(EvalResult::Value),
    ));

    // Case conversion
    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-upcase",
        Arity::Exact(1),
        "Returns the uppercase version of char.",
        |eval, args, _tail| char_upcase(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-downcase",
        Arity::Exact(1),
        "Returns the lowercase version of char.",
        |eval, args, _tail| char_downcase(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char-foldcase",
        Arity::Exact(1),
        "Returns the case-folded version of char.",
        |eval, args, _tail| char_foldcase(eval, args).map(EvalResult::Value),
    ));

    // Character/integer conversion
    registry.register(PrimitiveFn::new(
        "scheme.char",
        "char->integer",
        Arity::Exact(1),
        "Returns the Unicode code point of char.",
        |eval, args, _tail| char_to_integer(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "integer->char",
        Arity::Exact(1),
        "Returns the character with the given Unicode code point.",
        |eval, args, _tail| integer_to_char(eval, args).map(EvalResult::Value),
    ));

    // Digit value
    registry.register(PrimitiveFn::new(
        "scheme.char",
        "digit-value",
        Arity::Exact(1),
        "Returns the numeric value of a digit character, or #f.",
        |eval, args, _tail| digit_value(eval, args).map(EvalResult::Value),
    ));
}
