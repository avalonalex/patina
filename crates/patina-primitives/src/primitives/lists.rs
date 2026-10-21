//! List and pair primitive operations (R7RS Section 6.4)
//!
//! Implements pair construction, list manipulation, and search operations.
//!
//! Each takes the context (#706), but for `member` and `assoc`, which call a
//! comparator from Rust and convert with the other higher-order primitives.
//! The mutators store through the context's funnel (`Cx::set_car`,
//! `Cx::set_cdr`).

use crate::apply_context::ApplyContext;
use patina_core::{Cx, SpineEnd, Value};
use patina_runtime::EvalError;

// ========== Argument Extraction Helpers ==========

/// Extract an integer from a value
fn get_integer<'gc>(cx: &Cx<'gc>, tv: Value<'gc>, fn_name: &str) -> Result<i64, EvalError> {
    if tv.is_fixnum() {
        return Ok(tv.as_fixnum_unchecked());
    }
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

// ========== List Primitives ==========

pub(super) fn cons<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    // Allocate native heap pair
    let pair = cx.alloc_pair(args[0], args[1]);
    Ok(pair)
}

pub(super) fn car<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Fast path: native heap pair
    if args[0].is_pair() {
        return Ok(cx.car(args[0]));
    }

    // Fallback: use try_pair for any other pair types
    let (car, _) = cx
        .try_pair(args[0])
        .ok_or_else(|| EvalError::TypeError("car expects a pair".into()))?;
    Ok(car)
}

pub(super) fn cdr<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Fast path: native heap pair
    if args[0].is_pair() {
        return Ok(cx.cdr(args[0]));
    }

    // Fallback: use try_pair for any other pair types
    let (_, cdr) = cx
        .try_pair(args[0])
        .ok_or_else(|| EvalError::TypeError("cdr expects a pair".into()))?;
    Ok(cdr)
}

/// Define a car/cdr composition (caar, cadr, ..., cddddr), steps listed
/// innermost first — so `cadr` is `cdr` then `car`, exactly like the former
/// Scheme definition `(define (cadr x) (car (cdr x)))`.
///
/// Each step is a direct `Cx::car`/`Cx::cdr` rather than a chained call
/// through the handler fns above: the handlers' `try_pair` fallback only
/// ever *fails* (it is `is_pair()`-gated, same as the fast path), so
/// guarding with `is_pair` and emitting the same "car/cdr expects a pair"
/// message is behavior-identical while skipping the per-step arity check
/// and slice temp.
macro_rules! cxr {
    ($fname:ident, $($step:ident),+) => {
        pub(super) fn $fname<'gc>(
            cx: &mut Cx<'gc>,
            args: &[Value<'gc>],
        ) -> Result<Value<'gc>, EvalError> {
            crate::registry::expect_arity(args, 1)?;
            let mut v = args[0];
            $(
                if !v.is_pair() {
                    return Err(EvalError::TypeError(
                        concat!(stringify!($step), " expects a pair").into(),
                    ));
                }
                v = cx.$step(v);
            )+
            Ok(v)
        }
    };
}

// Two-deep compositions
cxr!(caar, car, car);
cxr!(cadr, cdr, car);
cxr!(cdar, car, cdr);
cxr!(cddr, cdr, cdr);

// Three-deep compositions
cxr!(caaar, car, car, car);
cxr!(caadr, cdr, car, car);
cxr!(cadar, car, cdr, car);
cxr!(caddr, cdr, cdr, car);
cxr!(cdaar, car, car, cdr);
cxr!(cdadr, cdr, car, cdr);
cxr!(cddar, car, cdr, cdr);
cxr!(cdddr, cdr, cdr, cdr);

// Four-deep compositions ((scheme cxr))
cxr!(caaaar, car, car, car, car);
cxr!(caaadr, cdr, car, car, car);
cxr!(caadar, car, cdr, car, car);
cxr!(caaddr, cdr, cdr, car, car);
cxr!(cadaar, car, car, cdr, car);
cxr!(cadadr, cdr, car, cdr, car);
cxr!(caddar, car, cdr, cdr, car);
cxr!(cadddr, cdr, cdr, cdr, car);
cxr!(cdaaar, car, car, car, cdr);
cxr!(cdaadr, cdr, car, car, cdr);
cxr!(cdadar, car, cdr, car, cdr);
cxr!(cdaddr, cdr, cdr, car, cdr);
cxr!(cddaar, car, car, cdr, cdr);
cxr!(cddadr, cdr, car, cdr, cdr);
cxr!(cdddar, car, cdr, cdr, cdr);
cxr!(cddddr, cdr, cdr, cdr, cdr);

pub(super) fn list<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    // list accepts any number of arguments (0 or more)
    // Use native heap list construction
    Ok(cx.list_from_iter(args.iter().copied()))
}

pub(super) fn length<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let len = cx
        .list_len(args[0])
        .ok_or_else(|| EvalError::TypeError("length expects a proper list".into()))?;
    Ok(Value::fixnum(len as i64))
}

pub(super) fn append<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    let Some((&last, lists)) = args.split_last() else {
        return Ok(Value::NULL);
    };
    // Only non-final arguments must be proper lists. The last argument is
    // shared as a tail, including when it is an atom or a circular list.
    let mut result = last;
    for &list in lists.iter().rev() {
        result = cx
            .list_append(list, result)
            .ok_or_else(|| EvalError::TypeError("append expects a proper list".into()))?;
    }
    Ok(result)
}

pub(super) fn reverse<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    cx.list_reverse(args[0])
        .ok_or_else(|| EvalError::TypeError("reverse expects a proper list".into()))
}

pub(super) fn list_ref<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let k = get_integer(cx, args[1], "list-ref")?;
    if k < 0 {
        return Err(EvalError::TypeError(
            "list-ref: index must be non-negative".to_string(),
        ));
    }
    let k = k as usize;

    // Fast path: use Cx::list_ref for native pairs
    if let Some(elem) = cx.list_ref(args[0], k) {
        return Ok(elem);
    }

    // Slow path: walk via try_pair
    let mut current = args[0];
    for _ in 0..k {
        let (_, cdr) = cx
            .try_pair(current)
            .ok_or_else(|| EvalError::TypeError("list-ref: index out of bounds".into()))?;
        current = cdr;
    }
    let (car, _) = cx
        .try_pair(current)
        .ok_or_else(|| EvalError::TypeError("list-ref: index out of bounds".into()))?;
    Ok(car)
}

pub(super) fn list_tail<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let k = get_integer(cx, args[1], "list-tail")?;
    if k < 0 {
        return Err(EvalError::TypeError(
            "list-tail: index must be non-negative".to_string(),
        ));
    }
    let k = k as usize;

    // Fast path: use Cx::list_tail for native pairs
    if let Some(tail) = cx.list_tail(args[0], k) {
        return Ok(tail);
    }

    // Slow path: walk via try_pair
    let mut current = args[0];
    for _ in 0..k {
        let (_, cdr) = cx
            .try_pair(current)
            .ok_or_else(|| EvalError::TypeError("list-tail: index out of bounds".into()))?;
        current = cdr;
    }
    Ok(current)
}

pub(super) fn memq<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let obj = args[0];
    let mut current = args[1];

    loop {
        if current.is_null() {
            break;
        }

        // Fast path: native heap pair
        if current.is_pair() {
            let (car, cdr) = cx.get_pair(current);
            if cx.values_eq(obj, car) {
                return Ok(current);
            }
            current = cdr;
            continue;
        }

        // Fallback path: use try_pair for any other pair types
        let saved = current;
        let (car, cdr) = match cx.try_pair(current) {
            Some(pair) => pair,
            None => break,
        };
        if cx.values_eq(obj, car) {
            return Ok(saved);
        }
        current = cdr;
    }

    Ok(Value::FALSE)
}

pub(super) fn memv<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let obj = args[0];
    let mut current = args[1];

    loop {
        if current.is_null() {
            break;
        }

        // Fast path: native heap pair
        if current.is_pair() {
            let (car, cdr) = cx.get_pair(current);
            if cx.values_eqv(obj, car) {
                return Ok(current);
            }
            current = cdr;
            continue;
        }

        // Fallback path: use try_pair for any other pair types
        let saved = current;
        let (car, cdr) = match cx.try_pair(current) {
            Some(pair) => pair,
            None => break,
        };
        if cx.values_eqv(obj, car) {
            return Ok(saved);
        }
        current = cdr;
    }

    Ok(Value::FALSE)
}

pub(super) fn member<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "2 or 3".to_string(),
            actual: args.len(),
        });
    }

    if args.len() == 2 {
        // 2-arg path: `equal?` through the context
        let obj = args[0];
        let mut current = args[1];

        loop {
            if current.is_null() {
                break;
            }

            // Fast path: native heap pair
            if current.is_pair() {
                let car = cx.car(current);
                let cdr = cx.cdr(current);
                if cx.tagged_values_equal(obj, car) {
                    return Ok(current);
                }
                current = cdr;
                continue;
            }

            // Fallback path: use try_pair for any other pair types
            let saved = current;
            let (car, cdr) = match cx.try_pair(current) {
                Some(pair) => pair,
                None => break,
            };
            if cx.tagged_values_equal(obj, car) {
                return Ok(saved);
            }
            current = cdr;
        }

        Ok(Value::FALSE)
    } else {
        // 3-arg path: custom comparator, called back through the context
        let obj = args[0];
        let compare_proc = args[2];

        let mut current = args[1];

        loop {
            if current.is_null() {
                break;
            }

            let saved = current;
            let (car_tv, cdr) = match cx.try_pair(current) {
                Some(pair) => pair,
                None => break,
            };

            // Reached only by importing `(patina internal lists)`: the
            // `(scheme base)` `member` calls its comparator as a frame (#471).
            let result = ctx.apply_in(cx, compare_proc, &[obj, car_tv])? != Value::FALSE;

            if result {
                return Ok(saved);
            }
            current = cdr;
        }

        Ok(Value::FALSE)
    }
}

pub(super) fn assq<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let obj = args[0];
    let mut current = args[1];

    loop {
        if current.is_null() {
            break;
        }

        // Get (entry, rest) from alist spine
        let (entry, rest) = match cx.try_pair(current) {
            Some(pair) => pair,
            None => break,
        };

        // Get (entry_car, _) from entry pair
        let entry_car = cx.try_pair(entry).map(|(c, _)| c);
        if let Some(entry_car) = entry_car
            && cx.values_eq(obj, entry_car)
        {
            return Ok(entry);
        }

        current = rest;
    }

    Ok(Value::FALSE)
}

pub(super) fn assv<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let obj = args[0];
    let mut current = args[1];

    loop {
        if current.is_null() {
            break;
        }

        // Get (entry, rest) from alist spine
        let (entry, rest) = match cx.try_pair(current) {
            Some(pair) => pair,
            None => break,
        };

        // Get (entry_car, _) from entry pair
        let entry_car = cx.try_pair(entry).map(|(c, _)| c);
        if let Some(entry_car) = entry_car
            && cx.values_eqv(obj, entry_car)
        {
            return Ok(entry);
        }

        current = rest;
    }

    Ok(Value::FALSE)
}

pub(super) fn assoc<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "2 or 3".to_string(),
            actual: args.len(),
        });
    }

    if args.len() == 2 {
        // 2-arg path: `equal?` through the context
        let obj = args[0];
        let mut current = args[1];

        loop {
            if current.is_null() {
                break;
            }

            // Get (entry, rest) from alist spine
            let (entry, rest) = match cx.try_pair(current) {
                Some(pair) => pair,
                None => break,
            };

            // Get (entry_car, _) from entry pair
            let entry_car = cx.try_pair(entry).map(|(c, _)| c);
            if let Some(entry_car) = entry_car
                && cx.tagged_values_equal(obj, entry_car)
            {
                return Ok(entry);
            }

            current = rest;
        }

        Ok(Value::FALSE)
    } else {
        // 3-arg path: custom comparator, called back through the context
        let obj = args[0];
        let compare_proc = args[2];

        let mut current = args[1];

        loop {
            if current.is_null() {
                break;
            }

            // Get (entry, rest) from alist spine
            let (entry, rest) = match cx.try_pair(current) {
                Some(pair) => pair,
                None => break,
            };

            // Get (entry_car, _) from entry pair
            let entry_car_tv = cx.try_pair(entry).map(|(c, _)| c);
            if let Some(entry_car_tv) = entry_car_tv {
                // Reached only by importing `(patina internal lists)`: the
                // `(scheme base)` `assoc` calls its comparator as a frame (#471).
                let result = ctx.apply_in(cx, compare_proc, &[obj, entry_car_tv])? != Value::FALSE;

                if result {
                    return Ok(entry);
                }
            }

            current = rest;
        }

        Ok(Value::FALSE)
    }
}

/// (make-list k [fill]) - Create list of k elements
pub(super) fn make_list<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }

    // Get the length
    let k = get_integer(cx, args[0], "make-list")?;
    if k < 0 {
        return Err(EvalError::TypeError(format!(
            "make-list: length must be non-negative, got {}",
            k
        )));
    }
    let k = k as usize;

    // Get the fill value (default to unspecified)
    let fill = if args.len() == 2 {
        args[1]
    } else {
        Value::UNSPECIFIED
    };

    // Build the list a pair at a time
    let mut result = Value::NULL;
    for _ in 0..k {
        result = cx.alloc_pair(fill, result);
    }

    Ok(result)
}

/// (list-copy list) - Create shallow copy of list
/// Copies proper and dotted spines; rejects circular cdr chains.
pub(super) fn list_copy<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Validate before allocating output pairs. Unlike list_to_vec, spine
    // preserves an improper tail (and returns a non-pair input unchanged).
    let (cars, end) = cx.spine(args[0]);
    let tail = match end {
        SpineEnd::Null => Value::NULL,
        SpineEnd::Improper(tail) => tail,
        SpineEnd::Circular => {
            return Err(EvalError::TypeError("list-copy: circular list".into()));
        }
    };
    Ok(cx.list_from_iter_with_tail(cars, tail))
}

/// (set-car! pair obj) - Mutate the car of a pair
pub(super) fn set_car<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    if args[0].is_pair() {
        cx.set_car(args[0], args[1]);
        Ok(Value::UNSPECIFIED)
    } else {
        Err(EvalError::TypeError("set-car! expects a pair".to_string()))
    }
}

/// (set-cdr! pair obj) - Mutate the cdr of a pair
pub(super) fn set_cdr<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    if args[0].is_pair() {
        cx.set_cdr(args[0], args[1]);
        Ok(Value::UNSPECIFIED)
    } else {
        Err(EvalError::TypeError("set-cdr! expects a pair".to_string()))
    }
}

/// (list-set! list k obj) - Set the k-th element of a list to obj
pub(super) fn list_set<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

    // Get index first
    let index = get_integer(cx, args[1], "list-set!")?;
    if index < 0 {
        return Err(EvalError::TypeError(format!(
            "list-set!: index must be non-negative, got {}",
            index
        )));
    }
    let index = index as usize;

    // Fast path: native heap pair
    if args[0].is_pair() {
        let mut current = args[0];
        for i in 0..index {
            if !current.is_pair() {
                return Err(EvalError::TypeError(format!(
                    "list-set!: index {} out of bounds (list has {} elements)",
                    index, i
                )));
            }
            current = cx.cdr(current);
        }

        if current.is_pair() {
            cx.set_car(current, args[2]);
            return Ok(Value::UNSPECIFIED);
        } else {
            return Err(EvalError::TypeError(format!(
                "list-set!: index {} out of bounds",
                index
            )));
        }
    }

    // Slow path: walk via try_pair to find the target pair
    let mut current = args[0];
    for i in 0..index {
        let (_, cdr) = cx.try_pair(current).ok_or_else(|| {
            EvalError::TypeError(format!(
                "list-set!: index {} out of bounds (list has {} elements)",
                index, i
            ))
        })?;
        current = cdr;
    }

    if current.is_pair() {
        cx.set_car(current, args[2]);
        Ok(Value::UNSPECIFIED)
    } else {
        Err(EvalError::TypeError(format!(
            "list-set!: index {} out of bounds",
            index
        )))
    }
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Cons - construct pair
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "cons",
        Arity::Exact(2),
        "Returns a newly allocated pair whose car is obj1 and whose cdr is obj2.",
        cons,
    ));

    // Car - first element of pair
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "car",
        Arity::Exact(1),
        "Returns the contents of the car field of pair.",
        car,
    ));

    // Cdr - rest of pair
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "cdr",
        Arity::Exact(1),
        "Returns the contents of the cdr field of pair.",
        cdr,
    ));

    // Car/cdr compositions (two- and three-deep, R7RS §6.4)
    for (name, handler) in [
        ("caar", caar as crate::registry::CxHandler),
        ("cadr", cadr),
        ("cdar", cdar),
        ("cddr", cddr),
        ("caaar", caaar),
        ("caadr", caadr),
        ("cadar", cadar),
        ("caddr", caddr),
        ("cdaar", cdaar),
        ("cdadr", cdadr),
        ("cddar", cddar),
        ("cdddr", cdddr),
    ] {
        registry.register(PrimitiveFn::new_cx(
            "scheme.base",
            name,
            Arity::Exact(1),
            "Composition of car/cdr accessors.",
            handler,
        ));
    }

    // Four-deep compositions ((scheme cxr))
    for (name, handler) in [
        ("caaaar", caaaar as crate::registry::CxHandler),
        ("caaadr", caaadr),
        ("caadar", caadar),
        ("caaddr", caaddr),
        ("cadaar", cadaar),
        ("cadadr", cadadr),
        ("caddar", caddar),
        ("cadddr", cadddr),
        ("cdaaar", cdaaar),
        ("cdaadr", cdaadr),
        ("cdadar", cdadar),
        ("cdaddr", cdaddr),
        ("cddaar", cddaar),
        ("cddadr", cddadr),
        ("cdddar", cdddar),
        ("cddddr", cddddr),
    ] {
        registry.register(PrimitiveFn::new_cx(
            "scheme.cxr",
            name,
            Arity::Exact(1),
            "Composition of car/cdr accessors.",
            handler,
        ));
    }

    // List - construct list from arguments
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "list",
        Arity::Min(0),
        "Returns a newly allocated list of its arguments.",
        list,
    ));

    // Length - list length
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "length",
        Arity::Exact(1),
        "Returns the length of list.",
        length,
    ));

    // Append - concatenate lists
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "append",
        Arity::Min(0),
        "Returns a list consisting of the elements of the first list followed by the elements of the other lists.",
        append,
    ));

    // Reverse - reverse list
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "reverse",
        Arity::Exact(1),
        "Returns a newly allocated list consisting of the elements of list in reverse order.",
        reverse,
    ));

    // List-ref - nth element
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "list-ref",
        Arity::Exact(2),
        "Returns the kth element of list.",
        list_ref,
    ));

    // List-tail - drop first k elements
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "list-tail",
        Arity::Exact(2),
        "Returns the sublist of list obtained by omitting the first k elements.",
        list_tail,
    ));

    // Memq - member using eq?
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "memq",
        Arity::Exact(2),
        "Returns the first sublist of list whose car is obj (compared using eq?), or #f if not found.",
        memq,
    ));

    // Memv - member using eqv?
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "memv",
        Arity::Exact(2),
        "Returns the first sublist of list whose car is obj (compared using eqv?), or #f if not found.",
        memv,
    ));

    // Member - member using equal? (or custom comparator)
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "member",
        Arity::Range(2, 3),
        "Returns the first sublist of list whose car is obj. Uses equal? or optional comparator.",
        member,
    ));

    // Assq - association list lookup using eq?
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "assq",
        Arity::Exact(2),
        "Returns the first pair in alist whose car is obj (compared using eq?), or #f if not found.",
        assq,
    ));

    // Assv - association list lookup using eqv?
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "assv",
        Arity::Exact(2),
        "Returns the first pair in alist whose car is obj (compared using eqv?), or #f if not found.",
        assv,
    ));

    // Assoc - association list lookup using equal? (or custom comparator)
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "assoc",
        Arity::Range(2, 3),
        "Returns the first pair in alist whose car is obj. Uses equal? or optional comparator.",
        assoc,
    ));

    // make-list - Create list of k elements with optional fill value
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "make-list",
        Arity::Range(1, 2),
        "Returns a newly allocated list of k elements. If fill is given, each element is initialized to fill; otherwise unspecified.",
        make_list,
    ));

    // list-copy - Create shallow copy of list
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "list-copy",
        Arity::Exact(1),
        "Returns a newly allocated copy of list. Only the top level of structure is copied.",
        list_copy,
    ));

    // set-car! - Mutate car of pair
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "set-car!",
        Arity::Exact(2),
        "Stores obj in the car field of pair.",
        set_car,
    ));

    // set-cdr! - Mutate cdr of pair
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "set-cdr!",
        Arity::Exact(2),
        "Stores obj in the cdr field of pair.",
        set_cdr,
    ));

    // list-set! - Set element at index
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "list-set!",
        Arity::Exact(3),
        "Stores obj in the kth element of list.",
        list_set,
    ));
}
