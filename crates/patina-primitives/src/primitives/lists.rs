//! List and pair primitive operations (R7RS Section 6.4)
//!
//! Implements pair construction, list manipulation, and search operations.

use crate::apply_context::ApplyContext;
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

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

pub(super) fn cons(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    // Allocate native heap pair - args are already TaggedValue
    let pair_tv = heap.borrow_mut().alloc_pair(args[0], args[1]);
    Ok(pair_tv)
}

pub(super) fn car(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Fast path: native heap pair
    if args[0].is_pair() {
        let heap_ref = heap.borrow();
        let car_tv = heap_ref.car(args[0]);
        return Ok(car_tv);
    }

    // Fallback: use try_pair for any other pair types
    let (car, _) = heap
        .borrow()
        .try_pair(args[0])
        .ok_or_else(|| EvalError::TypeError("car expects a pair".into()))?;
    Ok(car)
}

pub(super) fn cdr(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Fast path: native heap pair
    if args[0].is_pair() {
        let heap_ref = heap.borrow();
        let cdr_tv = heap_ref.cdr(args[0]);
        return Ok(cdr_tv);
    }

    // Fallback: use try_pair for any other pair types
    let (_, cdr) = heap
        .borrow()
        .try_pair(args[0])
        .ok_or_else(|| EvalError::TypeError("cdr expects a pair".into()))?;
    Ok(cdr)
}

/// Define a car/cdr composition (caar, cadr, ..., cddddr), steps listed
/// innermost first — so `cadr` is `cdr` then `car`, exactly like the former
/// Scheme definition `(define (cadr x) (car (cdr x)))`.
///
/// Each step is a direct `Heap::car`/`Heap::cdr` under one borrow rather
/// than a chained call through the handler fns above: the handlers'
/// `try_pair` fallback only ever *fails* (it is `is_pair()`-gated, same as
/// the fast path), so guarding with `is_pair` and emitting the same
/// "car/cdr expects a pair" message is behavior-identical while skipping
/// the per-step arity check, slice temp, and RefCell borrow.
macro_rules! cxr {
    ($fname:ident, $($step:ident),+) => {
        pub(super) fn $fname(
            heap: &SharedHeap,
            args: &[TaggedValue],
        ) -> Result<TaggedValue, EvalError> {
            crate::registry::expect_arity(args, 1)?;
            let h = heap.borrow();
            let mut v = args[0];
            $(
                if !v.is_pair() {
                    return Err(EvalError::TypeError(
                        concat!(stringify!($step), " expects a pair").into(),
                    ));
                }
                v = h.$step(v);
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

pub(super) fn list(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    // list accepts any number of arguments (0 or more)
    // Use native heap list construction
    Ok(heap.borrow_mut().list_from_iter(args.iter().copied()))
}

pub(super) fn length(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

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

pub(super) fn append(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.is_empty() {
        return Ok(TaggedValue::NULL);
    }

    if args.len() == 1 {
        return Ok(args[0]);
    }

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

    // Prepend collected elements before the last arg
    Ok(heap
        .borrow_mut()
        .list_from_iter_with_tail(all_cars, args[args.len() - 1]))
}

pub(super) fn reverse(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

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

pub(super) fn list_ref(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

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

pub(super) fn list_tail(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

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

pub(super) fn memq(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
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

pub(super) fn memv(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
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
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "2 or 3".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

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

            let result = ctx.apply_proc(compare_proc, vec![obj, car_tv])? != TaggedValue::FALSE;

            if result {
                return Ok(saved);
            }
            current = cdr;
        }

        Ok(TaggedValue::FALSE)
    }
}

pub(super) fn assq(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
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

pub(super) fn assv(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
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
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "2 or 3".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

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
                let result =
                    ctx.apply_proc(compare_proc, vec![obj, entry_car_tv])? != TaggedValue::FALSE;

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
pub(super) fn make_list(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }

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
pub(super) fn list_copy(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
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
        return Ok(heap.borrow_mut().list_from_iter_with_tail(cars, current));
    }

    // Fallback path: use try_pair for any other pair types

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

    // tail: Null for a proper list, the non-pair value for an improper one
    Ok(heap.borrow_mut().list_from_iter_with_tail(cars, current))
}

/// (set-car! pair obj) - Mutate the car of a pair
pub(super) fn set_car(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    if args[0].is_pair() {
        heap.borrow_mut().set_car(args[0], args[1]);
        Ok(TaggedValue::UNSPECIFIED)
    } else {
        Err(EvalError::TypeError("set-car! expects a pair".to_string()))
    }
}

/// (set-cdr! pair obj) - Mutate the cdr of a pair
pub(super) fn set_cdr(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    if args[0].is_pair() {
        heap.borrow_mut().set_cdr(args[0], args[1]);
        Ok(TaggedValue::UNSPECIFIED)
    } else {
        Err(EvalError::TypeError("set-cdr! expects a pair".to_string()))
    }
}

/// (list-set! list k obj) - Set the k-th element of a list to obj
pub(super) fn list_set(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

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
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Cons - construct pair
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "cons",
        Arity::Exact(2),
        "Returns a newly allocated pair whose car is obj1 and whose cdr is obj2.",
        cons,
    ));

    // Car - first element of pair
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "car",
        Arity::Exact(1),
        "Returns the contents of the car field of pair.",
        car,
    ));

    // Cdr - rest of pair
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "cdr",
        Arity::Exact(1),
        "Returns the contents of the cdr field of pair.",
        cdr,
    ));

    // Car/cdr compositions (two- and three-deep, R7RS §6.4)
    for (name, handler) in [
        ("caar", caar as crate::registry::TaggedHandler),
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
        registry.register(PrimitiveFn::new_heap(
            "scheme.base",
            name,
            Arity::Exact(1),
            "Composition of car/cdr accessors.",
            handler,
        ));
    }

    // Four-deep compositions ((scheme cxr))
    for (name, handler) in [
        ("caaaar", caaaar as crate::registry::TaggedHandler),
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
        registry.register(PrimitiveFn::new_heap(
            "scheme.cxr",
            name,
            Arity::Exact(1),
            "Composition of car/cdr accessors.",
            handler,
        ));
    }

    // List - construct list from arguments
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "list",
        Arity::Min(0),
        "Returns a newly allocated list of its arguments.",
        list,
    ));

    // Length - list length
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "length",
        Arity::Exact(1),
        "Returns the length of list.",
        length,
    ));

    // Append - concatenate lists
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "append",
        Arity::Min(0),
        "Returns a list consisting of the elements of the first list followed by the elements of the other lists.",
        append,
    ));

    // Reverse - reverse list
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "reverse",
        Arity::Exact(1),
        "Returns a newly allocated list consisting of the elements of list in reverse order.",
        reverse,
    ));

    // List-ref - nth element
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "list-ref",
        Arity::Exact(2),
        "Returns the kth element of list.",
        list_ref,
    ));

    // List-tail - drop first k elements
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "list-tail",
        Arity::Exact(2),
        "Returns the sublist of list obtained by omitting the first k elements.",
        list_tail,
    ));

    // Memq - member using eq?
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "memq",
        Arity::Exact(2),
        "Returns the first sublist of list whose car is obj (compared using eq?), or #f if not found.",
        memq,
    ));

    // Memv - member using eqv?
    registry.register(PrimitiveFn::new_heap(
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
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "assq",
        Arity::Exact(2),
        "Returns the first pair in alist whose car is obj (compared using eq?), or #f if not found.",
        assq,
    ));

    // Assv - association list lookup using eqv?
    registry.register(PrimitiveFn::new_heap(
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
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "make-list",
        Arity::Range(1, 2),
        "Returns a newly allocated list of k elements. If fill is given, each element is initialized to fill; otherwise unspecified.",
        make_list,
    ));

    // list-copy - Create shallow copy of list
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "list-copy",
        Arity::Exact(1),
        "Returns a newly allocated copy of list. Only the top level of structure is copied.",
        list_copy,
    ));

    // set-car! - Mutate car of pair
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "set-car!",
        Arity::Exact(2),
        "Stores obj in the car field of pair.",
        set_car,
    ));

    // set-cdr! - Mutate cdr of pair
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "set-cdr!",
        Arity::Exact(2),
        "Stores obj in the cdr field of pair.",
        set_cdr,
    ));

    // list-set! - Set element at index
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "list-set!",
        Arity::Exact(3),
        "Stores obj in the kth element of list.",
        list_set,
    ));
}
