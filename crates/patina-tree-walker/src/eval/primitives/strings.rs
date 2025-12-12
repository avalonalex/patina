//! String primitive operations with UTF-8 support (R7RS Section 6.7)
//!
//! Implements string operations with full Unicode support:
//! - Character-based indexing (O(n) as allowed by R7RS)
//! - Mutable strings via string-set!
//! - Comparison operations (case-sensitive and case-insensitive)
//!
//! Total: 20 string primitives + 2 helper functions

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn string_length(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "string-length")?;

    match &args[0] {
        Value::String(s) => {
            // Count Unicode characters, not bytes
            let len = s.borrow().chars().count();
            Ok(Value::Integer(len as i64))
        }
        _ => Err(EvalError::TypeError(format!(
            "string-length expects a string, got {}",
            args[0].type_name()
        ))),
    }
}

pub(super) fn string_ref(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "string-ref")?;

    let (s, k) = match (&args[0], &args[1]) {
        (Value::String(s), Value::Integer(k)) => (s, *k),
        (Value::String(_), _) => {
            return Err(EvalError::TypeError(
                "string-ref index must be an integer".to_string(),
            ));
        }
        _ => {
            return Err(EvalError::TypeError(format!(
                "string-ref expects string and integer, got {} and {}",
                args[0].type_name(),
                args[1].type_name()
            )));
        }
    };

    if k < 0 {
        return Err(EvalError::IndexOutOfBounds(format!(
            "string-ref index must be non-negative, got {}",
            k
        )));
    }

    // Get nth character (O(n) - R7RS compliant)
    let ch = s.borrow().chars().nth(k as usize).ok_or_else(|| {
        EvalError::IndexOutOfBounds(format!(
            "string-ref index {} out of bounds for string of length {}",
            k,
            s.borrow().chars().count()
        ))
    })?;

    Ok(Value::Character(ch))
}

pub(super) fn string_set(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 3, "string-set!")?;

    let (s, k, ch) = match (&args[0], &args[1], &args[2]) {
        (Value::String(s), Value::Integer(k), Value::Character(ch)) => (s, *k, *ch),
        (Value::String(_), Value::Integer(_), _) => {
            return Err(EvalError::TypeError(
                "string-set! third argument must be a character".to_string(),
            ));
        }
        (Value::String(_), _, _) => {
            return Err(EvalError::TypeError(
                "string-set! index must be an integer".to_string(),
            ));
        }
        _ => {
            return Err(EvalError::TypeError(
                "string-set! expects string, integer, and character".to_string(),
            ));
        }
    };

    if k < 0 {
        return Err(EvalError::IndexOutOfBounds(format!(
            "string-set! index must be non-negative, got {}",
            k
        )));
    }

    // Convert to Vec<char>, mutate, convert back
    // This is O(n) but works correctly with UTF-8
    let mut chars: Vec<char> = s.borrow().chars().collect();

    if k as usize >= chars.len() {
        return Err(EvalError::IndexOutOfBounds(format!(
            "string-set! index {} out of bounds for string of length {}",
            k,
            chars.len()
        )));
    }

    chars[k as usize] = ch;
    *s.borrow_mut() = chars.into_iter().collect();

    Ok(Value::Unspecified)
}

pub(super) fn make_string(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 1, 2, "make-string")?;

    let k = match &args[0] {
        Value::Integer(k) => *k,
        _ => {
            return Err(EvalError::TypeError(
                "make-string length must be an integer".to_string(),
            ));
        }
    };

    if k < 0 {
        return Err(EvalError::TypeError(format!(
            "make-string length must be non-negative, got {}",
            k
        )));
    }

    let fill_char = if args.len() == 2 {
        match &args[1] {
            Value::Character(ch) => *ch,
            _ => {
                return Err(EvalError::TypeError(
                    "make-string fill character must be a character".to_string(),
                ));
            }
        }
    } else {
        ' ' // Default fill character
    };

    let s = std::iter::repeat_n(fill_char, k as usize).collect::<String>();
    Ok(Value::String(Rc::new(RefCell::new(s))))
}

pub(super) fn string(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    let mut chars = Vec::new();

    for arg in args {
        match arg {
            Value::Character(ch) => chars.push(ch),
            _ => {
                return Err(EvalError::TypeError(format!(
                    "string expects characters, got {}",
                    arg.type_name()
                )));
            }
        }
    }

    let s = chars.into_iter().collect::<String>();
    Ok(Value::String(Rc::new(RefCell::new(s))))
}

pub(super) fn string_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "string=?")?;

    // Get first string for comparison
    let first = match &args[0] {
        Value::String(s) => s.borrow().clone(),
        _ => return Err(EvalError::TypeError("string=? expects strings".to_string())),
    };

    // Compare all remaining strings
    for arg in &args[1..] {
        match arg {
            Value::String(s) => {
                if *s.borrow() != first {
                    return Ok(Value::Boolean(false));
                }
            }
            _ => return Err(EvalError::TypeError("string=? expects strings".to_string())),
        }
    }

    Ok(Value::Boolean(true))
}

pub(super) fn string_less(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    string_compare(evaluator, args, |a, b| a < b, "string<?")
}

pub(super) fn string_greater(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    string_compare(evaluator, args, |a, b| a > b, "string>?")
}

pub(super) fn string_less_equal(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    string_compare(evaluator, args, |a, b| a <= b, "string<=?")
}

pub(super) fn string_greater_equal(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    string_compare(evaluator, args, |a, b| a >= b, "string>=?")
}

fn string_compare<F>(
    evaluator: &Evaluator,
    args: Vec<Value>,
    cmp: F,
    fn_name: &str,
) -> Result<Value, EvalError>
where
    F: Fn(&str, &str) -> bool,
{
    evaluator.check_arity_min(&args, 2, fn_name)?;

    for i in 0..args.len() - 1 {
        let (a, b) = match (&args[i], &args[i + 1]) {
            (Value::String(a), Value::String(b)) => (a.borrow().clone(), b.borrow().clone()),
            _ => return Err(EvalError::TypeError(format!("{} expects strings", fn_name))),
        };

        if !cmp(&a, &b) {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

pub(super) fn string_ci_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    string_ci_compare(evaluator, args, |a, b| a == b, "string-ci=?")
}

pub(super) fn string_ci_less(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    string_ci_compare(evaluator, args, |a, b| a < b, "string-ci<?")
}

pub(super) fn string_ci_greater(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    string_ci_compare(evaluator, args, |a, b| a > b, "string-ci>?")
}

pub(super) fn string_ci_less_equal(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    string_ci_compare(evaluator, args, |a, b| a <= b, "string-ci<=?")
}

pub(super) fn string_ci_greater_equal(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    string_ci_compare(evaluator, args, |a, b| a >= b, "string-ci>=?")
}

fn string_ci_compare<F>(
    evaluator: &Evaluator,
    args: Vec<Value>,
    cmp: F,
    fn_name: &str,
) -> Result<Value, EvalError>
where
    F: Fn(&str, &str) -> bool,
{
    evaluator.check_arity_min(&args, 2, fn_name)?;

    for i in 0..args.len() - 1 {
        let (a, b) = match (&args[i], &args[i + 1]) {
            (Value::String(a), Value::String(b)) => {
                (a.borrow().to_lowercase(), b.borrow().to_lowercase())
            }
            _ => return Err(EvalError::TypeError(format!("{} expects strings", fn_name))),
        };

        if !cmp(&a, &b) {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

pub(super) fn string_append(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    let mut result = String::new();

    for arg in args {
        match arg {
            Value::String(s) => result.push_str(&s.borrow()),
            _ => {
                return Err(EvalError::TypeError(format!(
                    "string-append expects strings, got {}",
                    arg.type_name()
                )));
            }
        }
    }

    Ok(Value::String(Rc::new(RefCell::new(result))))
}

pub(super) fn substring(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 3, "substring")?;

    let (s, start, end) = match (&args[0], &args[1], &args[2]) {
        (Value::String(s), Value::Integer(start), Value::Integer(end)) => (s, *start, *end),
        (Value::String(_), _, _) => {
            return Err(EvalError::TypeError(
                "substring indices must be integers".to_string(),
            ));
        }
        _ => {
            return Err(EvalError::TypeError(
                "substring expects string and two integers".to_string(),
            ));
        }
    };

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

    let chars: Vec<char> = s.borrow().chars().collect();

    if end as usize > chars.len() {
        return Err(EvalError::IndexOutOfBounds(format!(
            "substring end index {} out of bounds for string of length {}",
            end,
            chars.len()
        )));
    }

    let substr: String = chars[start as usize..end as usize].iter().collect();
    Ok(Value::String(Rc::new(RefCell::new(substr))))
}

pub(super) fn string_to_list(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "string->list expects 1 to 3 arguments".to_string(),
            actual: args.len(),
        });
    }

    let s = match &args[0] {
        Value::String(s) => s.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "string->list expects a string".to_string(),
            ));
        }
    };

    let chars: Vec<char> = s.chars().collect();
    let start = if args.len() >= 2 {
        match &args[1] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string->list start index must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string->list end index must be an integer".to_string(),
                ));
            }
        }
    } else {
        chars.len()
    };

    if start > end || end > chars.len() {
        return Err(EvalError::IndexOutOfBounds(
            "string->list indices out of bounds".to_string(),
        ));
    }

    let char_values: Vec<Value> = chars[start..end]
        .iter()
        .map(|&ch| Value::Character(ch))
        .collect();

    Ok(evaluator.list_from_vec(char_values))
}

pub(super) fn list_to_string(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "list->string")?;

    let chars = evaluator.list_to_vec(args[0].clone(), "list->string")?;
    let mut result = String::new();

    for ch_val in chars {
        match ch_val {
            Value::Character(ch) => result.push(ch),
            _ => {
                return Err(EvalError::TypeError(
                    "list->string expects a list of characters".to_string(),
                ));
            }
        }
    }

    Ok(Value::String(Rc::new(RefCell::new(result))))
}

pub(super) fn string_copy(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 1, 3, "string-copy")?;

    let s = match &args[0] {
        Value::String(s) => s.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "string-copy expects a string".to_string(),
            ));
        }
    };

    if args.len() == 1 {
        // Simple copy
        return Ok(Value::String(Rc::new(RefCell::new(s))));
    }

    // Copy substring
    let chars: Vec<char> = s.chars().collect();
    let start = match &args[1] {
        Value::Integer(n) => *n as usize,
        _ => {
            return Err(EvalError::TypeError(
                "string-copy start index must be an integer".to_string(),
            ));
        }
    };

    let end = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string-copy end index must be an integer".to_string(),
                ));
            }
        }
    } else {
        chars.len()
    };

    if start > end || end > chars.len() {
        return Err(EvalError::IndexOutOfBounds(
            "string-copy indices out of bounds".to_string(),
        ));
    }

    let substr: String = chars[start..end].iter().collect();
    Ok(Value::String(Rc::new(RefCell::new(substr))))
}

// ========== String Case Conversion ==========

/// (string-upcase string) - Convert string to uppercase
pub(super) fn string_upcase(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "string-upcase")?;

    match &args[0] {
        Value::String(s) => {
            let upper = s.borrow().to_uppercase();
            Ok(Value::String(Rc::new(RefCell::new(upper))))
        }
        _ => Err(EvalError::TypeError(
            "string-upcase expects a string".to_string(),
        )),
    }
}

/// (string-downcase string) - Convert string to lowercase
pub(super) fn string_downcase(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "string-downcase")?;

    match &args[0] {
        Value::String(s) => {
            let lower = s.borrow().to_lowercase();
            Ok(Value::String(Rc::new(RefCell::new(lower))))
        }
        _ => Err(EvalError::TypeError(
            "string-downcase expects a string".to_string(),
        )),
    }
}

/// (string-foldcase string) - Case-fold string for case-insensitive comparison
///
/// R7RS requires full Unicode case folding, which differs from simple lowercasing:
/// - ß (sharp s) folds to "ss"
/// - ſ (long s) folds to "s"
/// - All forms of sigma (Σ, σ, ς) fold to σ
pub(super) fn string_foldcase(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "string-foldcase")?;

    use unicode_casefold::UnicodeCaseFold;

    match &args[0] {
        Value::String(s) => {
            let folded: String = s.borrow().case_fold().collect();
            Ok(Value::String(Rc::new(RefCell::new(folded))))
        }
        _ => Err(EvalError::TypeError(
            "string-foldcase expects a string".to_string(),
        )),
    }
}

// ========== String Mutation ==========

/// (string-fill! string fill [start [end]]) - Fill string with character
pub(super) fn string_fill(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 2, 4, "string-fill!")?;

    let s = match &args[0] {
        Value::String(s) => s,
        _ => {
            return Err(EvalError::TypeError(
                "string-fill! expects a string".to_string(),
            ));
        }
    };

    let fill_char = match &args[1] {
        Value::Character(c) => *c,
        _ => {
            return Err(EvalError::TypeError(
                "string-fill! expects a character as second argument".to_string(),
            ));
        }
    };

    let mut s_borrowed = s.borrow_mut();
    let chars: Vec<char> = s_borrowed.chars().collect();
    let len = chars.len();

    let start = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string-fill! start index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 4 {
        match &args[3] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string-fill! end index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        len
    };

    if start > end || end > len {
        return Err(EvalError::IndexOutOfBounds(
            "string-fill! indices out of bounds".to_string(),
        ));
    }

    // Reconstruct string with filled range
    let mut new_chars = chars;
    for char_slot in new_chars.iter_mut().take(end).skip(start) {
        *char_slot = fill_char;
    }
    *s_borrowed = new_chars.iter().collect();

    Ok(Value::Unspecified)
}

// ========== String Higher-Order Functions ==========

/// (string-map proc string1 string2 ...) - Apply proc to each character
pub(super) fn string_map(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "string-map")?;

    let proc = &args[0];
    let strings: Vec<Vec<char>> = args[1..]
        .iter()
        .map(|v| match v {
            Value::String(s) => Ok(s.borrow().chars().collect()),
            _ => Err(EvalError::TypeError(
                "string-map expects strings".to_string(),
            )),
        })
        .collect::<Result<_, _>>()?;

    if strings.is_empty() {
        return Ok(Value::String(Rc::new(RefCell::new(String::new()))));
    }

    let min_len = strings.iter().map(|s| s.len()).min().unwrap_or(0);
    let mut result = String::with_capacity(min_len);

    for i in 0..min_len {
        let proc_args: Vec<Value> = strings.iter().map(|s| Value::Character(s[i])).collect();
        // Procedure calls within string-map are not in tail position
        let val = match evaluator.apply(proc.clone(), proc_args, false)? {
            super::super::EvalResult::Value(v) => v,
            _ => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in string-map".to_string(),
                ));
            }
        };
        match val {
            Value::Character(c) => result.push(c),
            _ => {
                return Err(EvalError::TypeError(
                    "string-map: procedure must return a character".to_string(),
                ));
            }
        }
    }

    Ok(Value::String(Rc::new(RefCell::new(result))))
}

/// (string-for-each proc string1 string2 ...) - Apply proc for side effects
pub(super) fn string_for_each(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "string-for-each")?;

    let proc = &args[0];
    let strings: Vec<Vec<char>> = args[1..]
        .iter()
        .map(|v| match v {
            Value::String(s) => Ok(s.borrow().chars().collect()),
            _ => Err(EvalError::TypeError(
                "string-for-each expects strings".to_string(),
            )),
        })
        .collect::<Result<_, _>>()?;

    if strings.is_empty() {
        return Ok(Value::Unspecified);
    }

    let min_len = strings.iter().map(|s| s.len()).min().unwrap_or(0);

    // string-for-each is guaranteed to call proc in order
    for i in 0..min_len {
        let proc_args: Vec<Value> = strings.iter().map(|s| Value::Character(s[i])).collect();
        // Procedure calls within string-for-each are not in tail position
        match evaluator.apply(proc.clone(), proc_args, false)? {
            super::super::EvalResult::Value(_) => {}
            _ => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in string-for-each".to_string(),
                ));
            }
        }
    }

    Ok(Value::Unspecified)
}

/// (string-copy! to at from [start [end]]) - Copy characters from one string to another
pub(super) fn string_copy_mutate(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 3, 5, "string-copy!")?;

    let to = match &args[0] {
        Value::String(s) => s,
        _ => {
            return Err(EvalError::TypeError(
                "string-copy! expects a string as first argument".to_string(),
            ));
        }
    };

    let at = match &args[1] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        _ => {
            return Err(EvalError::TypeError(
                "string-copy! at index must be a non-negative integer".to_string(),
            ));
        }
    };

    let from = match &args[2] {
        Value::String(s) => s.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "string-copy! expects a string as third argument".to_string(),
            ));
        }
    };

    let from_chars: Vec<char> = from.chars().collect();
    let from_len = from_chars.len();

    let start = if args.len() >= 4 {
        match &args[3] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string-copy! start index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 5 {
        match &args[4] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string-copy! end index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        from_len
    };

    if start > end || end > from_len {
        return Err(EvalError::IndexOutOfBounds(
            "string-copy! source indices out of bounds".to_string(),
        ));
    }

    let mut to_borrowed = to.borrow_mut();
    let mut to_chars: Vec<char> = to_borrowed.chars().collect();

    let copy_len = end - start;
    if at + copy_len > to_chars.len() {
        return Err(EvalError::IndexOutOfBounds(
            "string-copy! destination index out of bounds".to_string(),
        ));
    }

    // Copy characters from source to destination
    to_chars[at..(copy_len + at)].copy_from_slice(&from_chars[start..(copy_len + start)]);

    *to_borrowed = to_chars.iter().collect();

    Ok(Value::Unspecified)
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // String length
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-length",
        Arity::Exact(1),
        "Returns the number of characters in string.",
        |eval, args, _tail| string_length(eval, args).map(EvalResult::Value),
    ));

    // String ref
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-ref",
        Arity::Exact(2),
        "Returns character k of string.",
        |eval, args, _tail| string_ref(eval, args).map(EvalResult::Value),
    ));

    // String set!
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-set!",
        Arity::Exact(3),
        "Stores char in element k of string.",
        |eval, args, _tail| string_set(eval, args).map(EvalResult::Value),
    ));

    // Make-string
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "make-string",
        Arity::Range(1, 2),
        "Returns a newly allocated string of length k.",
        |eval, args, _tail| make_string(eval, args).map(EvalResult::Value),
    ));

    // String constructor
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string",
        Arity::Min(0),
        "Returns a newly allocated string composed of the arguments.",
        |eval, args, _tail| string(eval, args).map(EvalResult::Value),
    ));

    // String equality
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string=?",
        Arity::Min(2),
        "Returns #t if all strings are equal.",
        |eval, args, _tail| string_equal(eval, args).map(EvalResult::Value),
    ));

    // String less than
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string<?",
        Arity::Min(2),
        "Returns #t if strings are monotonically increasing.",
        |eval, args, _tail| string_less(eval, args).map(EvalResult::Value),
    ));

    // String greater than
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string>?",
        Arity::Min(2),
        "Returns #t if strings are monotonically decreasing.",
        |eval, args, _tail| string_greater(eval, args).map(EvalResult::Value),
    ));

    // String less than or equal
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string<=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-decreasing.",
        |eval, args, _tail| string_less_equal(eval, args).map(EvalResult::Value),
    ));

    // String greater than or equal
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string>=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-increasing.",
        |eval, args, _tail| string_greater_equal(eval, args).map(EvalResult::Value),
    ));

    // String case-insensitive equality
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-ci=?",
        Arity::Min(2),
        "Returns #t if all strings are equal (case-insensitive).",
        |eval, args, _tail| string_ci_equal(eval, args).map(EvalResult::Value),
    ));

    // String case-insensitive less than
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-ci<?",
        Arity::Min(2),
        "Returns #t if strings are monotonically increasing (case-insensitive).",
        |eval, args, _tail| string_ci_less(eval, args).map(EvalResult::Value),
    ));

    // String case-insensitive greater than
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-ci>?",
        Arity::Min(2),
        "Returns #t if strings are monotonically decreasing (case-insensitive).",
        |eval, args, _tail| string_ci_greater(eval, args).map(EvalResult::Value),
    ));

    // String case-insensitive less than or equal
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-ci<=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-decreasing (case-insensitive).",
        |eval, args, _tail| string_ci_less_equal(eval, args).map(EvalResult::Value),
    ));

    // String case-insensitive greater than or equal
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-ci>=?",
        Arity::Min(2),
        "Returns #t if strings are monotonically non-increasing (case-insensitive).",
        |eval, args, _tail| string_ci_greater_equal(eval, args).map(EvalResult::Value),
    ));

    // String append
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-append",
        Arity::Min(0),
        "Returns a newly allocated string whose characters are the concatenation of the given strings.",
        |eval, args, _tail| string_append(eval, args).map(EvalResult::Value),
    ));

    // Substring
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "substring",
        Arity::Exact(3),
        "Returns a newly allocated string formed from the characters of string beginning with index start and ending with index end.",
        |eval, args, _tail| substring(eval, args).map(EvalResult::Value),
    ));

    // String->list
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string->list",
        Arity::Range(1, 3),
        "Returns a newly allocated list of the characters of string.",
        |eval, args, _tail| string_to_list(eval, args).map(EvalResult::Value),
    ));

    // List->string
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "list->string",
        Arity::Exact(1),
        "Returns a newly allocated string formed from the characters in the list.",
        |eval, args, _tail| list_to_string(eval, args).map(EvalResult::Value),
    ));

    // String-copy
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-copy",
        Arity::Range(1, 3),
        "Returns a newly allocated copy of the part of the given string.",
        |eval, args, _tail| string_copy(eval, args).map(EvalResult::Value),
    ));

    // String case conversion (scheme.char library)
    registry.register(PrimitiveFn::new(
        "scheme.char",
        "string-upcase",
        Arity::Exact(1),
        "Returns a newly allocated string with all characters converted to uppercase.",
        |eval, args, _tail| string_upcase(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "string-downcase",
        Arity::Exact(1),
        "Returns a newly allocated string with all characters converted to lowercase.",
        |eval, args, _tail| string_downcase(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.char",
        "string-foldcase",
        Arity::Exact(1),
        "Returns a newly allocated string with all characters case-folded.",
        |eval, args, _tail| string_foldcase(eval, args).map(EvalResult::Value),
    ));

    // String mutation (scheme.base)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-fill!",
        Arity::Range(2, 4),
        "Stores fill character in every element of string between start and end.",
        |eval, args, _tail| string_fill(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-copy!",
        Arity::Range(3, 5),
        "Copies characters from source string to destination string.",
        |eval, args, _tail| string_copy_mutate(eval, args).map(EvalResult::Value),
    ));

    // String higher-order functions (scheme.base)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-map",
        Arity::Min(2),
        "Applies proc element-wise to strings and returns a string of the results.",
        |eval, args, _tail| string_map(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string-for-each",
        Arity::Min(2),
        "Applies proc to each element of the strings for its side effects.",
        |eval, args, _tail| string_for_each(eval, args).map(EvalResult::Value),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::eval::cps_eval::eval_cps;
    use patina_frontend::{Desugarer, Parser};

    fn make_evaluator() -> Evaluator {
        Evaluator::new()
    }

    fn make_string(s: &str) -> Value {
        Value::String(Rc::new(RefCell::new(s.to_string())))
    }

    /// Helper to parse and evaluate a lambda expression via CPS
    fn make_lambda(eval: &Evaluator, code: &str) -> Value {
        let mut parser = Parser::new(code).expect("Failed to create parser");
        let expr = parser.parse().expect("Failed to parse");
        let desugarer = Desugarer::with_env(eval.global_env.clone());
        let core_expr = desugarer.desugar(&expr).expect("Failed to desugar");
        eval_cps(&core_expr, eval.global_env.clone(), eval).expect("Failed to evaluate lambda")
    }

    // string-map tests

    #[test]
    fn test_string_map_identity() {
        let eval = make_evaluator();
        let identity = make_lambda(&eval, "(lambda (c) c)");

        let result = string_map(&eval, vec![identity, make_string("hello")]).unwrap();
        assert_eq!(result.to_string(), "\"hello\"");
    }

    #[test]
    fn test_string_map_empty_string() {
        let eval = make_evaluator();
        let identity = make_lambda(&eval, "(lambda (c) c)");

        let result = string_map(&eval, vec![identity, make_string("")]).unwrap();
        assert_eq!(result.to_string(), "\"\"");
    }

    #[test]
    fn test_string_map_multiple_strings_shortest_wins() {
        let eval = make_evaluator();
        let first = make_lambda(&eval, "(lambda (a b) a)");

        let result = string_map(&eval, vec![first, make_string("abc"), make_string("xy")]).unwrap();
        // Should only process 2 characters (length of "xy")
        assert_eq!(result.to_string(), "\"ab\"");
    }

    #[test]
    fn test_string_map_unicode() {
        let eval = make_evaluator();
        let identity = make_lambda(&eval, "(lambda (c) c)");

        let result = string_map(&eval, vec![identity, make_string("λαβ")]).unwrap();
        assert_eq!(result.to_string(), "\"λαβ\"");
    }

    #[test]
    fn test_string_map_wrong_return_type() {
        let eval = make_evaluator();
        let bad_proc = make_lambda(&eval, "(lambda (c) 42)");

        let result = string_map(&eval, vec![bad_proc, make_string("abc")]);
        assert!(result.is_err());
    }

    // string-for-each tests

    #[test]
    fn test_string_for_each_basic() {
        let eval = make_evaluator();
        let proc = make_lambda(&eval, "(lambda (c) c)");

        let result = string_for_each(&eval, vec![proc, make_string("hello")]).unwrap();
        assert!(matches!(result, Value::Unspecified));
    }

    #[test]
    fn test_string_for_each_empty_string() {
        let eval = make_evaluator();
        let proc = make_lambda(&eval, "(lambda (c) c)");

        let result = string_for_each(&eval, vec![proc, make_string("")]).unwrap();
        assert!(matches!(result, Value::Unspecified));
    }

    #[test]
    fn test_string_for_each_multiple_strings() {
        let eval = make_evaluator();
        let proc = make_lambda(&eval, "(lambda (a b) a)");

        let result =
            string_for_each(&eval, vec![proc, make_string("abc"), make_string("xyz")]).unwrap();
        assert!(matches!(result, Value::Unspecified));
    }

    #[test]
    fn test_string_for_each_type_error() {
        let eval = make_evaluator();
        let proc = make_lambda(&eval, "(lambda (c) c)");

        // Pass a number instead of string
        let result = string_for_each(&eval, vec![proc, Value::Integer(42)]);
        assert!(result.is_err());
    }
}
