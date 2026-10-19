//! String primitive operations with O(1) character access (R7RS Section 6.7)
//!
//! Implements string operations using Vec<char> for O(1) random access:
//! - Character indexing is O(1)
//! - Mutation via string-set! is O(1)
//! - Comparison operations (case-sensitive and case-insensitive)
//!
//! Note: UTF-8 conversion happens at I/O boundaries.
//!
//! Each takes the context (#706). A read borrows the string's characters
//! through the `Cx`, so a borrow held across an allocation does not
//! compile, and the mutators write through the context's string stores
//! (`Cx::string_set_char`, `string_fill`, `string_copy_chars`).

use patina_core::{Cx, Value};
use patina_runtime::EvalError;
use std::cell::Ref;

// ========== Argument Extraction Helpers ==========

/// Borrow a string's characters for read operations.
///
/// Borrowed, not copied: `string-ref` and `string-length` sit inside the
/// `(scheme base)` higher-order loops, and a per-call copy of the whole
/// string turned those loops quadratic. Callers that need an owned copy
/// call `.to_vec()` at their own site. The borrow is tied to the context's,
/// so one held across an allocation does not compile (#706).
fn get_string_as_chars<'a, 'gc>(
    cx: &'a Cx<'gc>,
    tv: Value<'gc>,
    fn_name: &str,
) -> Result<Ref<'a, [char]>, EvalError> {
    // Native heap string path
    if tv.is_string() {
        return Ok(cx.get_string_chars(tv));
    }

    // Not a string
    Err(EvalError::TypeError(format!(
        "{} expects a string",
        fn_name
    )))
}

/// Extract an integer from a value
fn get_integer<'gc>(cx: &Cx<'gc>, tv: Value<'gc>, fn_name: &str) -> Result<i64, EvalError> {
    if tv.is_fixnum() {
        return Ok(tv.as_fixnum_unchecked());
    }
    // BigInt on heap (convert to i64 if possible)
    if let Some(n) = cx.get_bigint(tv) {
        use num_traits::ToPrimitive;
        if let Some(i) = n.to_i64() {
            return Ok(i);
        }
        return Err(EvalError::TypeError(format!(
            "{}: integer too large",
            fn_name
        )));
    }

    Err(EvalError::TypeError(format!(
        "{} expects an integer",
        fn_name
    )))
}

/// Extract a character from a value
fn get_char(tv: Value<'_>, fn_name: &str) -> Result<char, EvalError> {
    if tv.is_char() {
        return Ok(tv.as_char_unchecked());
    }
    Err(EvalError::TypeError(format!(
        "{} expects a character",
        fn_name
    )))
}

/// Helper to convert Vec<char> to String for comparison/output
fn chars_to_string(chars: &[char]) -> String {
    chars.iter().collect()
}

pub(super) fn string_length<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let chars = get_string_as_chars(cx, args[0], "string-length")?;
    Ok(Value::fixnum(chars.len() as i64))
}

pub(super) fn string_ref<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let chars = get_string_as_chars(cx, args[0], "string-ref")?;
    let k = get_integer(cx, args[1], "string-ref")?;

    if k < 0 {
        return Err(EvalError::IndexOutOfBounds(format!(
            "string-ref index must be non-negative, got {}",
            k
        )));
    }

    let idx = k as usize;

    if idx >= chars.len() {
        return Err(EvalError::IndexOutOfBounds(format!(
            "string-ref index {} out of bounds for string of length {}",
            k,
            chars.len()
        )));
    }

    Ok(Value::character(chars[idx]))
}

pub(super) fn string_set<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

    // Extract index and char with immutable borrow
    let (k, ch) = {
        let k = get_integer(cx, args[1], "string-set!")?;
        let ch = get_char(args[2], "string-set!")?;
        (k, ch)
    };

    if k < 0 {
        return Err(EvalError::IndexOutOfBounds(format!(
            "string-set! index must be non-negative, got {}",
            k
        )));
    }

    let idx = k as usize;

    // Native heap string path
    if args[0].is_string() {
        let len = cx.string_char_len(args[0]);
        if idx >= len {
            return Err(EvalError::IndexOutOfBounds(format!(
                "string-set! index {} out of bounds for string of length {}",
                k, len
            )));
        }
        cx.string_set_char(args[0], idx, ch);
        return Ok(Value::UNSPECIFIED);
    }

    Err(EvalError::TypeError(
        "string-set! expects a string".to_string(),
    ))
}

pub(super) fn make_string<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 to 2".to_string(),
            actual: args.len(),
        });
    }

    let k = get_integer(cx, args[0], "make-string")?;

    if k < 0 {
        return Err(EvalError::TypeError(format!(
            "make-string length must be non-negative, got {}",
            k
        )));
    }

    let fill_char = if args.len() == 2 {
        get_char(args[1], "make-string")?
    } else {
        ' ' // Default fill character
    };

    let chars: Vec<char> = std::iter::repeat_n(fill_char, k as usize).collect();
    Ok(cx.alloc_string_chars(chars))
}

pub(super) fn string<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    let mut chars = Vec::new();

    for &arg in args {
        let ch = get_char(arg, "string")?;
        chars.push(ch);
    }

    Ok(cx.alloc_string_chars(chars))
}

pub(super) fn string_equal<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    // Get first string for comparison
    let first = get_string_as_chars(cx, args[0], "string=?")?;

    // Compare all remaining strings
    for arg in &args[1..] {
        let s = get_string_as_chars(cx, *arg, "string=?")?;
        if *s != *first {
            return Ok(Value::FALSE);
        }
    }

    Ok(Value::TRUE)
}

pub(super) fn string_less<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    string_compare(cx, args, |a, b| a < b, "string<?")
}

pub(super) fn string_greater<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    string_compare(cx, args, |a, b| a > b, "string>?")
}

pub(super) fn string_less_equal<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    string_compare(cx, args, |a, b| a <= b, "string<=?")
}

pub(super) fn string_greater_equal<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    string_compare(cx, args, |a, b| a >= b, "string>=?")
}

fn string_compare<'gc, F>(
    cx: &Cx<'gc>,
    args: &[Value<'gc>],
    cmp: F,
    fn_name: &str,
) -> Result<Value<'gc>, EvalError>
where
    F: Fn(&[char], &[char]) -> bool,
{
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    for i in 0..args.len() - 1 {
        let a = get_string_as_chars(cx, args[i], fn_name)?;
        let b = get_string_as_chars(cx, args[i + 1], fn_name)?;

        if !cmp(&a, &b) {
            return Ok(Value::FALSE);
        }
    }

    Ok(Value::TRUE)
}

pub(super) fn string_ci_equal<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    string_ci_compare(cx, args, |a, b| a == b, "string-ci=?")
}

pub(super) fn string_ci_less<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    string_ci_compare(cx, args, |a, b| a < b, "string-ci<?")
}

pub(super) fn string_ci_greater<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    string_ci_compare(cx, args, |a, b| a > b, "string-ci>?")
}

pub(super) fn string_ci_less_equal<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    string_ci_compare(cx, args, |a, b| a <= b, "string-ci<=?")
}

pub(super) fn string_ci_greater_equal<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    string_ci_compare(cx, args, |a, b| a >= b, "string-ci>=?")
}

fn string_ci_compare<'gc, F>(
    cx: &Cx<'gc>,
    args: &[Value<'gc>],
    cmp: F,
    fn_name: &str,
) -> Result<Value<'gc>, EvalError>
where
    F: Fn(&str, &str) -> bool,
{
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    for i in 0..args.len() - 1 {
        let a_chars = get_string_as_chars(cx, args[i], fn_name)?;
        let b_chars = get_string_as_chars(cx, args[i + 1], fn_name)?;

        // R7RS 6.7: string-ci=? and friends compare "as if by
        // string-foldcase" — full case folding, so "Straße" and "STRASSE"
        // are equal. Lower-casing left ß as ß and said they were not.
        use unicode_casefold::UnicodeCaseFold;
        let a_str: String = chars_to_string(&a_chars).case_fold().collect();
        let b_str: String = chars_to_string(&b_chars).case_fold().collect();

        if !cmp(&a_str, &b_str) {
            return Ok(Value::FALSE);
        }
    }

    Ok(Value::TRUE)
}

pub(super) fn string_append<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    let mut result: Vec<char> = Vec::new();

    for &arg in args {
        let chars = get_string_as_chars(cx, arg, "string-append")?;
        result.extend(chars.iter());
    }

    Ok(cx.alloc_string_chars(result))
}

pub(super) fn substring<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

    let chars = get_string_as_chars(cx, args[0], "substring")?.to_vec();
    let start = get_integer(cx, args[1], "substring")?;
    let end = get_integer(cx, args[2], "substring")?;

    if start < 0 || end < 0 {
        return Err(EvalError::IndexOutOfBounds(
            "substring indices must be non-negative".to_string(),
        ));
    }

    if start > end {
        return Err(EvalError::IndexOutOfBounds(
            "substring start index must be <= end index".to_string(),
        ));
    }

    let start_idx = start as usize;
    let end_idx = end as usize;

    if end_idx > chars.len() {
        return Err(EvalError::IndexOutOfBounds(format!(
            "substring end index {} out of bounds for string of length {}",
            end,
            chars.len()
        )));
    }

    let substr: Vec<char> = chars[start_idx..end_idx].to_vec();
    Ok(cx.alloc_string_chars(substr))
}

pub(super) fn string_to_list<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    // Extract chars and indices
    let (chars, start, end) = {
        let chars = get_string_as_chars(cx, args[0], "string->list")?.to_vec();

        let start = if args.len() >= 2 {
            get_integer(cx, args[1], "string->list")? as usize
        } else {
            0
        };

        let end = if args.len() >= 3 {
            get_integer(cx, args[2], "string->list")? as usize
        } else {
            chars.len()
        };

        if start > end || end > chars.len() {
            return Err(EvalError::IndexOutOfBounds(
                "string->list indices out of bounds".to_string(),
            ));
        }

        (chars, start, end)
    };

    Ok(cx.list_from_iter(chars[start..end].iter().map(|&ch| Value::character(ch))))
}

pub(super) fn list_to_string<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let elements = cx
        .list_to_vec(args[0])
        .ok_or_else(|| EvalError::TypeError("list->string expects a proper list".into()))?;
    let mut chars = Vec::with_capacity(elements.len());
    for tv in elements {
        if !tv.is_char() {
            return Err(EvalError::TypeError(
                "list->string expects a list of characters".into(),
            ));
        }
        chars.push(tv.as_char_unchecked());
    }
    Ok(cx.alloc_string_chars(chars))
}

pub(super) fn string_copy<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    let chars = get_string_as_chars(cx, args[0], "string-copy")?.to_vec();

    if args.len() == 1 {
        // Simple copy
        return Ok(cx.alloc_string_chars(chars));
    }

    // Copy substring
    let start = get_integer(cx, args[1], "string-copy")? as usize;

    let end = if args.len() >= 3 {
        get_integer(cx, args[2], "string-copy")? as usize
    } else {
        chars.len()
    };

    if start > end || end > chars.len() {
        return Err(EvalError::IndexOutOfBounds(
            "string-copy indices out of bounds".to_string(),
        ));
    }

    let substr: Vec<char> = chars[start..end].to_vec();
    Ok(cx.alloc_string_chars(substr))
}

// ========== String Case Conversion ==========

/// (string-upcase string) - Convert string to uppercase
pub(super) fn string_upcase<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let chars = get_string_as_chars(cx, args[0], "string-upcase")?.to_vec();

    // Convert to String, uppercase, then back to Vec<char>
    let upper: Vec<char> = chars_to_string(&chars).to_uppercase().chars().collect();
    Ok(cx.alloc_string_chars(upper))
}

/// (string-downcase string) - Convert string to lowercase
pub(super) fn string_downcase<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let chars = get_string_as_chars(cx, args[0], "string-downcase")?.to_vec();

    // Convert to String, lowercase, then back to Vec<char>
    let lower: Vec<char> = chars_to_string(&chars).to_lowercase().chars().collect();
    Ok(cx.alloc_string_chars(lower))
}

/// (string-foldcase string) - Case-fold string for case-insensitive comparison
///
/// R7RS requires full Unicode case folding, which differs from simple lowercasing:
/// - ß (sharp s) folds to "ss"
/// - ſ (long s) folds to "s"
/// - All forms of sigma (Σ, σ, ς) fold to σ
pub(super) fn string_foldcase<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    use unicode_casefold::UnicodeCaseFold;

    let chars = get_string_as_chars(cx, args[0], "string-foldcase")?.to_vec();

    let folded: Vec<char> = chars_to_string(&chars).case_fold().collect();
    Ok(cx.alloc_string_chars(folded))
}

// ========== String Mutation ==========

/// (string-fill! string fill [start [end]]) - Fill string with character
pub(super) fn string_fill<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "2 to 4".to_string(),
            actual: args.len(),
        });
    }

    // Extract fill_char and indices with immutable borrow
    let (fill_char, start, end_arg) = {
        let fill_char = get_char(args[1], "string-fill!")?;
        let start = if args.len() >= 3 {
            let n = get_integer(cx, args[2], "string-fill!")?;
            if n < 0 {
                return Err(EvalError::TypeError(
                    "string-fill! start index must be non-negative".to_string(),
                ));
            }
            n as usize
        } else {
            0
        };
        let end_arg = if args.len() >= 4 {
            let n = get_integer(cx, args[3], "string-fill!")?;
            if n < 0 {
                return Err(EvalError::TypeError(
                    "string-fill! end index must be non-negative".to_string(),
                ));
            }
            Some(n as usize)
        } else {
            None
        };
        (fill_char, start, end_arg)
    };

    // Native heap string path
    if args[0].is_string() {
        let len = cx.string_char_len(args[0]);
        let end = end_arg.unwrap_or(len);

        if start > end || end > len {
            return Err(EvalError::IndexOutOfBounds(
                "string-fill! indices out of bounds".to_string(),
            ));
        }

        cx.string_fill(args[0], fill_char, start..end);
        return Ok(Value::UNSPECIFIED);
    }

    Err(EvalError::TypeError(
        "string-fill! expects a string".to_string(),
    ))
}

/// (string-copy! to at from [start [end]]) - Copy characters from one string to another
pub(super) fn string_copy_mutate<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 3 || args.len() > 5 {
        return Err(EvalError::WrongArity {
            expected: "3 to 5".to_string(),
            actual: args.len(),
        });
    }

    // Extract at, source chars, start, end with immutable borrow
    let (at, source_chars, start, end) = {
        let at = get_integer(cx, args[1], "string-copy!")?;
        if at < 0 {
            return Err(EvalError::TypeError(
                "string-copy! at index must be non-negative".to_string(),
            ));
        }
        let at = at as usize;

        let from = get_string_as_chars(cx, args[2], "string-copy!")?;
        let from_len = from.len();

        let start = if args.len() >= 4 {
            let n = get_integer(cx, args[3], "string-copy!")?;
            if n < 0 {
                return Err(EvalError::TypeError(
                    "string-copy! start index must be non-negative".to_string(),
                ));
            }
            n as usize
        } else {
            0
        };

        let end = if args.len() >= 5 {
            let n = get_integer(cx, args[4], "string-copy!")?;
            if n < 0 {
                return Err(EvalError::TypeError(
                    "string-copy! end index must be non-negative".to_string(),
                ));
            }
            n as usize
        } else {
            from_len
        };

        if start > end || end > from_len {
            return Err(EvalError::IndexOutOfBounds(
                "string-copy! source indices out of bounds".to_string(),
            ));
        }

        (at, from[start..end].to_vec(), start, end)
    };

    let copy_len = end - start;

    // Native heap string path
    if args[0].is_string() {
        if at + copy_len > cx.string_char_len(args[0]) {
            return Err(EvalError::IndexOutOfBounds(
                "string-copy! destination index out of bounds".to_string(),
            ));
        }
        cx.string_copy_chars(args[0], at, &source_chars);
        return Ok(Value::UNSPECIFIED);
    }

    Err(EvalError::TypeError(
        "string-copy! expects a string".to_string(),
    ))
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // String length
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-length",
        Arity::Exact(1),
        "Returns the number of characters in string.",
        string_length,
    ));

    // String ref
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-ref",
        Arity::Exact(2),
        "Returns character k of string.",
        string_ref,
    ));

    // String set!
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-set!",
        Arity::Exact(3),
        "Stores char in element k of string.",
        string_set,
    ));

    // Make-string
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "make-string",
        Arity::Range(1, 2),
        "Returns a newly allocated string of length k.",
        make_string,
    ));

    // String constructor
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string",
        Arity::Min(0),
        "Returns a newly allocated string composed of the arguments.",
        string,
    ));

    // String equality
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string=?",
        Arity::Min(2),
        "Returns #t if all strings are equal.",
        string_equal,
    ));

    // String less than
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string<?",
        Arity::Min(2),
        "Returns #t if strings are monotonically increasing.",
        string_less,
    ));

    // String greater than
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string>?",
        Arity::Min(2),
        "Returns #t if strings are monotonically decreasing.",
        string_greater,
    ));

    // String less than or equal
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string<=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-decreasing.",
        string_less_equal,
    ));

    // String greater than or equal
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string>=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-increasing.",
        string_greater_equal,
    ));

    // String case-insensitive equality
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-ci=?",
        Arity::Min(2),
        "Returns #t if all strings are equal (case-insensitive).",
        string_ci_equal,
    ));

    // String case-insensitive less than
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-ci<?",
        Arity::Min(2),
        "Returns #t if strings are monotonically increasing (case-insensitive).",
        string_ci_less,
    ));

    // String case-insensitive greater than
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-ci>?",
        Arity::Min(2),
        "Returns #t if strings are monotonically decreasing (case-insensitive).",
        string_ci_greater,
    ));

    // String case-insensitive less than or equal
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-ci<=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-decreasing (case-insensitive).",
        string_ci_less_equal,
    ));

    // String case-insensitive greater than or equal
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-ci>=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-increasing (case-insensitive).",
        string_ci_greater_equal,
    ));

    // String append
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-append",
        Arity::Min(0),
        "Returns a newly allocated string whose characters are the concatenation of the given strings.",
        string_append,
    ));

    // Substring
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "substring",
        Arity::Exact(3),
        "Returns a newly allocated string formed from the characters of string beginning with index start and ending with index end.",
        substring,
    ));

    // String->list
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string->list",
        Arity::Range(1, 3),
        "Returns a newly allocated list of the characters of string.",
        string_to_list,
    ));

    // List->string
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "list->string",
        Arity::Exact(1),
        "Returns a newly allocated string formed from the characters in the list.",
        list_to_string,
    ));

    // String-copy
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-copy",
        Arity::Range(1, 3),
        "Returns a newly allocated copy of the part of the given string.",
        string_copy,
    ));

    // String case conversion (scheme.char library)
    registry.register(PrimitiveFn::new_cx(
        "scheme.char",
        "string-upcase",
        Arity::Exact(1),
        "Returns a newly allocated string with all characters converted to uppercase.",
        string_upcase,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.char",
        "string-downcase",
        Arity::Exact(1),
        "Returns a newly allocated string with all characters converted to lowercase.",
        string_downcase,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.char",
        "string-foldcase",
        Arity::Exact(1),
        "Returns a newly allocated string with all characters case-folded.",
        string_foldcase,
    ));

    // String mutation (scheme.base)
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-fill!",
        Arity::Range(2, 4),
        "Stores fill character in every element of string between start and end.",
        string_fill,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string-copy!",
        Arity::Range(3, 5),
        "Copies characters from source string to destination string.",
        string_copy_mutate,
    ));
}
