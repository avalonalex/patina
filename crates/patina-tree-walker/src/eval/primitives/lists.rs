//! List and pair primitive operations (R7RS Section 6.4)
//!
//! Implements pair construction, list manipulation, and search operations.

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_core::TaggedValue;

// ========== TaggedValue Extraction Helpers ==========

/// Extract an integer from a TaggedValue
fn get_integer(tv: TaggedValue, heap: &patina_core::Heap, fn_name: &str) -> Result<i64, EvalError> {
    if tv.is_fixnum() {
        return Ok(tv.as_fixnum_unchecked());
    }
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

// ========== List Primitives ==========

pub(super) fn cons(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    // Allocate native heap pair - args are already TaggedValue
    let heap = evaluator.global_env.heap();
    let pair_tv = heap.borrow_mut().alloc_pair(args[0], args[1]);
    Ok(pair_tv)
}

pub(super) fn car(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Fast path: native heap pair
    if args[0].is_pair() {
        let heap = evaluator.global_env.heap();
        let heap_ref = heap.borrow();
        let car_tv = heap_ref.car(args[0]);
        return Ok(car_tv);
    }

    // Fallback: use try_pair for any other pair types
    let heap = evaluator.global_env.heap();
    let (car, _) = heap
        .borrow()
        .try_pair(args[0])
        .ok_or_else(|| EvalError::TypeError("car expects a pair".into()))?;
    Ok(car)
}

pub(super) fn cdr(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Fast path: native heap pair
    if args[0].is_pair() {
        let heap = evaluator.global_env.heap();
        let heap_ref = heap.borrow();
        let cdr_tv = heap_ref.cdr(args[0]);
        return Ok(cdr_tv);
    }

    // Fallback: use try_pair for any other pair types
    let heap = evaluator.global_env.heap();
    let (_, cdr) = heap
        .borrow()
        .try_pair(args[0])
        .ok_or_else(|| EvalError::TypeError("cdr expects a pair".into()))?;
    Ok(cdr)
}

pub(super) fn list(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    // list accepts any number of arguments (0 or more)
    // Use native heap list construction
    let heap = evaluator.global_env.heap();
    Ok(heap.borrow_mut().list_from_iter(args))
}

pub(super) fn length(
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

    // Fast path: use Heap::list_len for native pairs
    {
        let heap_ref = heap.borrow();
        if let Some(len) = heap_ref.list_len(args[0]) {
            return Ok(TaggedValue::fixnum(len as i64));
        }
    }

    // Slow path: walk via try_pair
    let mut count = 0usize;
    let mut current = args[0];
    loop {
        if current.is_null() {
            break;
        }
        let (_, cdr) = heap
            .borrow()
            .try_pair(current)
            .ok_or_else(|| EvalError::TypeError("length expects a proper list".into()))?;
        count += 1;
        current = cdr;
    }
    Ok(TaggedValue::fixnum(count as i64))
}

pub(super) fn append(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() {
        return Ok(TaggedValue::NULL);
    }

    if args.len() == 1 {
        return Ok(args[0]);
    }

    let heap = evaluator.global_env.heap();

    // Try fast path using heap's list_append for native pairs
    // Fold from right: (append a b c d) = (append a (append b (append c d)))
    let mut result = args[args.len() - 1];
    let mut all_native = true;

    for i in (0..args.len() - 1).rev() {
        let list = args[i];

        // Try heap's list_append for native pairs
        if let Some(appended) = heap.borrow_mut().list_append(list, result) {
            result = appended;
        } else {
            // list_append failed - not a proper list
            all_native = false;
            break;
        }
    }

    if all_native {
        return Ok(result);
    }

    // Slow path: collect elements via try_pair and rebuild
    let mut all_cars: Vec<TaggedValue> = Vec::new();
    for arg in args.iter().take(args.len() - 1) {
        let mut current = *arg;
        loop {
            if current.is_null() {
                break;
            }
            let (car, cdr) = heap
                .borrow()
                .try_pair(current)
                .ok_or_else(|| EvalError::TypeError("append expects a proper list".into()))?;
            all_cars.push(car);
            current = cdr;
        }
    }

    // Build result: prepend collected elements before last arg
    let mut result = args[args.len() - 1];
    let mut heap_ref = heap.borrow_mut();
    for car in all_cars.into_iter().rev() {
        result = heap_ref.alloc_pair(car, result);
    }
    Ok(result)
}

pub(super) fn reverse(
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

    // Fast path: use Heap::list_reverse for native pairs
    if (args[0].is_pair() || args[0].is_null())
        && let Some(reversed) = heap.borrow_mut().list_reverse(args[0])
    {
        return Ok(reversed);
    }

    // Slow path: walk via try_pair and build reversed list
    let mut result = TaggedValue::NULL;
    let mut current = args[0];
    loop {
        if current.is_null() {
            break;
        }
        let (car, cdr) = heap
            .borrow()
            .try_pair(current)
            .ok_or_else(|| EvalError::TypeError("reverse expects a proper list".into()))?;
        result = heap.borrow_mut().alloc_pair(car, result);
        current = cdr;
    }
    Ok(result)
}

pub(super) fn list_ref(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let k = get_integer(args[1], &heap_ref, "list-ref")?;
    if k < 0 {
        return Err(EvalError::TypeError(
            "list-ref: index must be non-negative".to_string(),
        ));
    }
    let k = k as usize;

    // Fast path: use Heap::list_ref for native pairs
    if let Some(elem) = heap_ref.list_ref(args[0], k) {
        return Ok(elem);
    }

    // Slow path: walk via try_pair
    drop(heap_ref);
    let mut current = args[0];
    for _ in 0..k {
        let (_, cdr) = heap
            .borrow()
            .try_pair(current)
            .ok_or_else(|| EvalError::TypeError("list-ref: index out of bounds".into()))?;
        current = cdr;
    }
    let (car, _) = heap
        .borrow()
        .try_pair(current)
        .ok_or_else(|| EvalError::TypeError("list-ref: index out of bounds".into()))?;
    Ok(car)
}

pub(super) fn list_tail(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let k = get_integer(args[1], &heap_ref, "list-tail")?;
    if k < 0 {
        return Err(EvalError::TypeError(
            "list-tail: index must be non-negative".to_string(),
        ));
    }
    let k = k as usize;

    // Fast path: use Heap::list_tail for native pairs
    if let Some(tail) = heap_ref.list_tail(args[0], k) {
        return Ok(tail);
    }

    // Slow path: walk via try_pair
    drop(heap_ref);
    let mut current = args[0];
    for _ in 0..k {
        let (_, cdr) = heap
            .borrow()
            .try_pair(current)
            .ok_or_else(|| EvalError::TypeError("list-tail: index out of bounds".into()))?;
        current = cdr;
    }
    Ok(current)
}

pub(super) fn memq(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let obj = args[0];
    let mut current = args[1];

    loop {
        if current.is_null() {
            break;
        }

        // Fast path: native heap pair
        if current.is_pair() {
            let heap_ref = heap.borrow();
            let car = heap_ref.car(current);
            let cdr = heap_ref.cdr(current);
            if heap_ref.values_eq(obj, car) {
                return Ok(current);
            }
            drop(heap_ref);
            current = cdr;
            continue;
        }

        // Fallback path: use try_pair for any other pair types
        let saved = current;
        let (car, cdr) = match heap.borrow().try_pair(current) {
            Some(pair) => pair,
            None => break,
        };
        if heap.borrow().values_eq(obj, car) {
            return Ok(saved);
        }
        current = cdr;
    }

    Ok(TaggedValue::FALSE)
}

pub(super) fn memv(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let obj = args[0];
    let mut current = args[1];

    loop {
        if current.is_null() {
            break;
        }

        // Fast path: native heap pair
        if current.is_pair() {
            let heap_ref = heap.borrow();
            let car = heap_ref.car(current);
            let cdr = heap_ref.cdr(current);
            if heap_ref.values_eqv(obj, car) {
                return Ok(current);
            }
            drop(heap_ref);
            current = cdr;
            continue;
        }

        // Fallback path: use try_pair for any other pair types
        let saved = current;
        let (car, cdr) = match heap.borrow().try_pair(current) {
            Some(pair) => pair,
            None => break,
        };
        if heap.borrow().values_eqv(obj, car) {
            return Ok(saved);
        }
        current = cdr;
    }

    Ok(TaggedValue::FALSE)
}

pub(super) fn member(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "2 or 3".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();

    if args.len() == 2 {
        // 2-arg path: pure TaggedValue with heap.tagged_values_equal
        let obj = args[0];
        let mut current = args[1];

        loop {
            if current.is_null() {
                break;
            }

            // Fast path: native heap pair
            if current.is_pair() {
                let heap_ref = heap.borrow();
                let car = heap_ref.car(current);
                let cdr = heap_ref.cdr(current);
                if heap_ref.tagged_values_equal(obj, car) {
                    return Ok(current);
                }
                drop(heap_ref);
                current = cdr;
                continue;
            }

            // Fallback path: use try_pair for any other pair types
            let saved = current;
            let (car, cdr) = match heap.borrow().try_pair(current) {
                Some(pair) => pair,
                None => break,
            };
            if heap.borrow().tagged_values_equal(obj, car) {
                return Ok(saved);
            }
            current = cdr;
        }

        Ok(TaggedValue::FALSE)
    } else {
        // 3-arg path: custom comparator — apply() takes TaggedValue directly
        let obj = args[0];
        let compare_proc = args[2];

        let mut current = args[1];

        loop {
            if current.is_null() {
                break;
            }

            let saved = current;
            let (car_tv, cdr) = match heap.borrow().try_pair(current) {
                Some(pair) => pair,
                None => break,
            };

            let result = match evaluator.apply(compare_proc, vec![obj, car_tv], false)? {
                super::super::EvalResult::Tagged(tv) => tv != TaggedValue::FALSE,
                _ => {
                    return Err(EvalError::InternalError(
                        "Unexpected tail call in member comparison".to_string(),
                    ));
                }
            };

            if result {
                return Ok(saved);
            }
            current = cdr;
        }

        Ok(TaggedValue::FALSE)
    }
}

pub(super) fn assq(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let obj = args[0];
    let mut current = args[1];

    loop {
        if current.is_null() {
            break;
        }

        // Get (entry, rest) from alist spine
        let (entry, rest) = match heap.borrow().try_pair(current) {
            Some(pair) => pair,
            None => break,
        };

        // Get (entry_car, _) from entry pair
        let entry_car = heap.borrow().try_pair(entry).map(|(c, _)| c);
        if let Some(entry_car) = entry_car
            && heap.borrow().values_eq(obj, entry_car)
        {
            return Ok(entry);
        }

        current = rest;
    }

    Ok(TaggedValue::FALSE)
}

pub(super) fn assv(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let obj = args[0];
    let mut current = args[1];

    loop {
        if current.is_null() {
            break;
        }

        // Get (entry, rest) from alist spine
        let (entry, rest) = match heap.borrow().try_pair(current) {
            Some(pair) => pair,
            None => break,
        };

        // Get (entry_car, _) from entry pair
        let entry_car = heap.borrow().try_pair(entry).map(|(c, _)| c);
        if let Some(entry_car) = entry_car
            && heap.borrow().values_eqv(obj, entry_car)
        {
            return Ok(entry);
        }

        current = rest;
    }

    Ok(TaggedValue::FALSE)
}

pub(super) fn assoc(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "2 or 3".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();

    if args.len() == 2 {
        // 2-arg path: pure TaggedValue with heap.tagged_values_equal
        let obj = args[0];
        let mut current = args[1];

        loop {
            if current.is_null() {
                break;
            }

            // Get (entry, rest) from alist spine
            let (entry, rest) = match heap.borrow().try_pair(current) {
                Some(pair) => pair,
                None => break,
            };

            // Get (entry_car, _) from entry pair
            let entry_car = heap.borrow().try_pair(entry).map(|(c, _)| c);
            if let Some(entry_car) = entry_car
                && heap.borrow().tagged_values_equal(obj, entry_car)
            {
                return Ok(entry);
            }

            current = rest;
        }

        Ok(TaggedValue::FALSE)
    } else {
        // 3-arg path: custom comparator — apply() takes TaggedValue directly
        let obj = args[0];
        let compare_proc = args[2];

        let mut current = args[1];

        loop {
            if current.is_null() {
                break;
            }

            // Get (entry, rest) from alist spine
            let (entry, rest) = match heap.borrow().try_pair(current) {
                Some(pair) => pair,
                None => break,
            };

            // Get (entry_car, _) from entry pair
            let entry_car_tv = heap.borrow().try_pair(entry).map(|(c, _)| c);
            if let Some(entry_car_tv) = entry_car_tv {
                let result = match evaluator.apply(compare_proc, vec![obj, entry_car_tv], false)? {
                    super::super::EvalResult::Tagged(tv) => tv != TaggedValue::FALSE,
                    _ => {
                        return Err(EvalError::InternalError(
                            "Unexpected tail call in assoc comparison".to_string(),
                        ));
                    }
                };

                if result {
                    return Ok(entry);
                }
            }

            current = rest;
        }

        Ok(TaggedValue::FALSE)
    }
}

/// (make-list k [fill]) - Create list of k elements
pub(super) fn make_list(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();

    // Get the length
    let k = {
        let heap_ref = heap.borrow();
        let k = get_integer(args[0], &heap_ref, "make-list")?;
        if k < 0 {
            return Err(EvalError::TypeError(format!(
                "make-list: length must be non-negative, got {}",
                k
            )));
        }
        k as usize
    };

    // Get the fill value (default to unspecified) - keep as TaggedValue
    let fill = if args.len() == 2 {
        args[1]
    } else {
        TaggedValue::UNSPECIFIED
    };

    // Build list directly using heap allocation (no Value conversion)
    let mut result = TaggedValue::NULL;
    let mut heap_ref = heap.borrow_mut();
    for _ in 0..k {
        result = heap_ref.alloc_pair(fill, result);
    }

    Ok(result)
}

/// (list-copy list) - Create shallow copy of list
/// Handles both proper lists and improper lists (dotted lists)
pub(super) fn list_copy(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let input = args[0];

    // Handle empty list
    if input.is_null() {
        return Ok(TaggedValue::NULL);
    }

    // Fast path: native heap pair - copy using heap operations
    if input.is_pair() {
        let heap = evaluator.global_env.heap();

        // Collect all car values and find the tail
        let mut cars: Vec<TaggedValue> = Vec::new();
        let mut current = input;

        {
            let heap_ref = heap.borrow();
            while current.is_pair() {
                cars.push(heap_ref.car(current));
                current = heap_ref.cdr(current);
            }
        }

        // current is now the tail (Null for proper list, other value for improper list)
        let tail = current;

        // Build the copied list from right to left
        let mut result = tail;
        let mut heap_ref = heap.borrow_mut();
        for car in cars.into_iter().rev() {
            result = heap_ref.alloc_pair(car, result);
        }

        return Ok(result);
    }

    // Fallback path: use try_pair for any other pair types
    let heap = evaluator.global_env.heap();

    // Try to walk as a pair - collect cars and find tail
    let mut cars: Vec<TaggedValue> = Vec::new();
    let mut current = input;

    loop {
        if current.is_null() {
            break;
        }
        if let Some((car, cdr)) = heap.borrow().try_pair(current) {
            cars.push(car);
            current = cdr;
        } else {
            // Not a pair - improper list tail or non-pair input
            break;
        }
    }

    if cars.is_empty() {
        // Not a pair at all, return as-is
        return Ok(input);
    }

    // Build copied list from right to left
    let mut result = current; // tail (Null for proper list, other for improper)
    let mut heap_ref = heap.borrow_mut();
    for car in cars.into_iter().rev() {
        result = heap_ref.alloc_pair(car, result);
    }
    Ok(result)
}

/// (set-car! pair obj) - Mutate the car of a pair
pub(super) fn set_car(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    if args[0].is_pair() {
        let heap = evaluator.global_env.heap();
        heap.borrow_mut().set_car(args[0], args[1]);
        Ok(TaggedValue::UNSPECIFIED)
    } else {
        Err(EvalError::TypeError("set-car! expects a pair".to_string()))
    }
}

/// (set-cdr! pair obj) - Mutate the cdr of a pair
pub(super) fn set_cdr(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    if args[0].is_pair() {
        let heap = evaluator.global_env.heap();
        heap.borrow_mut().set_cdr(args[0], args[1]);
        Ok(TaggedValue::UNSPECIFIED)
    } else {
        Err(EvalError::TypeError("set-cdr! expects a pair".to_string()))
    }
}

/// (list-set! list k obj) - Set the k-th element of a list to obj
pub(super) fn list_set(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();

    // Get index first
    let index = {
        let heap_ref = heap.borrow();
        let index = get_integer(args[1], &heap_ref, "list-set!")?;
        if index < 0 {
            return Err(EvalError::TypeError(format!(
                "list-set!: index must be non-negative, got {}",
                index
            )));
        }
        index as usize
    };

    // Fast path: native heap pair - navigate using TaggedValue
    if args[0].is_pair() {
        let mut current = args[0];
        for i in 0..index {
            if !current.is_pair() {
                return Err(EvalError::TypeError(format!(
                    "list-set!: index {} out of bounds (list has {} elements)",
                    index, i
                )));
            }
            current = heap.borrow().cdr(current);
        }

        if current.is_pair() {
            heap.borrow_mut().set_car(current, args[2]);
            return Ok(TaggedValue::UNSPECIFIED);
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
        let (_, cdr) = heap.borrow().try_pair(current).ok_or_else(|| {
            EvalError::TypeError(format!(
                "list-set!: index {} out of bounds (list has {} elements)",
                index, i
            ))
        })?;
        current = cdr;
    }

    if current.is_pair() {
        heap.borrow_mut().set_car(current, args[2]);
        Ok(TaggedValue::UNSPECIFIED)
    } else {
        Err(EvalError::TypeError(format!(
            "list-set!: index {} out of bounds",
            index
        )))
    }
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Cons - construct pair
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "cons",
        Arity::Exact(2),
        "Returns a newly allocated pair whose car is obj1 and whose cdr is obj2.",
        |eval, args, _tail| cons(eval, args).map(EvalResult::Tagged),
    ));

    // Car - first element of pair
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "car",
        Arity::Exact(1),
        "Returns the contents of the car field of pair.",
        |eval, args, _tail| car(eval, args).map(EvalResult::Tagged),
    ));

    // Cdr - rest of pair
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "cdr",
        Arity::Exact(1),
        "Returns the contents of the cdr field of pair.",
        |eval, args, _tail| cdr(eval, args).map(EvalResult::Tagged),
    ));

    // List - construct list from arguments
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "list",
        Arity::Min(0),
        "Returns a newly allocated list of its arguments.",
        |eval, args, _tail| list(eval, args).map(EvalResult::Tagged),
    ));

    // Length - list length
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "length",
        Arity::Exact(1),
        "Returns the length of list.",
        |eval, args, _tail| length(eval, args).map(EvalResult::Tagged),
    ));

    // Append - concatenate lists
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "append",
        Arity::Min(0),
        "Returns a list consisting of the elements of the first list followed by the elements of the other lists.",
        |eval, args, _tail| append(eval, args).map(EvalResult::Tagged),
    ));

    // Reverse - reverse list
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "reverse",
        Arity::Exact(1),
        "Returns a newly allocated list consisting of the elements of list in reverse order.",
        |eval, args, _tail| reverse(eval, args).map(EvalResult::Tagged),
    ));

    // List-ref - nth element
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "list-ref",
        Arity::Exact(2),
        "Returns the kth element of list.",
        |eval, args, _tail| list_ref(eval, args).map(EvalResult::Tagged),
    ));

    // List-tail - drop first k elements
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "list-tail",
        Arity::Exact(2),
        "Returns the sublist of list obtained by omitting the first k elements.",
        |eval, args, _tail| list_tail(eval, args).map(EvalResult::Tagged),
    ));

    // Memq - member using eq?
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "memq",
        Arity::Exact(2),
        "Returns the first sublist of list whose car is obj (compared using eq?), or #f if not found.",
        |eval, args, _tail| memq(eval, args).map(EvalResult::Tagged),
    ));

    // Memv - member using eqv?
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "memv",
        Arity::Exact(2),
        "Returns the first sublist of list whose car is obj (compared using eqv?), or #f if not found.",
        |eval, args, _tail| memv(eval, args).map(EvalResult::Tagged),
    ));

    // Member - member using equal? (or custom comparator)
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "member",
        Arity::Range(2, 3),
        "Returns the first sublist of list whose car is obj. Uses equal? or optional comparator.",
        |eval, args, _tail| member(eval, args).map(EvalResult::Tagged),
    ));

    // Assq - association list lookup using eq?
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "assq",
        Arity::Exact(2),
        "Returns the first pair in alist whose car is obj (compared using eq?), or #f if not found.",
        |eval, args, _tail| assq(eval, args).map(EvalResult::Tagged),
    ));

    // Assv - association list lookup using eqv?
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "assv",
        Arity::Exact(2),
        "Returns the first pair in alist whose car is obj (compared using eqv?), or #f if not found.",
        |eval, args, _tail| assv(eval, args).map(EvalResult::Tagged),
    ));

    // Assoc - association list lookup using equal? (or custom comparator)
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "assoc",
        Arity::Range(2, 3),
        "Returns the first pair in alist whose car is obj. Uses equal? or optional comparator.",
        |eval, args, _tail| assoc(eval, args).map(EvalResult::Tagged),
    ));

    // make-list - Create list of k elements with optional fill value
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "make-list",
        Arity::Range(1, 2),
        "Returns a newly allocated list of k elements. If fill is given, each element is initialized to fill; otherwise unspecified.",
        |eval, args, _tail| make_list(eval, args).map(EvalResult::Tagged),
    ));

    // list-copy - Create shallow copy of list
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "list-copy",
        Arity::Exact(1),
        "Returns a newly allocated copy of list. Only the top level of structure is copied.",
        |eval, args, _tail| list_copy(eval, args).map(EvalResult::Tagged),
    ));

    // set-car! - Mutate car of pair
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "set-car!",
        Arity::Exact(2),
        "Stores obj in the car field of pair.",
        |eval, args, _tail| set_car(eval, args).map(EvalResult::Tagged),
    ));

    // set-cdr! - Mutate cdr of pair
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "set-cdr!",
        Arity::Exact(2),
        "Stores obj in the cdr field of pair.",
        |eval, args, _tail| set_cdr(eval, args).map(EvalResult::Tagged),
    ));

    // list-set! - Set element at index
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "list-set!",
        Arity::Exact(3),
        "Stores obj in the kth element of list.",
        |eval, args, _tail| list_set(eval, args).map(EvalResult::Tagged),
    ));
}
