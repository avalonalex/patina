//! String primitive operations with O(1) character access (R7RS Section 6.7)
//!
//! Implements string operations using Vec<char> for O(1) random access:
//! - Character indexing is O(1)
//! - Mutation via string-set! is O(1)
//! - Comparison operations (case-sensitive and case-insensitive)
//!
//! Note: UTF-8 conversion happens at I/O boundaries.
//!
//! Total: 20 string primitives + 2 helper functions

use crate::apply_context::ApplyContext;
use patina_core::TaggedValue;
use patina_runtime::EvalError;

// ========== TaggedValue Extraction Helpers ==========

/// Get string as Vec<char> for read operations
fn get_string_as_chars(
    tv: TaggedValue,
    heap: &std::cell::Ref<'_, patina_core::Heap>,
    fn_name: &str,
) -> Result<Vec<char>, EvalError> {
    // Native heap string path
    if tv.is_string() {
        return Ok(heap.get_string_chars(tv).to_vec());
    }

    // Not a string
    Err(EvalError::TypeError(format!(
        "{} expects a string",
        fn_name
    )))
}

/// Extract an integer from a TaggedValue
fn get_integer(
    tv: TaggedValue,
    heap: &std::cell::Ref<'_, patina_core::Heap>,
    fn_name: &str,
) -> Result<i64, EvalError> {
    if tv.is_fixnum() {
        return Ok(tv.as_fixnum_unchecked());
    }
    // BigInt on heap (convert to i64 if possible)
    if let Some(n) = heap.get_bigint(tv) {
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
        "{} expects a character",
        fn_name
    )))
}

/// Helper to convert Vec<char> to String for comparison/output
fn chars_to_string(chars: &[char]) -> String {
    chars.iter().collect()
}

pub(super) fn string_length(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();
    let chars = get_string_as_chars(args[0], &heap_ref, "string-length")?;
    Ok(TaggedValue::fixnum(chars.len() as i64))
}

pub(super) fn string_ref(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();

    let chars = get_string_as_chars(args[0], &heap_ref, "string-ref")?;
    let k = get_integer(args[1], &heap_ref, "string-ref")?;

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

    Ok(TaggedValue::character(chars[idx]))
}

pub(super) fn string_set(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

    // Extract index and char with immutable borrow
    let (k, ch) = {
        let heap_ref = heap.borrow();
        let k = get_integer(args[1], &heap_ref, "string-set!")?;
        let ch = get_char(args[2], &heap_ref, "string-set!")?;
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
        let mut heap_mut = heap.borrow_mut();
        let len = heap_mut.string_char_len(args[0]);
        if idx >= len {
            return Err(EvalError::IndexOutOfBounds(format!(
                "string-set! index {} out of bounds for string of length {}",
                k, len
            )));
        }
        heap_mut.string_set_char(args[0], idx, ch);
        return Ok(TaggedValue::UNSPECIFIED);
    }

    Err(EvalError::TypeError(
        "string-set! expects a string".to_string(),
    ))
}

pub(super) fn make_string(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 to 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();

    let k = get_integer(args[0], &heap_ref, "make-string")?;

    if k < 0 {
        return Err(EvalError::TypeError(format!(
            "make-string length must be non-negative, got {}",
            k
        )));
    }

    let fill_char = if args.len() == 2 {
        get_char(args[1], &heap_ref, "make-string")?
    } else {
        ' ' // Default fill character
    };

    drop(heap_ref);
    let chars: Vec<char> = std::iter::repeat_n(fill_char, k as usize).collect();
    Ok(heap.borrow_mut().alloc_string_chars(chars))
}

pub(super) fn string(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let heap = ctx.heap();
    let heap_ref = heap.borrow();

    let mut chars = Vec::new();

    for arg in args {
        let ch = get_char(arg, &heap_ref, "string")?;
        chars.push(ch);
    }

    drop(heap_ref);
    Ok(heap.borrow_mut().alloc_string_chars(chars))
}

pub(super) fn string_equal(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();

    // Get first string for comparison
    let first = get_string_as_chars(args[0], &heap_ref, "string=?")?;

    // Compare all remaining strings
    for arg in &args[1..] {
        let s = get_string_as_chars(*arg, &heap_ref, "string=?")?;
        if s != first {
            return Ok(TaggedValue::FALSE);
        }
    }

    Ok(TaggedValue::TRUE)
}

pub(super) fn string_less(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    string_compare(ctx, args, |a, b| a < b, "string<?")
}

pub(super) fn string_greater(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    string_compare(ctx, args, |a, b| a > b, "string>?")
}

pub(super) fn string_less_equal(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    string_compare(ctx, args, |a, b| a <= b, "string<=?")
}

pub(super) fn string_greater_equal(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    string_compare(ctx, args, |a, b| a >= b, "string>=?")
}

fn string_compare<F>(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
    cmp: F,
    fn_name: &str,
) -> Result<TaggedValue, EvalError>
where
    F: Fn(&[char], &[char]) -> bool,
{
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let a = get_string_as_chars(args[i], &heap_ref, fn_name)?;
        let b = get_string_as_chars(args[i + 1], &heap_ref, fn_name)?;

        if !cmp(&a, &b) {
            return Ok(TaggedValue::FALSE);
        }
    }

    Ok(TaggedValue::TRUE)
}

pub(super) fn string_ci_equal(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    string_ci_compare(ctx, args, |a, b| a == b, "string-ci=?")
}

pub(super) fn string_ci_less(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    string_ci_compare(ctx, args, |a, b| a < b, "string-ci<?")
}

pub(super) fn string_ci_greater(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    string_ci_compare(ctx, args, |a, b| a > b, "string-ci>?")
}

pub(super) fn string_ci_less_equal(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    string_ci_compare(ctx, args, |a, b| a <= b, "string-ci<=?")
}

pub(super) fn string_ci_greater_equal(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    string_ci_compare(ctx, args, |a, b| a >= b, "string-ci>=?")
}

fn string_ci_compare<F>(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
    cmp: F,
    fn_name: &str,
) -> Result<TaggedValue, EvalError>
where
    F: Fn(&str, &str) -> bool,
{
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();

    for i in 0..args.len() - 1 {
        let a_chars = get_string_as_chars(args[i], &heap_ref, fn_name)?;
        let b_chars = get_string_as_chars(args[i + 1], &heap_ref, fn_name)?;

        // Convert Vec<char> to String for case-insensitive comparison
        let a_str = chars_to_string(&a_chars).to_lowercase();
        let b_str = chars_to_string(&b_chars).to_lowercase();

        if !cmp(&a_str, &b_str) {
            return Ok(TaggedValue::FALSE);
        }
    }

    Ok(TaggedValue::TRUE)
}

pub(super) fn string_append(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let heap = ctx.heap();
    let heap_ref = heap.borrow();

    let mut result: Vec<char> = Vec::new();

    for arg in args {
        let chars = get_string_as_chars(arg, &heap_ref, "string-append")?;
        result.extend(chars.iter());
    }

    drop(heap_ref);
    Ok(heap.borrow_mut().alloc_string_chars(result))
}

pub(super) fn substring(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();

    let chars = get_string_as_chars(args[0], &heap_ref, "substring")?;
    let start = get_integer(args[1], &heap_ref, "substring")?;
    let end = get_integer(args[2], &heap_ref, "substring")?;
    drop(heap_ref);

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
    Ok(heap.borrow_mut().alloc_string_chars(substr))
}

pub(super) fn string_to_list(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

    // Extract chars and indices
    let (chars, start, end) = {
        let heap_ref = heap.borrow();
        let chars = get_string_as_chars(args[0], &heap_ref, "string->list")?;

        let start = if args.len() >= 2 {
            get_integer(args[1], &heap_ref, "string->list")? as usize
        } else {
            0
        };

        let end = if args.len() >= 3 {
            get_integer(args[2], &heap_ref, "string->list")? as usize
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

    // Build list directly using TaggedValue::character and heap's list_from_iter
    let char_tvs: Vec<TaggedValue> = chars[start..end]
        .iter()
        .map(|&ch| TaggedValue::character(ch))
        .collect();

    Ok(heap.borrow_mut().list_from_iter(char_tvs))
}

pub(super) fn list_to_string(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

    // Try fast path using heap's list_to_vec
    let chars_opt = {
        let heap_ref = heap.borrow();
        heap_ref.list_to_vec(args[0])
    };

    let result: Vec<char> = if let Some(char_tvs) = chars_opt {
        // Fast path: native heap list - chars are already extracted as TaggedValues
        let mut chars = Vec::with_capacity(char_tvs.len());
        for tv in char_tvs {
            if tv.is_char() {
                chars.push(tv.as_char_unchecked());
            } else {
                return Err(EvalError::TypeError(
                    "list->string expects a list of characters".to_string(),
                ));
            }
        }
        chars
    } else {
        // Slow path: walk the list manually
        let heap_ref = heap.borrow();
        let mut chars = Vec::new();
        let mut current = args[0];
        while !current.is_null() {
            if !current.is_pair() {
                return Err(EvalError::TypeError(
                    "list->string expects a proper list".to_string(),
                ));
            }
            let car = heap_ref.car(current);
            if car.is_char() {
                chars.push(car.as_char_unchecked());
            } else {
                return Err(EvalError::TypeError(
                    "list->string expects a list of characters".to_string(),
                ));
            }
            current = heap_ref.cdr(current);
        }
        chars
    };

    Ok(heap.borrow_mut().alloc_string_chars(result))
}

pub(super) fn string_copy(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();

    let chars = get_string_as_chars(args[0], &heap_ref, "string-copy")?;

    if args.len() == 1 {
        // Simple copy
        drop(heap_ref);
        return Ok(heap.borrow_mut().alloc_string_chars(chars));
    }

    // Copy substring
    let start = get_integer(args[1], &heap_ref, "string-copy")? as usize;

    let end = if args.len() >= 3 {
        get_integer(args[2], &heap_ref, "string-copy")? as usize
    } else {
        chars.len()
    };

    if start > end || end > chars.len() {
        return Err(EvalError::IndexOutOfBounds(
            "string-copy indices out of bounds".to_string(),
        ));
    }

    let substr: Vec<char> = chars[start..end].to_vec();
    drop(heap_ref);
    Ok(heap.borrow_mut().alloc_string_chars(substr))
}

// ========== String Case Conversion ==========

/// (string-upcase string) - Convert string to uppercase
pub(super) fn string_upcase(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();
    let chars = get_string_as_chars(args[0], &heap_ref, "string-upcase")?;
    drop(heap_ref);

    // Convert to String, uppercase, then back to Vec<char>
    let upper: Vec<char> = chars_to_string(&chars).to_uppercase().chars().collect();
    Ok(heap.borrow_mut().alloc_string_chars(upper))
}

/// (string-downcase string) - Convert string to lowercase
pub(super) fn string_downcase(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let heap_ref = heap.borrow();
    let chars = get_string_as_chars(args[0], &heap_ref, "string-downcase")?;
    drop(heap_ref);

    // Convert to String, lowercase, then back to Vec<char>
    let lower: Vec<char> = chars_to_string(&chars).to_lowercase().chars().collect();
    Ok(heap.borrow_mut().alloc_string_chars(lower))
}

/// (string-foldcase string) - Case-fold string for case-insensitive comparison
///
/// R7RS requires full Unicode case folding, which differs from simple lowercasing:
/// - ß (sharp s) folds to "ss"
/// - ſ (long s) folds to "s"
/// - All forms of sigma (Σ, σ, ς) fold to σ
pub(super) fn string_foldcase(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    use unicode_casefold::UnicodeCaseFold;

    let heap = ctx.heap();
    let heap_ref = heap.borrow();
    let chars = get_string_as_chars(args[0], &heap_ref, "string-foldcase")?;
    drop(heap_ref);

    let folded: Vec<char> = chars_to_string(&chars).case_fold().collect();
    Ok(heap.borrow_mut().alloc_string_chars(folded))
}

// ========== String Mutation ==========

/// (string-fill! string fill [start [end]]) - Fill string with character
pub(super) fn string_fill(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "2 to 4".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

    // Extract fill_char and indices with immutable borrow
    let (fill_char, start, end_arg) = {
        let heap_ref = heap.borrow();
        let fill_char = get_char(args[1], &heap_ref, "string-fill!")?;
        let start = if args.len() >= 3 {
            let n = get_integer(args[2], &heap_ref, "string-fill!")?;
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
            let n = get_integer(args[3], &heap_ref, "string-fill!")?;
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
        let mut heap_mut = heap.borrow_mut();
        let chars = heap_mut.get_string_chars_mut(args[0]);
        let len = chars.len();
        let end = end_arg.unwrap_or(len);

        if start > end || end > len {
            return Err(EvalError::IndexOutOfBounds(
                "string-fill! indices out of bounds".to_string(),
            ));
        }

        chars[start..end].fill(fill_char);
        return Ok(TaggedValue::UNSPECIFIED);
    }

    Err(EvalError::TypeError(
        "string-fill! expects a string".to_string(),
    ))
}

// ========== String Higher-Order Functions ==========

/// (string-map proc string1 string2 ...) - Apply proc to each character
pub(super) fn string_map(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let proc = args[0]; // already TaggedValue

    // Extract all strings
    let strings: Vec<Vec<char>> = {
        let heap_ref = heap.borrow();
        args[1..]
            .iter()
            .map(|&tv| get_string_as_chars(tv, &heap_ref, "string-map"))
            .collect::<Result<_, _>>()?
    };

    if strings.is_empty() {
        return Ok(heap.borrow_mut().alloc_string_chars(Vec::new()));
    }

    let min_len = strings.iter().map(|s| s.len()).min().unwrap_or(0);
    let mut result: Vec<char> = Vec::with_capacity(min_len);

    for i in 0..min_len {
        // Characters are immediate TaggedValues — no heap allocation needed
        let proc_args: Vec<TaggedValue> = strings
            .iter()
            .map(|s| TaggedValue::character(s[i]))
            .collect();
        // Procedure calls within string-map are not in tail position
        let result_tv = ctx.apply_proc(proc, proc_args)?;
        match result_tv.as_char() {
            Some(c) => result.push(c),
            None => {
                return Err(EvalError::TypeError(
                    "string-map: procedure must return a character".to_string(),
                ));
            }
        }
    }

    Ok(heap.borrow_mut().alloc_string_chars(result))
}

/// (string-for-each proc string1 string2 ...) - Apply proc for side effects
pub(super) fn string_for_each(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    let proc = args[0]; // already TaggedValue

    // Extract all strings
    let strings: Vec<Vec<char>> = {
        let heap_ref = heap.borrow();
        args[1..]
            .iter()
            .map(|&tv| get_string_as_chars(tv, &heap_ref, "string-for-each"))
            .collect::<Result<_, _>>()?
    };

    if strings.is_empty() {
        return Ok(TaggedValue::UNSPECIFIED);
    }

    let min_len = strings.iter().map(|s| s.len()).min().unwrap_or(0);

    // string-for-each is guaranteed to call proc in order
    for i in 0..min_len {
        // Characters are immediate TaggedValues — no heap allocation needed
        let proc_args: Vec<TaggedValue> = strings
            .iter()
            .map(|s| TaggedValue::character(s[i]))
            .collect();
        // Procedure calls within string-for-each are not in tail position
        ctx.apply_proc(proc, proc_args)?;
    }

    Ok(TaggedValue::UNSPECIFIED)
}

/// (string-copy! to at from [start [end]]) - Copy characters from one string to another
pub(super) fn string_copy_mutate(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 3 || args.len() > 5 {
        return Err(EvalError::WrongArity {
            expected: "3 to 5".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

    // Extract at, source chars, start, end with immutable borrow
    let (at, source_chars, start, end) = {
        let heap_ref = heap.borrow();

        let at = get_integer(args[1], &heap_ref, "string-copy!")?;
        if at < 0 {
            return Err(EvalError::TypeError(
                "string-copy! at index must be non-negative".to_string(),
            ));
        }
        let at = at as usize;

        let from = get_string_as_chars(args[2], &heap_ref, "string-copy!")?;
        let from_len = from.len();

        let start = if args.len() >= 4 {
            let n = get_integer(args[3], &heap_ref, "string-copy!")?;
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
            let n = get_integer(args[4], &heap_ref, "string-copy!")?;
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
        let mut heap_mut = heap.borrow_mut();
        let to_chars = heap_mut.get_string_chars_mut(args[0]);
        if at + copy_len > to_chars.len() {
            return Err(EvalError::IndexOutOfBounds(
                "string-copy! destination index out of bounds".to_string(),
            ));
        }
        to_chars[at..at + copy_len].copy_from_slice(&source_chars);
        return Ok(TaggedValue::UNSPECIFIED);
    }

    Err(EvalError::TypeError(
        "string-copy! expects a string".to_string(),
    ))
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // String length
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-length",
        Arity::Exact(1),
        "Returns the number of characters in string.",
        |ctx, args| string_length(ctx, args),
    ));

    // String ref
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-ref",
        Arity::Exact(2),
        "Returns character k of string.",
        |ctx, args| string_ref(ctx, args),
    ));

    // String set!
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-set!",
        Arity::Exact(3),
        "Stores char in element k of string.",
        |ctx, args| string_set(ctx, args),
    ));

    // Make-string
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "make-string",
        Arity::Range(1, 2),
        "Returns a newly allocated string of length k.",
        |ctx, args| make_string(ctx, args),
    ));

    // String constructor
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string",
        Arity::Min(0),
        "Returns a newly allocated string composed of the arguments.",
        |ctx, args| string(ctx, args),
    ));

    // String equality
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string=?",
        Arity::Min(2),
        "Returns #t if all strings are equal.",
        |ctx, args| string_equal(ctx, args),
    ));

    // String less than
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string<?",
        Arity::Min(2),
        "Returns #t if strings are monotonically increasing.",
        |ctx, args| string_less(ctx, args),
    ));

    // String greater than
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string>?",
        Arity::Min(2),
        "Returns #t if strings are monotonically decreasing.",
        |ctx, args| string_greater(ctx, args),
    ));

    // String less than or equal
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string<=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-decreasing.",
        |ctx, args| string_less_equal(ctx, args),
    ));

    // String greater than or equal
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string>=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-increasing.",
        |ctx, args| string_greater_equal(ctx, args),
    ));

    // String case-insensitive equality
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-ci=?",
        Arity::Min(2),
        "Returns #t if all strings are equal (case-insensitive).",
        |ctx, args| string_ci_equal(ctx, args),
    ));

    // String case-insensitive less than
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-ci<?",
        Arity::Min(2),
        "Returns #t if strings are monotonically increasing (case-insensitive).",
        |ctx, args| string_ci_less(ctx, args),
    ));

    // String case-insensitive greater than
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-ci>?",
        Arity::Min(2),
        "Returns #t if strings are monotonically decreasing (case-insensitive).",
        |ctx, args| string_ci_greater(ctx, args),
    ));

    // String case-insensitive less than or equal
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-ci<=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-decreasing (case-insensitive).",
        |ctx, args| string_ci_less_equal(ctx, args),
    ));

    // String case-insensitive greater than or equal
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-ci>=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-increasing (case-insensitive).",
        |ctx, args| string_ci_greater_equal(ctx, args),
    ));

    // String append
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-append",
        Arity::Min(0),
        "Returns a newly allocated string whose characters are the concatenation of the given strings.",
        |ctx, args| string_append(ctx, args),
    ));

    // Substring
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "substring",
        Arity::Exact(3),
        "Returns a newly allocated string formed from the characters of string beginning with index start and ending with index end.",
        |ctx, args| substring(ctx, args),
    ));

    // String->list
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string->list",
        Arity::Range(1, 3),
        "Returns a newly allocated list of the characters of string.",
        |ctx, args| string_to_list(ctx, args),
    ));

    // List->string
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "list->string",
        Arity::Exact(1),
        "Returns a newly allocated string formed from the characters in the list.",
        |ctx, args| list_to_string(ctx, args),
    ));

    // String-copy
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-copy",
        Arity::Range(1, 3),
        "Returns a newly allocated copy of the part of the given string.",
        |ctx, args| string_copy(ctx, args),
    ));

    // String case conversion (scheme.char library)
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.char",
        "string-upcase",
        Arity::Exact(1),
        "Returns a newly allocated string with all characters converted to uppercase.",
        |ctx, args| string_upcase(ctx, args),
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.char",
        "string-downcase",
        Arity::Exact(1),
        "Returns a newly allocated string with all characters converted to lowercase.",
        |ctx, args| string_downcase(ctx, args),
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.char",
        "string-foldcase",
        Arity::Exact(1),
        "Returns a newly allocated string with all characters case-folded.",
        |ctx, args| string_foldcase(ctx, args),
    ));

    // String mutation (scheme.base)
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-fill!",
        Arity::Range(2, 4),
        "Stores fill character in every element of string between start and end.",
        |ctx, args| string_fill(ctx, args),
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-copy!",
        Arity::Range(3, 5),
        "Copies characters from source string to destination string.",
        |ctx, args| string_copy_mutate(ctx, args),
    ));

    // String higher-order functions (scheme.base)
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-map",
        Arity::Min(2),
        "Applies proc element-wise to strings and returns a string of the results.",
        |ctx, args| string_map(ctx, args),
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "string-for-each",
        Arity::Min(2),
        "Applies proc to each element of the strings for its side effects.",
        |ctx, args| string_for_each(ctx, args),
    ));
}
