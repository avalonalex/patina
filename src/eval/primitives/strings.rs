//! String primitive operations with UTF-8 support (R7RS Section 6.7)
//!
//! Implements string operations with full Unicode support:
//! - Character-based indexing (O(n) as allowed by R7RS)
//! - Mutable strings via string-set!
//! - Comparison operations (case-sensitive and case-insensitive)
//!
//! Total: 20 string primitives + 2 helper functions

use super::super::error::EvalError;
use super::super::Evaluator;
use crate::value::Value;
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
            ))
        }
        _ => {
            return Err(EvalError::TypeError(format!(
                "string-ref expects string and integer, got {} and {}",
                args[0].type_name(),
                args[1].type_name()
            )))
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
            ))
        }
        (Value::String(_), _, _) => {
            return Err(EvalError::TypeError(
                "string-set! index must be an integer".to_string(),
            ))
        }
        _ => {
            return Err(EvalError::TypeError(
                "string-set! expects string, integer, and character".to_string(),
            ))
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
            ))
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
                ))
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
                )))
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
                )))
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
            ))
        }
        _ => {
            return Err(EvalError::TypeError(
                "substring expects string and two integers".to_string(),
            ))
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
            ))
        }
    };

    let chars: Vec<char> = s.chars().collect();
    let start = if args.len() >= 2 {
        match &args[1] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string->list start index must be an integer".to_string(),
                ))
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
                ))
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
                ))
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
            ))
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
            ))
        }
    };

    let end = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string-copy end index must be an integer".to_string(),
                ))
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
